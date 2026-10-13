//! A kinematic capsule mover (player or NPC) with step and slope handling.

use std::collections::{HashMap, HashSet};

use engine_class_derive::{engine_class, register_world_component};
use pulsar_scenedb::{component_id, Entity, World};
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::parry::query::DefaultQueryDispatcher;
use rapier3d::pipeline::QueryFilter;
use rapier3d::prelude::*;

use crate::behavior::{read_rows, Output, Part, PhysEvent, PhysicsBehavior, Rows, Touched, WriteCtx};
use crate::declare_events;
use crate::math::{euler_degrees_from_quat, pose, vector, array};
use crate::sim::{ColliderInfo, Sim, GRAVITY};

/// The class name scripts and level files use.
pub const CHARACTER_CONTROLLER_CLASS_NAME: &str = "CharacterControllerComponent";

#[engine_class(category = "Physics", clone, debug, serialize, deserialize)]
#[category("Capsule", category_color = "#FF6B6B")]
#[category("Movement", category_color = "#F4C542")]
#[category("Motion", category_color = "#4ECDC4")]
#[serde(default)]
pub struct CharacterControllerComponent {
    #[property(min = 0.0, max = 100.0, step = 0.01, category = "Capsule")]
    pub radius: f32,
    /// Total height including both caps.
    #[property(min = 0.0, max = 100.0, step = 0.01, category = "Capsule")]
    pub height: f32,
    /// Gap kept between the capsule and surfaces.
    #[property(min = 0.0, max = 1.0, step = 0.001, category = "Capsule")]
    pub skin_width: f32,
    /// Tallest ledge it walks up without jumping.
    #[property(min = 0.0, max = 10.0, step = 0.01, category = "Movement")]
    pub step_height: f32,
    /// Steepest slope, in degrees, it can walk up.
    #[property(min = 0.0, max = 89.0, step = 0.5, category = "Movement")]
    pub slope_limit_degrees: f32,
    /// Distance below its feet it still counts as grounded and snaps down.
    #[property(min = 0.0, max = 10.0, step = 0.01, category = "Movement")]
    pub snap_to_ground: f32,
    #[property(min = 0.0, max = 10.0, step = 0.01, category = "Movement")]
    pub gravity_scale: f32,
    /// Pushes dynamic bodies it walks into.
    #[property(category = "Movement")]
    pub push_dynamic_bodies: bool,
    /// Where the controller should move, in metres per second (horizontal
    /// walking; scripts write it every frame). Gravity and jumping act on
    /// `vertical_velocity`, not here.
    #[property(category = "Motion")]
    pub desired_velocity: [f32; 3],
    /// Vertical speed in metres per second. Gravity changes it; set it
    /// positive to jump. Zeroed while grounded and falling.
    #[property(min = -1000.0, max = 1000.0, step = 0.1, category = "Motion")]
    pub vertical_velocity: f32,
    /// Whether the controller stood on ground after its last move (written
    /// by the simulation).
    #[property(category = "Motion")]
    pub grounded: bool,
}

impl Default for CharacterControllerComponent {
    fn default() -> Self {
        Self {
            radius: 0.35,
            height: 1.8,
            skin_width: 0.02,
            step_height: 0.35,
            slope_limit_degrees: 45.0,
            snap_to_ground: 0.2,
            gravity_scale: 1.0,
            push_dynamic_bodies: true,
            desired_velocity: [0.0; 3],
            vertical_velocity: 0.0,
            grounded: false,
        }
    }
}

impl CharacterControllerComponent {
    /// Length of the straight part between the two caps (never negative).
    pub fn cylinder_height(&self) -> f32 {
        (self.height - 2.0 * self.radius).max(0.0)
    }
}

#[register_world_component]
impl CharacterControllerComponent {}

// ---------------------------------------------------------------------------
// Physics behavior
// ---------------------------------------------------------------------------

declare_events! {
    /// The controller landed on ground.
    pub const GROUNDED = (CHARACTER_CONTROLLER_CLASS_NAME, "grounded");
    /// The controller left the ground (walked off, jumped).
    pub const AIRBORNE = (CHARACTER_CONTROLLER_CLASS_NAME, "airborne");
}

