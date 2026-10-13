//! A collision shape on an object.
//!
//! The inspector edits a flat set of fields (a shape kind plus the dimensions
//! that kind uses); [`ColliderComponent::shape`] folds them into a
//! [`ColliderShapeData`] for the simulation.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use engine_class_derive::{engine_class, register_world_component};
use pulsar_reflection::Reflectable;
use pulsar_scenedb::{component_id, Entity, World};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::behavior::{read_rows, Output, Part, PhysEvent, PhysicsBehavior, Row, Rows, Touched};
use crate::declare_events;
use crate::math::pose;
use crate::sim::{ColliderInfo, Sim};

/// The class name scripts and level files use.
pub const COLLIDER_CLASS_NAME: &str = "ColliderComponent";

/// Which shape a collider has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflectable)]
pub enum ColliderShape {
    /// Axis-aligned in the owner's space; uses `half_extents`.
    Box,
    /// Uses `radius`.
    Sphere,
    /// Uses `radius`, `height` and `axis`.
    Capsule,
    /// Uses `radius`, `height` and `axis`.
    Cylinder,
    /// Uses `radius`, `height` and `axis`.
    Cone,
    /// Convex hull of the mesh at `mesh`; dynamic bodies can use it.
    ConvexHull,
    /// Exact triangles of the mesh at `mesh`; static and kinematic bodies only.
    TriangleMesh,
}

impl Default for ColliderShape {
    fn default() -> Self {
        Self::Box
    }
}

/// The axis a capsule, cylinder or cone runs along, in the owner's space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflectable)]
pub enum ColliderAxis {
    X,
    Y,
    Z,
}

impl Default for ColliderAxis {
    fn default() -> Self {
        Self::Y
    }
}

/// A collider's shape with the dimensions that shape uses.
#[derive(Clone, Debug, PartialEq)]
pub enum ColliderShapeData {
    Box { half_extents: [f32; 3] },
    Sphere { radius: f32 },
    Capsule { radius: f32, height: f32, axis: ColliderAxis },
    Cylinder { radius: f32, height: f32, axis: ColliderAxis },
    Cone { radius: f32, height: f32, axis: ColliderAxis },
    ConvexHull { mesh: String },
    TriangleMesh { mesh: String },
}

#[engine_class(category = "Physics", clone, debug, serialize, deserialize)]
#[category("Shape", category_color = "#FF6B6B")]
#[category("Filtering", category_color = "#4ECDC4")]
#[serde(default)]
pub struct ColliderComponent {
    #[property(category = "Shape")]
    pub enabled: bool,
    #[property(category = "Shape")]
    pub shape: ColliderShape,
    /// Offset of the shape from the owner's origin, in the owner's space.
    #[property(category = "Shape")]
    pub center: [f32; 3],
    /// Box: half the size along each axis.
    #[property(min = 0.0, max = 100000.0, step = 0.01, category = "Shape")]
    pub half_extents: [f32; 3],
    /// Sphere, capsule, cylinder, cone.
    #[property(min = 0.0, max = 100000.0, step = 0.01, category = "Shape")]
    pub radius: f32,
    /// Capsule (cylindrical part), cylinder and cone: full length along `axis`.
    #[property(min = 0.0, max = 100000.0, step = 0.01, category = "Shape")]
    pub height: f32,
    #[property(category = "Shape")]
    pub axis: ColliderAxis,
    /// Convex hull and triangle mesh: project-relative path of the mesh.
    #[property(category = "Shape")]
    pub mesh: String,
    /// Reports overlaps instead of blocking.
    #[property(category = "Filtering")]
    pub is_trigger: bool,
    /// Project-relative path of a `.physmat` asset. Empty: engine defaults.
    #[property(category = "Filtering")]
    pub physics_material: String,
    /// Name of a layer in the project's `.physlayers` asset.
    #[property(category = "Filtering")]
    pub layer: String,
}

