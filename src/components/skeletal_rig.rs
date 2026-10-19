//! Articulated Rapier instances driven by a mesh's matching `.physasset`.
//!
//! The skeletal mesh component supplies the evaluated model-local pose. The
//! physics behavior creates one Rapier body/collider per authored bone, joins
//! them with the authored constraints, and writes dynamic body poses back to
//! the component so its skin palette follows the simulation.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use helio_component::SkeletalMeshComponent;
use pulsar_physics::{SkeletalJointKind, SkeletalMotion, SkeletalPhysicsAsset, SkeletalShape};
use pulsar_scenedb::{component_id, Entity, World};
use pulsar_world_registry::pulsar_scene_model::Transform;
use rapier3d::prelude::*;

use crate::behavior::{read_rows, Output, Part, PhysicsBehavior, Touched, WriteCtx};
use crate::math::{pose, simulated, vector};
use crate::sim::Sim;

const NAME: &str = "skeletal_rig";
const ORDER: i32 = 15;

#[derive(Clone)]
struct RigRow {
    instance: Entity,
    owner: Entity,
    moved: bool,
    transform: Transform,
    root_scale: [f32; 3],
    asset: Option<SkeletalPhysicsAsset>,
    bone_ids: Vec<String>,
    bone_pose: Vec<[[f32; 4]; 4]>,
}

struct RigRows {
    replace: bool,
    upsert: Vec<RigRow>,
    remove: Vec<Entity>,
}