impl CharacterControllerComponent {
    /// What decides the capsule; a change rebuilds the body.
    fn signature(&self) -> (u32, u32) {
        (self.radius.to_bits(), self.height.to_bits())
    }

    fn controller(&self) -> KinematicCharacterController {
        KinematicCharacterController {
            up: Vector::Y,
            offset: CharacterLength::Absolute(self.skin_width.max(0.001)),
            slide: true,
            autostep: (self.step_height > 0.0).then(|| CharacterAutostep {
                max_height: CharacterLength::Absolute(self.step_height),
                min_width: CharacterLength::Absolute((self.radius * 0.5).max(0.01)),
                include_dynamic_bodies: self.push_dynamic_bodies,
            }),
            max_slope_climb_angle: self.slope_limit_degrees.to_radians(),
            min_slope_slide_angle: self.slope_limit_degrees.to_radians(),
            snap_to_ground: (self.snap_to_ground > 0.0)
                .then(|| CharacterLength::Absolute(self.snap_to_ground)),
            ..KinematicCharacterController::default()
        }
    }
}

/// What the simulation thread remembers about one controller. `cfg` is the
/// working copy the per-run movement reads (no `World` there); every edit in
/// SceneDB replaces it.
struct Tracked {
    owner: Entity,
    cfg: CharacterControllerComponent,
    signature: (u32, u32),
    /// Vertical speed the simulation carries between runs. A component
    /// holding a different value than the last one written was set by a
    /// script (a jump) and wins.
    vertical: f32,
    vertical_token: f32,
    grounded: bool,
}

#[derive(Default)]
struct State {
    controllers: HashMap<Entity, Tracked>,
}

fn read(world: &mut World, touched: &Touched) -> Option<Part> {
    read_rows::<CharacterControllerComponent>(world, touched, |_| true)
        .map(|rows| Box::new(rows) as Part)
}

fn forget(state: &mut State, sim: &mut Sim, instance: Entity) {
    if let Some(t) = state.controllers.remove(&instance) {
        sim.collider_info
            .retain(|_, info| info.instance != instance);
        sim.remove_body(t.owner);
    }
}

fn apply(sim: &mut Sim, part: Part) {
    let Ok(rows) = part.downcast::<Rows<CharacterControllerComponent>>() else {
        return;
    };
    sim.with_state::<State, _>(|state, sim| {
        if rows.replace {
            let live: HashSet<Entity> = rows.upsert.iter().map(|r| r.instance).collect();
            let stale: Vec<Entity> = state
                .controllers
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
            let signature = row.cfg.signature();
            if let Some(t) = state.controllers.get_mut(&row.instance) {
                if t.signature == signature {
                    if row.moved {
                        if let Some(body) = sim
                            .owner_body
                            .get(&row.owner)
                            .and_then(|&h| sim.bodies.get_mut(h))
                        {
                            body.set_position(
                                pose(row.transform.position, row.transform.rotation),
                                true,
                            );
                        }
                    }
                    if row.cfg.vertical_velocity != t.vertical_token {
                        t.vertical = row.cfg.vertical_velocity;
                        t.vertical_token = row.cfg.vertical_velocity;
                    }
                    t.cfg = row.cfg;
                    continue;
                }
                forget(state, sim, row.instance);
            }
            // One body per object.
            if sim.owner_body.contains_key(&row.owner) {
                continue;
            }
            let body = RigidBodyBuilder::kinematic_position_based()
                .pose(pose(row.transform.position, row.transform.rotation))
                .build();
            let body = sim.insert_body(row.owner, body);
            let collider = ColliderBuilder::capsule_y(
                row.cfg.cylinder_height() * 0.5,
                row.cfg.radius.max(0.01),
            )
            .build();
            let handle = sim
                .colliders
                .insert_with_parent(collider, body, &mut sim.bodies);
            sim.collider_info.insert(
                handle,
                ColliderInfo {
                    instance: row.instance,
                    owner: row.owner,
                    sensor: false,
                },
            );
            state.controllers.insert(
                row.instance,
                Tracked {
                    owner: row.owner,
                    signature,
                    vertical: row.cfg.vertical_velocity,
                    vertical_token: row.cfg.vertical_velocity,
                    grounded: row.cfg.grounded,
                    cfg: row.cfg,
                },
            );
        }
    });
}