impl Default for ColliderComponent {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: ColliderShape::Box,
            center: [0.0; 3],
            half_extents: [0.5; 3],
            radius: 0.5,
            height: 1.0,
            axis: ColliderAxis::Y,
            mesh: String::new(),
            is_trigger: false,
            physics_material: String::new(),
            layer: "Default".into(),
        }
    }
}

impl ColliderComponent {
    /// The shape with its dimensions, negative sizes clamped to zero.
    pub fn shape(&self) -> ColliderShapeData {
        let size = |v: f32| v.max(0.0);
        let (radius, height, axis) = (size(self.radius), size(self.height), self.axis);
        match self.shape {
            ColliderShape::Box => ColliderShapeData::Box {
                half_extents: self.half_extents.map(size),
            },
            ColliderShape::Sphere => ColliderShapeData::Sphere { radius },
            ColliderShape::Capsule => ColliderShapeData::Capsule { radius, height, axis },
            ColliderShape::Cylinder => ColliderShapeData::Cylinder { radius, height, axis },
            ColliderShape::Cone => ColliderShapeData::Cone { radius, height, axis },
            ColliderShape::ConvexHull => ColliderShapeData::ConvexHull { mesh: self.mesh.clone() },
            ColliderShape::TriangleMesh => {
                ColliderShapeData::TriangleMesh { mesh: self.mesh.clone() }
            }
        }
    }
}

#[register_world_component]
impl ColliderComponent {}

// ---------------------------------------------------------------------------
// Physics behavior
// ---------------------------------------------------------------------------

declare_events! {
    /// This collider started touching `other`.
    pub const COLLISION_ENTER = (COLLIDER_CLASS_NAME, "collision_enter");
    /// This collider stopped touching `other`.
    pub const COLLISION_EXIT = (COLLIDER_CLASS_NAME, "collision_exit");
    /// `other` entered this trigger.
    pub const TRIGGER_ENTER = (COLLIDER_CLASS_NAME, "trigger_enter");
    /// `other` left this trigger.
    pub const TRIGGER_EXIT = (COLLIDER_CLASS_NAME, "trigger_exit");
}

impl ColliderComponent {
    /// Hash of everything that decides the Rapier shape; a change rebuilds it.
    fn signature(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match self.shape() {
            ColliderShapeData::Box { half_extents } => {
                0u8.hash(&mut h);
                half_extents.map(f32::to_bits).hash(&mut h);
            }
            ColliderShapeData::Sphere { radius } => {
                1u8.hash(&mut h);
                radius.to_bits().hash(&mut h);
            }
            ColliderShapeData::Capsule { radius, height, axis } => {
                2u8.hash(&mut h);
                (radius.to_bits(), height.to_bits(), axis as u8).hash(&mut h);
            }
            ColliderShapeData::Cylinder { radius, height, axis } => {
                3u8.hash(&mut h);
                (radius.to_bits(), height.to_bits(), axis as u8).hash(&mut h);
            }
            ColliderShapeData::Cone { radius, height, axis } => {
                4u8.hash(&mut h);
                (radius.to_bits(), height.to_bits(), axis as u8).hash(&mut h);
            }
            ColliderShapeData::ConvexHull { mesh } | ColliderShapeData::TriangleMesh { mesh } => {
                5u8.hash(&mut h);
                mesh.hash(&mut h);
            }
        }
        self.center.map(f32::to_bits).hash(&mut h);
        self.is_trigger.hash(&mut h);
        h.finish()
    }

    /// Rotation taking Rapier's Y-aligned capsule/cylinder/cone onto `axis`.
    fn axis_rotation(axis: ColliderAxis) -> [f32; 3] {
        match axis {
            ColliderAxis::Y => [0.0, 0.0, 0.0],
            ColliderAxis::X => [0.0, 0.0, -90.0],
            ColliderAxis::Z => [90.0, 0.0, 0.0],
        }
    }

