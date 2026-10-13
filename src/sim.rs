//! The Rapier world shared by the physics behaviors.
//!
//! This is only the container: Rapier's sets and pipeline, the maps that let
//! behaviors find each other's handles (an owner's body, a collider's
//! owner), a fixed-step loop, and a typed scratch space per behavior. What a
//! component *means* to the simulation lives in that component's file; see
//! [`crate::behavior`].
//!
//! SceneDB stays the source of truth. A [`Sim`] is rebuilt from it by one
//! full `replace` read, so dropping it loses nothing; what behaviors keep
//! here is Rapier handles, change-detection tokens, and the working copy of
//! rows that must be stepped every tick without the `World` (force fields).
//! Every SceneDB edit replaces the working copy.
//!
//! `Sim` knows nothing about SceneDB or threads; `crate::runtime` runs it on
//! its own thread.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver};

use pulsar_scenedb::Entity;
use rapier3d::pipeline::ChannelEventCollector;
use rapier3d::prelude::*;

use crate::behavior::{behaviors, Output, Part};
use crate::math::vector;

/// Fixed simulation rate. Frames are accumulated into whole steps so the
/// result does not depend on the frame rate.
pub const STEP_SECONDS: f32 = 1.0 / 60.0;
/// Most steps run for one frame's worth of time; beyond it time is dropped,
/// not owed.
const MAX_STEPS_PER_RUN: u32 = 5;
pub const GRAVITY: [f32; 3] = [0.0, -9.81, 0.0];

/// What a collider handle means to event translation.
#[derive(Clone, Copy)]
pub struct ColliderInfo {
    pub instance: Entity,
    pub owner: Entity,
    pub sensor: bool,
}

/// One session's Rapier world.
pub struct Sim {
    pipeline: PhysicsPipeline,
    pub islands: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    pub impulse_joints: ImpulseJointSet,
    pub multibody_joints: MultibodyJointSet,
    ccd: CCDSolver,
    params: IntegrationParameters,
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    /// The simulated body of each object (rigid body or character).
    pub owner_body: HashMap<Entity, RigidBodyHandle>,
    pub collider_info: HashMap<ColliderHandle, ColliderInfo>,
    /// A fixed body joints attach to when they connect to "the world".
    pub anchor: RigidBodyHandle,
    /// Objects whose body appeared or disappeared during this run; cleared
    /// after every behavior's `settle` has seen it.
    pub dirty: HashSet<Entity>,
    states: HashMap<TypeId, Box<dyn Any + Send>>,
    accumulator: f32,
    collisions: Receiver<CollisionEvent>,
    collector: ChannelEventCollector,
}

impl Default for Sim {
    fn default() -> Self {
        let mut params = IntegrationParameters::default();
        params.dt = STEP_SECONDS;
        let (collision_tx, collisions) = channel();
        let (force_tx, _forces) = channel();
        let mut bodies = RigidBodySet::new();
        let anchor = bodies.insert(RigidBodyBuilder::fixed().build());
        Self {
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
            params,
            bodies,
            colliders: ColliderSet::new(),
            owner_body: HashMap::new(),
            collider_info: HashMap::new(),
            anchor,
            dirty: HashSet::new(),
            states: HashMap::new(),
            accumulator: 0.0,
            collisions,
            collector: ChannelEventCollector::new(collision_tx, force_tx),
        }
    }
}

impl Sim {
    /// `(bodies, colliders, joints)` currently simulated.
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.owner_body.len(),
            self.collider_info.len(),
            self.impulse_joints.len(),
        )
    }

    /// Run `f` with behavior state `T` (created on first use) and the sim.
    /// The state is moved out for the call, so `f` can use both freely.
    pub fn with_state<T: Default + Send + 'static, R>(
        &mut self,
        f: impl FnOnce(&mut T, &mut Sim) -> R,
    ) -> R {
        let mut state: Box<T> = self
            .states
            .remove(&TypeId::of::<T>())
            .and_then(|s| s.downcast().ok())
            .unwrap_or_default();
        let result = f(&mut state, self);
        self.states.insert(TypeId::of::<T>(), state);
        result
    }

    /// Note that `owner`'s body appeared or disappeared; behaviors that
    /// depend on it reconnect in `settle`.
    pub fn mark_dirty(&mut self, owner: Entity) {
        self.dirty.insert(owner);
    }

    /// Insert a body for `owner` and remember it.
    pub fn insert_body(&mut self, owner: Entity, body: RigidBody) -> RigidBodyHandle {
        let handle = self.bodies.insert(body);
        self.owner_body.insert(owner, handle);
        self.mark_dirty(owner);
        handle
    }

    /// Remove `owner`'s body. Colliders attached to it stay in the world,
    /// detached; their behavior re-attaches or removes them in `settle`.
    pub fn remove_body(&mut self, owner: Entity) {
        if let Some(handle) = self.owner_body.remove(&owner) {
            self.bodies.remove(
                handle,
                &mut self.islands,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                false,
            );
            self.mark_dirty(owner);
        }
    }

    /// Drain the collision events the last steps produced.
    pub fn take_collisions(&mut self) -> Vec<CollisionEvent> {
        self.collisions.try_iter().collect()
    }

    /// Apply the messages (`(behavior index, part)`), step for `dt` seconds
    /// and let every behavior report.
    pub fn run(&mut self, mut parts: Vec<(usize, Part)>, dt: f32) -> Output {
        let behaviors = behaviors();
        let mut out = Output::default();
        {
            profiling::profile_scope!("Physics::Reconcile");
            parts.sort_by_key(|(index, _)| *index);
            for (index, part) in parts {
                (behaviors[index].apply)(self, part);
            }
            for behavior in behaviors {
                if let Some(settle) = behavior.settle {
                    settle(self);
                }
            }
            self.dirty.clear();
        }

        self.accumulator += dt.clamp(0.0, 0.25);
        let mut steps = 0;
        while self.accumulator >= STEP_SECONDS && steps < MAX_STEPS_PER_RUN {
            self.accumulator -= STEP_SECONDS;
            steps += 1;
            profiling::profile_scope!("Physics::Step");
            for behavior in behaviors {
                if let Some(before) = behavior.before_step {
                    before(self, &mut out);
                }
            }
            self.pipeline.step(
                vector(GRAVITY),
                &self.params,
                &mut self.islands,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.bodies,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                &mut self.ccd,
                &(),
                &self.collector,
            );
        }
        if steps == MAX_STEPS_PER_RUN {
            self.accumulator = 0.0;
        }
        if steps > 0 {
            profiling::profile_scope!("Physics::Collect");
            let seconds = steps as f32 * STEP_SECONDS;
            for behavior in behaviors {
                if let Some(after) = behavior.after_steps {
                    after(self, seconds, &mut out);
                }
            }
        }
        out
    }
}