fn read(world: &mut World, touched: &Touched) -> Option<Part> {
    let rows = read_rows::<SkeletalMeshComponent>(world, touched, |_| true)?;
    let upsert = rows
        .upsert
        .into_iter()
        .map(|row| {
            let (asset, bone_ids) = row
                .cfg
                .mesh
                .as_ref()
                .and_then(|mesh| load_asset(mesh.as_str()).map(|asset| (asset, mesh.as_str())))
                .map(|(asset, _)| {
                    let ids = row
                        .cfg
                        .loaded_skeleton
                        .as_ref()
                        .map(|skeleton| {
                            skeleton
                                .skeleton()
                                .bones
                                .into_iter()
                                .map(|bone| bone.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    (Some(asset), ids)
                })
                .unwrap_or((None, Vec::new()));
            RigRow {
                instance: row.instance,
                owner: row.owner,
                moved: row.moved,
                transform: row.transform,
                root_scale: safe_root_scale(row.transform.scale),
                asset,
                bone_ids,
                bone_pose: row.cfg.bone_world_pose.clone(),
            }
        })
        .collect();
    Some(Box::new(RigRows {
        replace: rows.replace,
        upsert,
        remove: rows.remove,
    }))
}

#[derive(Default)]
struct AssetCache {
    entries: HashMap<(PathBuf, String), (Instant, Option<SkeletalPhysicsAsset>)>,
    warned_ambiguous: HashSet<(PathBuf, String)>,
}

static ASSETS: LazyLock<Mutex<AssetCache>> = LazyLock::new(|| Mutex::new(AssetCache::default()));

fn load_asset(mesh_id: &str) -> Option<SkeletalPhysicsAsset> {
    let root = PathBuf::from(engine_state::get_project_path()?);
    let key = (root.clone(), mesh_id.to_owned());
    let mut cache = ASSETS.lock().ok()?;
    if let Some((checked, asset)) = cache.entries.get(&key) {
        if checked.elapsed() < Duration::from_secs(1) {
            return asset.clone();
        }
    }

    let asset = (|| {
        let index = engine_fs::open_project_file_index(&root).ok().flatten()?;
        let files = index
            .query(&engine_fs::FileIndexQuery {
                extension: Some("physasset".to_owned()),
                sort: engine_fs::FileIndexSort::Path,
                ..Default::default()
            })
            .ok()?;
        let mut matching = None;
        let mut matching_paths = Vec::new();
        for file in files {
            let path = index
                .project_root()
                .join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR));
            let Ok(bytes) = engine_fs::virtual_fs::read_file(&path) else {
                continue;
            };
            let Ok(candidate) = serde_json::from_slice::<SkeletalPhysicsAsset>(&bytes) else {
                continue;
            };
            if candidate.skeletal_mesh.as_str() == mesh_id {
                matching_paths.push(path);
                if matching.is_none() {
                    matching = Some(candidate);
                }
            }
        }
        if matching_paths.len() > 1 {
            if cache.warned_ambiguous.insert(key.clone()) {
                tracing::warn!(
                    mesh_id,
                    paths = ?matching_paths,
                    "multiple .physasset files reference this skeletal mesh; skipping physics rig"
                );
            }
            return None;
        }
        cache.warned_ambiguous.remove(&key);
        matching
    })();
    cache.entries.insert(key, (Instant::now(), asset.clone()));
    asset
}

struct TrackedRig {
    owner: Entity,
    root_pose: Pose,
    root_scale: [f32; 3],
    asset: Option<SkeletalPhysicsAsset>,
    bone_ids: Vec<String>,
    bone_pose: Vec<[[f32; 4]; 4]>,
    bodies: HashMap<String, RigidBodyHandle>,
    colliders: Vec<ColliderHandle>,
    joints: Vec<ImpulseJointHandle>,
}

#[derive(Default)]
struct State {
    rigs: HashMap<Entity, TrackedRig>,
    released: Vec<Entity>,
}

fn apply(sim: &mut Sim, part: Part) {
    let Ok(rows) = part.downcast::<RigRows>() else {
        return;
    };
    sim.with_state::<State, _>(|state, sim| {
        if rows.replace {
            let live: HashSet<_> = rows.upsert.iter().map(|row| row.instance).collect();
            let stale: Vec<_> = state
                .rigs
                .keys()
                .filter(|instance| !live.contains(instance))
                .copied()
                .collect();
            for instance in stale {
                forget(state, sim, instance, true);
            }
        }
        for instance in rows.remove {
            forget(state, sim, instance, true);
        }
        for row in rows.upsert {
            let Some(asset) = row.asset else {
                forget(state, sim, row.instance, true);
                continue;
            };
            if row.bone_ids.is_empty() || row.bone_pose.len() != row.bone_ids.len() {
                forget(state, sim, row.instance, true);
                continue;
            }
            let root = pose(simulated(row.transform.position), row.transform.rotation);
            let rebuild = state.rigs.get(&row.instance).is_none_or(|rig| {
                rig.owner != row.owner
                    || rig.asset.as_ref() != Some(&asset)
                    || rig.bone_ids != row.bone_ids
                    || rig.root_scale != row.root_scale
            });
            if rebuild {
                forget(state, sim, row.instance, false);
                let mut rig = TrackedRig {
                    owner: row.owner,
                    root_pose: root,
                    root_scale: row.root_scale,
                    asset: Some(asset.clone()),
                    bone_ids: row.bone_ids.clone(),
                    bone_pose: row.bone_pose.clone(),
                    bodies: HashMap::new(),
                    colliders: Vec::new(),
                    joints: Vec::new(),
                };
                create_rig(
                    sim,
                    &mut rig,
                    &asset,
                    &row.bone_ids,
                    &row.bone_pose,
                    root,
                    row.root_scale,
                );
                state.rigs.insert(row.instance, rig);
            } else if let Some(rig) = state.rigs.get_mut(&row.instance) {
                rig.bone_pose = row.bone_pose.clone();
                if row.moved {
                    let delta = root * rig.root_pose.inverse();
                    for handle in rig.bodies.values() {
                        if let Some(body) = sim.bodies.get_mut(*handle) {
                            body.set_position(delta * *body.position(), true);
                        }
                    }
                    rig.root_pose = root;
                }
                // Dynamic bodies own their pose after creation. Kinematic
                // bodies continue following the animated source pose.
                if let Some(asset) = &rig.asset {
                    let poses: HashMap<_, _> = row
                        .bone_ids
                        .iter()
                        .zip(&row.bone_pose)
                        .map(|(id, matrix)| (id.as_str(), matrix))
                        .collect();
                    for body_cfg in &asset.bodies {
                        if body_cfg.motion != SkeletalMotion::Kinematic {
                            continue;
                        }
                        let (Some(handle), Some(matrix)) = (
                            rig.bodies.get(&body_cfg.bone_id),
                            poses.get(body_cfg.bone_id.as_str()),
                        ) else {
                            continue;
                        };
                        if let Some(body) = sim.bodies.get_mut(*handle) {
                            body.set_next_kinematic_position(
                                root * scale_local_pose(pose_from_matrix(matrix), row.root_scale),
                            );
                        }
                    }
                }
            }
        }
    });
}

fn forget(state: &mut State, sim: &mut Sim, instance: Entity, release_pose: bool) {
    let Some(mut rig) = state.rigs.remove(&instance) else {
        return;
    };
    for joint in rig.joints.drain(..) {
        sim.impulse_joints.remove(joint, true);
    }
    for collider in rig.colliders.drain(..) {
        sim.collider_info.remove(&collider);
        sim.colliders
            .remove(collider, &mut sim.islands, &mut sim.bodies, true);
    }
    for handle in rig.bodies.values() {
        sim.bodies.remove(
            *handle,
            &mut sim.islands,
            &mut sim.colliders,
            &mut sim.impulse_joints,
            &mut sim.multibody_joints,
            true,
        );
    }
    if release_pose {
        state.released.push(instance);
    }
}

fn create_rig(
    sim: &mut Sim,
    rig: &mut TrackedRig,
    asset: &SkeletalPhysicsAsset,
    bone_ids: &[String],
    bone_pose: &[[[f32; 4]; 4]],
    root: Pose,
    root_scale: [f32; 3],
) {
    let indices: HashMap<_, _> = bone_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect();
    for cfg in &asset.bodies {
        let Some(&index) = indices.get(cfg.bone_id.as_str()) else {
            continue;
        };
        let local = scale_local_pose(pose_from_matrix(&bone_pose[index]), root_scale);
        let body_type = match cfg.motion {
            SkeletalMotion::Dynamic => RigidBodyType::Dynamic,
            SkeletalMotion::Kinematic => RigidBodyType::KinematicPositionBased,
            SkeletalMotion::Fixed => RigidBodyType::Fixed,
        };
        let mut builder = RigidBodyBuilder::new(body_type).pose(root * local);
        if cfg.mass.is_finite() && cfg.mass > 0.0 {
            builder = builder.additional_mass(cfg.mass);
        }
        let handle = sim.bodies.insert(builder.build());
        rig.bodies.insert(cfg.bone_id.clone(), handle);

        let dims = cfg.dimensions.map(|value| {
            if value.is_finite() {
                value.abs().clamp(0.005, 1.0e4)
            } else {
                0.05
            }
        });
        // Rapier's primitive colliders do not support general non-uniform
        // scaling. Scale boxes per axis; for round primitives, use the larger
        // radial factor so the collider conservatively contains the scaled
        // shape instead of silently under-covering it.
        let shape_scale = root_scale.map(|value| value.abs().max(1.0e-6));
        let radial_scale = shape_scale[0].max(shape_scale[2]);
        let scaled_dim = |dimension: f32, scale: f32| (dimension * scale).clamp(0.005, 1.0e8);
        let shape = match cfg.shape {
            SkeletalShape::Box => SharedShape::cuboid(
                scaled_dim(dims[0], shape_scale[0]),
                scaled_dim(dims[1], shape_scale[1]),
                scaled_dim(dims[2], shape_scale[2]),
            ),
            SkeletalShape::Sphere => SharedShape::ball(scaled_dim(
                dims[0],
                shape_scale.into_iter().fold(0.0, f32::max),
            )),
            SkeletalShape::Capsule => SharedShape::capsule_y(
                scaled_dim(dims[1], shape_scale[1]),
                scaled_dim(dims[0], radial_scale.max(shape_scale[1])),
            ),
            SkeletalShape::Cylinder => SharedShape::cylinder(
                scaled_dim(dims[1], shape_scale[1]),
                scaled_dim(dims[0], radial_scale),
            ),
        };
        let center = scale_vector(cfg.center, root_scale);
        let collider = ColliderBuilder::new(shape)
            .position(pose(center, [0.0; 3]))
            .active_events(ActiveEvents::COLLISION_EVENTS)
            .build();
        let collider = sim
            .colliders
            .insert_with_parent(collider, handle, &mut sim.bodies);
        rig.colliders.push(collider);
        sim.collider_info.insert(
            collider,
            crate::sim::ColliderInfo {
                instance: rig.owner,
                owner: rig.owner,
                sensor: false,
            },
        );
    }

    for cfg in &asset.constraints {
        let (Some(&a), Some(&b)) = (
            rig.bodies.get(&cfg.bone_id),
            rig.bodies.get(&cfg.connected_bone_id),
        ) else {
            continue;
        };
        let axis = vector(cfg.axis);
        let a1 = vector(scale_vector(cfg.anchor, root_scale));
        let a2 = vector(scale_vector(cfg.connected_anchor, root_scale));
        let joint: GenericJoint = match cfg.kind {
            SkeletalJointKind::Fixed => FixedJointBuilder::new()
                .local_anchor1(a1)
                .local_anchor2(a2)
                .build()
                .into(),
            SkeletalJointKind::Hinge => {
                let mut builder = RevoluteJointBuilder::new(axis)
                    .local_anchor1(a1)
                    .local_anchor2(a2);
                if cfg.limits_enabled {
                    builder = builder.limits([
                        cfg.limits_degrees[0]
                            .min(cfg.limits_degrees[1])
                            .to_radians(),
                        cfg.limits_degrees[0]
                            .max(cfg.limits_degrees[1])
                            .to_radians(),
                    ]);
                }
                builder.build().into()
            }
            SkeletalJointKind::BallSocket => {
                let mut builder = SphericalJointBuilder::new()
                    .local_anchor1(a1)
                    .local_anchor2(a2);
                if cfg.limits_enabled {
                    let limits = [
                        cfg.limits_degrees[0]
                            .min(cfg.limits_degrees[1])
                            .to_radians(),
                        cfg.limits_degrees[0]
                            .max(cfg.limits_degrees[1])
                            .to_radians(),
                    ];
                    builder = builder
                        .limits(JointAxis::AngX, limits)
                        .limits(JointAxis::AngY, limits)
                        .limits(JointAxis::AngZ, limits);
                }
                builder.build().into()
            }
        };
        rig.joints
            .push(sim.impulse_joints.insert(a, b, joint, true));
    }
}

fn pose_from_matrix(matrix: &[[f32; 4]; 4]) -> Pose {
    let transform = glam::Mat4::from_cols_array_2d(matrix);
    let (_, rotation, translation) = transform.to_scale_rotation_translation();
    let rotation = Rotation::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w);
    Pose::from_parts(vector(translation.to_array()), rotation)
}