    /// The collider's pose in its owner's space.
    fn local_pose(&self) -> Pose {
        let rotation = match self.shape() {
            ColliderShapeData::Capsule { axis, .. }
            | ColliderShapeData::Cylinder { axis, .. }
            | ColliderShapeData::Cone { axis, .. } => Self::axis_rotation(axis),
            _ => [0.0; 3],
        };
        pose(self.center, rotation)
    }

    /// `None` for shapes that need a mesh asset; those are not simulated yet.
    fn builder(&self) -> Option<ColliderBuilder> {
        let builder = match self.shape() {
            ColliderShapeData::Box { half_extents: [x, y, z] } => ColliderBuilder::cuboid(x, y, z),
            ColliderShapeData::Sphere { radius } => ColliderBuilder::ball(radius),
            ColliderShapeData::Capsule { radius, height, .. } => {
                ColliderBuilder::capsule_y(height * 0.5, radius)
            }
            ColliderShapeData::Cylinder { radius, height, .. } => {
                ColliderBuilder::cylinder(height * 0.5, radius)
            }
            ColliderShapeData::Cone { radius, height, .. } => {
                ColliderBuilder::cone(height * 0.5, radius)
            }
            ColliderShapeData::ConvexHull { .. } | ColliderShapeData::TriangleMesh { .. } => {
                tracing::warn!("mesh colliders are not simulated yet; skipping");
                return None;
            }
        };
        Some(
            builder
                .sensor(self.is_trigger)
                .active_events(ActiveEvents::COLLISION_EVENTS),
        )
    }
}

/// What the simulation thread remembers about one collider component.
struct Tracked {
    handle: ColliderHandle,
    owner: Entity,
    signature: u64,
    /// The body it is attached to, if any.
    parent: Option<RigidBodyHandle>,
    /// The collider's pose in its owner's space.
    local: Pose,
    /// The owner's pose when the collider is not attached to a body.
    owner_pose: Pose,
}

#[derive(Default)]
struct State {
    colliders: HashMap<Entity, Tracked>,
}

fn read(world: &mut World, touched: &Touched) -> Option<Part> {
    read_rows::<ColliderComponent>(world, touched, |c| c.enabled).map(|rows| Box::new(rows) as Part)
}

fn forget(state: &mut State, sim: &mut Sim, instance: Entity) {
    if let Some(t) = state.colliders.remove(&instance) {
        sim.collider_info.remove(&t.handle);
        sim.colliders
            .remove(t.handle, &mut sim.islands, &mut sim.bodies, true);
    }
}

fn place(sim: &mut Sim, t: &Tracked) {
    if let Some(collider) = sim.colliders.get_mut(t.handle) {
        if t.parent.is_some() {
            collider.set_position_wrt_parent(t.local);
        } else {
            collider.set_position(t.owner_pose * t.local);
        }
    }
}

fn apply(sim: &mut Sim, part: Part) {
    let Ok(rows) = part.downcast::<Rows<ColliderComponent>>() else {
        return;
    };
    sim.with_state::<State, _>(|state, sim| {
        if rows.replace {
            let live: HashSet<Entity> = rows.upsert.iter().map(|r| r.instance).collect();
            let stale: Vec<Entity> = state
                .colliders
                .keys()
                .filter(|i| !live.contains(i))
                .copied()
                .collect();
            for instance in stale {
                forget(state, sim, instance);
            }
        }
        for instance in rows.remove {
            forget(state, sim, instance);
        }
        for row in rows.upsert {
            upsert(state, sim, row);
        }
    });
}

