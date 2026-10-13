//! A simulated body: what moves an object under the physics step.
//!
//! The component is the body's authoritative description. Its behavior,
//! below, keeps the Rapier body in step with it: an edit to the component
//! (or a move of its object) reaches the body on the next simulation run,
//! and the body's results are written back into the object's `Transform`
//! and this component's velocities.

use std::collections::HashMap;

use engine_class_derive::{engine_class, register_world_component};
use pulsar_scenedb::{component_id, Entity, World};
use pulsar_reflection::Reflectable;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::behavior::{read_rows, Output, Part, PhysEvent, PhysicsBehavior, Rows, Touched, WriteCtx};
use crate::declare_events;
use crate::math::{array, euler_degrees_from_quat, near, pose, vector};
use crate::sim::Sim;

/// The class name scripts and level files use.
pub const RIGID_BODY_CLASS_NAME: &str = "RigidBodyComponent";

/// How a body takes part in the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflectable)]
pub enum BodyType {
    /// Moved by forces, gravity and collisions.
    Dynamic,
    /// Follows its object's `Transform`; pushes dynamic bodies but is never
    /// pushed back.
    Kinematic,
    /// Never moves; collides with everything else.
    Static,
}

impl Default for BodyType {
    fn default() -> Self {
        Self::Dynamic
    }
}

#[engine_class(category = "Physics", clone, debug, serialize, deserialize)]
#[category("Body", category_color = "#FF6B6B")]
#[category("Damping", category_color = "#4ECDC4")]
#[category("Constraints", category_color = "#A78BFA", default_collapsed = true)]
#[category("Motion", category_color = "#F4C542")]
#[serde(default)]
pub struct RigidBodyComponent {
    #[property(category = "Body")]
    pub enabled: bool,
    #[property(category = "Body")]
    pub body_type: BodyType,
    /// Total mass in kilograms. Zero: derived from the colliders' density.
    #[property(min = 0.0, max = 1000000.0, step = 0.1, category = "Body")]
    pub mass: f32,
    #[property(min = -100.0, max = 100.0, step = 0.1, category = "Body")]
    pub gravity_scale: f32,
    /// Allows the body to rest when it stops moving.
    #[property(category = "Body")]
    pub can_sleep: bool,
    /// Sweeps fast movement so the body cannot tunnel through thin colliders.
    #[property(category = "Body")]
    pub continuous_collision: bool,
    #[property(min = 0.0, max = 1000.0, step = 0.01, category = "Damping")]
    pub linear_damping: f32,
    #[property(min = 0.0, max = 1000.0, step = 0.01, category = "Damping")]
    pub angular_damping: f32,
    #[property(category = "Constraints")]
    pub lock_position_x: bool,
    #[property(category = "Constraints")]
    pub lock_position_y: bool,
    #[property(category = "Constraints")]
    pub lock_position_z: bool,
    #[property(category = "Constraints")]
    pub lock_rotation_x: bool,
    #[property(category = "Constraints")]
    pub lock_rotation_y: bool,
    #[property(category = "Constraints")]
    pub lock_rotation_z: bool,
    /// Metres per second. Authored as the starting velocity; the simulation
    /// keeps it current while the body moves, and writing it changes the
    /// body's velocity.
    #[property(category = "Motion")]
    pub linear_velocity: [f32; 3],
    /// Radians per second about each axis; same contract as `linear_velocity`.
    #[property(category = "Motion")]
    pub angular_velocity: [f32; 3],
}

impl Default for RigidBodyComponent {
    fn default() -> Self {
        Self {
            enabled: true,
            body_type: BodyType::Dynamic,
            mass: 0.0,
            gravity_scale: 1.0,
            can_sleep: true,
            continuous_collision: false,
            linear_damping: 0.0,
            angular_damping: 0.05,
            lock_position_x: false,
            lock_position_y: false,
            lock_position_z: false,
            lock_rotation_x: false,
            lock_rotation_y: false,
            lock_rotation_z: false,
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        }
    }
}

#[register_world_component]
impl RigidBodyComponent {}

// ---------------------------------------------------------------------------
// Physics behavior
// ---------------------------------------------------------------------------

declare_events! {
    /// The body came to rest.
    pub const SLEEP = (RIGID_BODY_CLASS_NAME, "sleep");
    /// The body started moving again.
    pub const WAKE = (RIGID_BODY_CLASS_NAME, "wake");
}

fn rapier_type(kind: BodyType) -> RigidBodyType {
    match kind {
        BodyType::Dynamic => RigidBodyType::Dynamic,
        BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
        BodyType::Static => RigidBodyType::Fixed,
    }
}

impl RigidBodyComponent {
    /// Apply every setting that can change while the body lives.
    fn configure(&self, body: &mut RigidBody) {
        body.set_body_type(rapier_type(self.body_type), true);
        body.set_linear_damping(self.linear_damping.max(0.0));
        body.set_angular_damping(self.angular_damping.max(0.0));
        body.set_gravity_scale(self.gravity_scale, false);
        body.enable_ccd(self.continuous_collision);
        body.set_enabled_translations(
            !self.lock_position_x,
            !self.lock_position_y,
            !self.lock_position_z,
            false,
        );
        body.set_enabled_rotations(
            !self.lock_rotation_x,
            !self.lock_rotation_y,
            !self.lock_rotation_z,
            false,
        );
        if self.mass > 0.0 {
            body.set_additional_mass(self.mass, false);
        }
        if !self.can_sleep {
            body.activation_mut().normalized_linear_threshold = -1.0;
            body.activation_mut().angular_threshold = -1.0;
        }
    }
}