/// One controller's results.
struct MoveOut {
    owner: Entity,
    instance: Entity,
    position: [f32; 3],
    vertical: f32,
    grounded: bool,
}

/// Move every controller by what it wants this run, collide, and report.
fn after_steps(sim: &mut Sim, seconds: f32, out: &mut Output) {
    let mut results = Vec::new();
    sim.with_state::<State, _>(|state, sim| {
        for (&instance, t) in &mut state.controllers {
            let Some(&handle) = sim.owner_body.get(&t.owner) else {
                continue;
            };
            let Some(body) = sim.bodies.get(handle) else {
                continue;
            };
            let cfg = &t.cfg;
            let position = *body.position();
            if t.grounded && t.vertical < 0.0 {
                t.vertical = 0.0;
            }
            t.vertical += GRAVITY[1] * cfg.gravity_scale * seconds;
            let desired = vector([
                cfg.desired_velocity[0] * seconds,
                (cfg.desired_velocity[1] + t.vertical) * seconds,
                cfg.desired_velocity[2] * seconds,
            ]);
            let shape = Capsule::new_y(cfg.cylinder_height() * 0.5, cfg.radius.max(0.01));
            let dispatcher = DefaultQueryDispatcher;
            let queries = sim.broad_phase.as_query_pipeline(
                &dispatcher,
                &sim.bodies,
                &sim.colliders,
                QueryFilter::default().exclude_rigid_body(handle),
            );
            let movement = cfg
                .controller()
                .move_shape(seconds, &queries, &shape, &position, desired, |_| {});
            let new_position = array(position.translation + movement.translation);
            // Landing or hitting a ceiling stops vertical motion.
            if (movement.grounded && t.vertical < 0.0)
                || (!movement.grounded && movement.translation.y < desired.y - 1e-4 && t.vertical > 0.0)
            {
                t.vertical = 0.0;
            }
            let r = position.rotation;
            let rotation = euler_degrees_from_quat([r.x, r.y, r.z, r.w]);
            if let Some(body) = sim.bodies.get_mut(handle) {
                body.set_next_kinematic_position(pose(new_position, rotation));
            }

            if movement.grounded != t.grounded {
                t.grounded = movement.grounded;
                out.events.push(PhysEvent {
                    spec: if movement.grounded { GROUNDED } else { AIRBORNE },
                    owner: t.owner,
                    component: instance,
                    other: None,
                });
            }
            t.vertical_token = t.vertical;
            results.push(MoveOut {
                owner: t.owner,
                instance,
                position: new_position,
                vertical: t.vertical,
                grounded: t.grounded,
            });
        }
    });
    if !results.is_empty() {
        out.batches.push((NAME, Box::new(results)));
    }
}

/// Game thread: write the controllers' new positions and state into SceneDB.
fn write_back(world: &mut World, batch: Part, ctx: &mut WriteCtx) {
    let Ok(results) = batch.downcast::<Vec<MoveOut>>() else {
        return;
    };
    for r in *results {
        if !ctx.write_transform(world, r.owner, |t| t.position = r.position) {
            continue;
        }
        if let Some(mut c) = world.get_mut::<CharacterControllerComponent>(r.instance) {
            if c.vertical_velocity != r.vertical {
                c.vertical_velocity = r.vertical;
            }
            if c.grounded != r.grounded {
                c.grounded = r.grounded;
            }
        }
    }
}

const NAME: &str = "character_controller";

inventory::submit! {
    PhysicsBehavior {
        name: NAME,
        order: 5,
        component: component_id::<CharacterControllerComponent>,
        read,
        apply,
        settle: None,
        before_step: None,
        after_steps: Some(after_steps),
        write_back: Some(write_back),
    }
}