fn safe_root_scale(scale: [f32; 3]) -> [f32; 3] {
    scale.map(|value| {
        if value.is_finite() {
            value.clamp(-1.0e4, 1.0e4)
        } else {
            1.0
        }
    })
}

fn scale_vector(value: [f32; 3], scale: [f32; 3]) -> [f32; 3] {
    [
        value[0] * scale[0],
        value[1] * scale[1],
        value[2] * scale[2],
    ]
}

fn scale_local_pose(mut local: Pose, scale: [f32; 3]) -> Pose {
    let p = local.translation;
    local.translation = vector(scale_vector([p.x, p.y, p.z], scale));
    local
}

fn unscale_vector(value: [f32; 3], scale: [f32; 3]) -> [f32; 3] {
    [
        if scale[0].abs() > 1.0e-6 {
            value[0] / scale[0]
        } else {
            0.0
        },
        if scale[1].abs() > 1.0e-6 {
            value[1] / scale[1]
        } else {
            0.0
        },
        if scale[2].abs() > 1.0e-6 {
            value[2] / scale[2]
        } else {
            0.0
        },
    ]
}

struct RigPoseOut {
    instance: Entity,
    pose: Vec<[[f32; 4]; 4]>,
}

struct RigOutput {
    poses: Vec<RigPoseOut>,
    released: Vec<Entity>,
}