/// What the simulation thread remembers about one body component.
struct Tracked {
    owner: Entity,
    /// The velocities the last step produced; a component holding other
    /// ones was written by someone else.
    velocity_token: ([f32; 3], [f32; 3]),
    asleep: bool,
}

/// Simulation-side state of this behavior, by component instance.
#[derive(Default)]
struct State {
    bodies: HashMap<Entity, Tracked>,
}

fn read(world: &mut World, touched: &Touched) -> Option<Part> {
    read_rows::<RigidBodyComponent>(world, touched, |c| c.enabled)
        .map(|rows| Box::new(rows) as Part)
}

fn apply(sim: &mut Sim, part: Part) {
    let Ok(rows) = part.downcast::<Rows<RigidBodyComponent>>() else {
        return;
    };
    sim.with_state::<State, _>(|state, sim| {
        if rows.replace {
            let live: std::collections::HashSet<Entity> =
                rows.upsert.iter().map(|r| r.instance).collect();
            let stale: Vec<Entity> = state
                .bodies
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
            let target = pose(row.transform.position, row.transform.rotation);
            let cfg = &row.cfg;
            if let Some(tracked) = state.bodies.get_mut(&row.instance) {
                let handle = sim.owner_body.get(&tracked.owner).copied();
                let Some(body) = handle.and_then(|h| sim.bodies.get_mut(h)) else {
                    state.bodies.remove(&row.instance);
                    continue;
                };
                cfg.configure(body);
                match cfg.body_type {
                    BodyType::Kinematic => body.set_next_kinematic_position(target),
                    _ if row.moved => {
                        body.set_position(target, true);
                        body.set_linvel(vector(cfg.linear_velocity), true);
                        body.set_angvel(vector(cfg.angular_velocity), true);
                    }
                    _ => {
                        // A velocity that is not the one the last step
                        // produced was written by a script or the inspector.
                        if !near(cfg.linear_velocity, tracked.velocity_token.0) {
                            body.set_linvel(vector(cfg.linear_velocity), true);
                        }
                        if !near(cfg.angular_velocity, tracked.velocity_token.1) {
                            body.set_angvel(vector(cfg.angular_velocity), true);
                        }
                    }
                }
                tracked.velocity_token = (cfg.linear_velocity, cfg.angular_velocity);
                continue;
            }
            // One body per object: the first component (or a character
            // controller) wins.
            if sim.owner_body.contains_key(&row.owner) {
                continue;
            }
            let mut body = RigidBodyBuilder::new(rapier_type(cfg.body_type))
                .pose(target)
                .linvel(vector(cfg.linear_velocity))
                .angvel(vector(cfg.angular_velocity))
                .build();
            cfg.configure(&mut body);
            sim.insert_body(row.owner, body);
            state.bodies.insert(
                row.instance,
                Tracked {
                    owner: row.owner,
                    velocity_token: (cfg.linear_velocity, cfg.angular_velocity),
                    asleep: false,
                },
            );
        }
    });
}

fn forget(state: &mut State, sim: &mut Sim, instance: Entity) {
    if let Some(tracked) = state.bodies.remove(&instance) {
        sim.remove_body(tracked.owner);
    }
}

/// One dynamic body's results.
struct BodyOut {
    owner: Entity,
    instance: Entity,
    position: [f32; 3],
    rotation: [f32; 3],
    linear: [f32; 3],
    angular: [f32; 3],
}

fn after_steps(sim: &mut Sim, _seconds: f32, out: &mut Output) {
    let mut results = Vec::new();
    sim.with_state::<State, _>(|state, sim| {
        for (&instance, tracked) in &mut state.bodies {
            let Some(body) = sim
                .owner_body
                .get(&tracked.owner)
                .and_then(|&h| sim.bodies.get(h))
            else {
                continue;
            };
            let owner = tracked.owner;
            let asleep = body.is_sleeping();
            if asleep != tracked.asleep {
                tracked.asleep = asleep;
                out.events.push(PhysEvent {
                    spec: if asleep { SLEEP } else { WAKE },
                    owner,
                    component: instance,
                    other: None,
                });
            }
            if body.body_type() != RigidBodyType::Dynamic || asleep {
                continue;
            }
            let r = body.rotation();
            let linear = array(body.linvel());
            let angular = array(body.angvel());
            tracked.velocity_token = (linear, angular);
            results.push(BodyOut {
                owner,
                instance,
                position: array(body.translation()),
                rotation: euler_degrees_from_quat([r.x, r.y, r.z, r.w]),
                linear,
                angular,
            });
        }
    });
    if !results.is_empty() {
        out.batches.push((NAME, Box::new(results)));
    }
}

/// Game thread: write the bodies' new poses and velocities into SceneDB.
fn write_back(world: &mut World, batch: Part, ctx: &mut WriteCtx) {
    let Ok(results) = batch.downcast::<Vec<BodyOut>>() else {
        return;
    };
    for body in *results {
        let written = ctx.write_transform(world, body.owner, |t| {
            t.position = body.position;
            t.rotation = body.rotation;
        });
        if !written {
            continue;
        }
        if let Some(mut component) = world.get_mut::<RigidBodyComponent>(body.instance) {
            if component.linear_velocity != body.linear {
                component.linear_velocity = body.linear;
            }
            if component.angular_velocity != body.angular {
                component.angular_velocity = body.angular;
            }
        }
    }
}

const NAME: &str = "rigid_body";

inventory::submit! {
    PhysicsBehavior {
        name: NAME,
        order: 0,
        component: component_id::<RigidBodyComponent>,
        read,
        apply,
        settle: None,
        before_step: None,
        after_steps: Some(after_steps),
        write_back: Some(write_back),
    }
}