fn upsert(state: &mut State, sim: &mut Sim, row: Row<ColliderComponent>) {
    let parent = sim.owner_body.get(&row.owner).copied();
    let signature = row.cfg.signature();
    let owner_pose = pose(row.transform.position, row.transform.rotation);
    if let Some(t) = state.colliders.get_mut(&row.instance) {
        if t.signature == signature {
            if row.moved {
                t.owner_pose = owner_pose;
                place(sim, t);
            }
            return;
        }
        forget(state, sim, row.instance);
    }
    let Some(builder) = row.cfg.builder() else {
        return;
    };
    let local = row.cfg.local_pose();
    let handle = match parent {
        Some(body) => sim.colliders.insert_with_parent(
            builder.position(local).build(),
            body,
            &mut sim.bodies,
        ),
        None => sim.colliders.insert(builder.position(owner_pose * local).build()),
    };
    sim.collider_info.insert(
        handle,
        ColliderInfo {
            instance: row.instance,
            owner: row.owner,
            sensor: row.cfg.is_trigger,
        },
    );
    state.colliders.insert(
        row.instance,
        Tracked {
            handle,
            owner: row.owner,
            signature,
            parent,
            local,
            owner_pose,
        },
    );
}

/// Colliders follow their object's body: when it appears or goes away,
/// attach to it or stand free where it left them.
fn settle(sim: &mut Sim) {
    if sim.dirty.is_empty() {
        return;
    }
    sim.with_state::<State, _>(|state, sim| {
        for t in state.colliders.values_mut() {
            if !sim.dirty.contains(&t.owner) {
                continue;
            }
            let parent = sim.owner_body.get(&t.owner).copied();
            if parent == t.parent {
                continue;
            }
            // Keep the world pose across the switch.
            if parent.is_none() {
                if let Some(c) = sim.colliders.get(t.handle) {
                    t.owner_pose = *c.position() * t.local.inverse();
                }
            }
            sim.colliders.set_parent(t.handle, parent, &mut sim.bodies);
            t.parent = parent;
            place(sim, t);
        }
    });
}

/// Turn Rapier's contact and sensor events into physics events.
fn after_steps(sim: &mut Sim, _seconds: f32, out: &mut Output) {
    for event in sim.take_collisions() {
        let (Some(a), Some(b)) = (
            sim.collider_info.get(&event.collider1()).copied(),
            sim.collider_info.get(&event.collider2()).copied(),
        ) else {
            continue;
        };
        if a.owner == b.owner {
            continue;
        }
        let started = event.started();
        if a.sensor || b.sensor {
            // The trigger's owner hears about whoever entered it.
            for (trigger, visitor) in [(a, b), (b, a)] {
                if trigger.sensor {
                    out.events.push(PhysEvent {
                        spec: if started { TRIGGER_ENTER } else { TRIGGER_EXIT },
                        owner: trigger.owner,
                        component: trigger.instance,
                        other: Some(visitor.owner),
                    });
                }
            }
        } else {
            for (this, other) in [(a, b), (b, a)] {
                out.events.push(PhysEvent {
                    spec: if started { COLLISION_ENTER } else { COLLISION_EXIT },
                    owner: this.owner,
                    component: this.instance,
                    other: Some(other.owner),
                });
            }
        }
    }
}

inventory::submit! {
    PhysicsBehavior {
        name: "collider",
        order: 10,
        component: component_id::<ColliderComponent>,
        read,
        apply,
        settle: Some(settle),
        before_step: None,
        after_steps: Some(after_steps),
        write_back: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_uses_only_the_fields_of_its_kind() {
        let mut c = ColliderComponent::default();
        assert_eq!(c.shape(), ColliderShapeData::Box { half_extents: [0.5; 3] });
        c.shape = ColliderShape::Capsule;
        c.axis = ColliderAxis::Z;
        assert_eq!(
            c.shape(),
            ColliderShapeData::Capsule { radius: 0.5, height: 1.0, axis: ColliderAxis::Z }
        );
        c.shape = ColliderShape::ConvexHull;
        c.mesh = "meshes/rock.mesh".into();
        assert_eq!(c.shape(), ColliderShapeData::ConvexHull { mesh: "meshes/rock.mesh".into() });
    }

    #[test]
    fn negative_sizes_clamp_to_zero() {
        let mut c = ColliderComponent::default();
        c.half_extents = [-1.0, 2.0, 0.0];
        assert_eq!(c.shape(), ColliderShapeData::Box { half_extents: [0.0, 2.0, 0.0] });
    }
}