fn after_steps(sim: &mut Sim, _seconds: f32, out: &mut Output) {
    let mut batch = RigOutput {
        poses: Vec::new(),
        released: Vec::new(),
    };
    sim.with_state::<State, _>(|state, sim| {
        for (&instance, rig) in &state.rigs {
            let Some(asset) = &rig.asset else { continue };
            let mut pose = rig.bone_pose.clone();
            let index: HashMap<_, _> = rig
                .bone_ids
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            let inverse_root = rig.root_pose.inverse();
            for body in &asset.bodies {
                if body.motion != SkeletalMotion::Dynamic {
                    continue;
                }
                let (Some(&slot), Some(handle)) = (
                    index.get(body.bone_id.as_str()),
                    rig.bodies.get(&body.bone_id),
                ) else {
                    continue;
                };
                let Some(rigid_body) = sim.bodies.get(*handle) else {
                    continue;
                };
                let local = inverse_root * *rigid_body.position();
                let rotation = local.rotation;
                let translation = local.translation;
                let translation = unscale_vector(
                    [translation.x, translation.y, translation.z],
                    rig.root_scale,
                );
                pose[slot] = glam::Mat4::from_rotation_translation(
                    glam::Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
                    glam::Vec3::from_array(translation),
                )
                .to_cols_array_2d();
            }
            batch.poses.push(RigPoseOut { instance, pose });
        }
        batch.released = std::mem::take(&mut state.released);
    });
    if !batch.poses.is_empty() || !batch.released.is_empty() {
        out.batches.push((NAME, Box::new(batch)));
    }
}

fn write_back(world: &mut World, part: Part, _ctx: &mut WriteCtx) {
    let Ok(batch) = part.downcast::<RigOutput>() else {
        return;
    };
    for output in batch.poses {
        if let Some(mut component) = world.get_mut::<SkeletalMeshComponent>(output.instance) {
            component.apply_physics_pose(output.pose);
        }
    }
    for instance in batch.released {
        if let Some(mut component) = world.get_mut::<SkeletalMeshComponent>(instance) {
            component.clear_physics_pose();
        }
    }
}

inventory::submit! {
    PhysicsBehavior {
        name: NAME,
        order: ORDER,
        component: component_id::<SkeletalMeshComponent>,
        read,
        apply,
        settle: None,
        before_step: None,
        after_steps: Some(after_steps),
        write_back: Some(write_back),
    }
}
