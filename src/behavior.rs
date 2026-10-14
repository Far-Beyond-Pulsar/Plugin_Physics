//! How a physics component plugs into the simulation.
//!
//! Each component's file declares its own [`PhysicsBehavior`] (with
//! `inventory::submit!`) next to the component: how a changed row becomes
//! Rapier state, what the step does for it each tick, and what it writes
//! back to SceneDB. The driver in `crate::runtime` knows no component by
//! name; it only calls the registered behaviors in `order`.
//!
//! # Updates come from SceneDB's change journals
//!
//! SceneDB's sanctioned way for a system that derives state from a
//! component (a physics proxy, a spatial index) to follow it is a journal
//! cursor per component type; the engine's own voxel and script watchers
//! read the same journals. The driver keeps one per behavior plus one on
//! `ComponentOwner` and one on `Transform`, and each frame hands a behavior
//! a [`Touched`]: the instances inserted, mutated or removed since last
//! frame, the ones whose owner link changed, and the objects someone else
//! moved. Reading an empty journal is an atomic load, so a frame in which
//! nothing changed costs nothing; [`read_rows`] turns a `Touched` into a
//! [`Rows`] message of just the affected rows. The first read and any
//! journal overflow ask for a full `replace`.
//!
//! # Two sides
//!
//! * `read` runs on the game thread with the `World` and produces a typed
//!   message ([`Part`]) the simulation thread owns afterwards.
//! * `apply`, `settle`, `before_step` and `after_steps` run on the
//!   simulation thread with the [`Sim`]; they never see the `World`. What
//!   they want written back they push to [`Output`] as one batch per run,
//!   and `write_back` applies it on the game thread.

use std::any::Any;
use std::collections::{HashMap, HashSet};

use pulsar_scenedb::{component_id, Component, ComponentId, Entity, World};
use pulsar_world_registry::pulsar_scene_model::{attachments, ComponentOwner, Transform};

use crate::physics_events::EventSpec;
use crate::sim::Sim;

/// A message passed between the two sides of a behavior.
pub type Part = Box<dyn Any + Send>;

/// What the behaviors produced during a run.
#[derive(Default)]
pub struct Output {
    /// At most one batch per behavior, for its `write_back`.
    pub batches: Vec<(&'static str, Part)>,
    pub events: Vec<PhysEvent>,
}

/// One physics event: `owner` is the object whose component raised it,
/// `component` that component's entity, `other` the other party if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysEvent {
    pub spec: EventSpec,
    pub owner: Entity,
    pub component: Entity,
    pub other: Option<Entity>,
}

/// Registered next to a component with `inventory::submit!`.
pub struct PhysicsBehavior {
    pub name: &'static str,
    /// Lower runs first: bodies before the colliders that attach to them,
    /// colliders before the joints that connect bodies.
    pub order: i32,
    pub component: fn() -> ComponentId,
    /// Game thread: turn what changed into a message, or `None`.
    pub read: fn(&mut World, &Touched) -> Option<Part>,
    /// Simulation thread: apply a message made by `read`.
    pub apply: fn(&mut Sim, Part),
    /// Simulation thread, once after all messages were applied. `sim.dirty`
    /// names the objects whose body appeared or disappeared this run.
    pub settle: Option<fn(&mut Sim)>,
    /// Simulation thread, before every fixed step.
    pub before_step: Option<fn(&mut Sim, &mut Output)>,
    /// Simulation thread, after the steps of a run; the seconds simulated.
    pub after_steps: Option<fn(&mut Sim, f32, &mut Output)>,
    /// Game thread: apply a batch this behavior pushed to [`Output`].
    pub write_back: Option<fn(&mut World, Part, &mut WriteCtx)>,
}

inventory::collect!(PhysicsBehavior);

/// The registered behaviors, in `order`.
pub fn behaviors() -> &'static [&'static PhysicsBehavior] {
    static ALL: std::sync::LazyLock<Vec<&'static PhysicsBehavior>> =
        std::sync::LazyLock::new(|| {
            let mut all: Vec<_> = inventory::iter::<PhysicsBehavior>.into_iter().collect();
            all.sort_by_key(|b| (b.order, b.name));
            all
        });
    &ALL
}

/// What a `write_back` may touch: results for an object that someone else
/// moved since the run began are stale and must be skipped.
pub struct WriteCtx<'a> {
    pub(crate) stale: &'a dyn Fn(Entity) -> bool,
    pub(crate) written: &'a mut HashMap<Entity, Transform>,
}

impl WriteCtx<'_> {
    /// Edit `owner`'s `Transform` unless the result is stale. Returns
    /// whether it was written. The new value is remembered so it is not
    /// mistaken for someone else's move.
    pub fn write_transform(
        &mut self,
        world: &mut World,
        owner: Entity,
        edit: impl FnOnce(&mut Transform),
    ) -> bool {
        if (self.stale)(owner) {
            return false;
        }
        let Some(mut transform) = world.get_mut::<Transform>(owner) else {
            return false;
        };
        edit(&mut transform);
        self.written.insert(owner, *transform);
        true
    }
}

/// What changed in SceneDB since the previous frame, for one behavior.
pub struct Touched<'a> {
    /// First read, or a journal overflowed: everything must be re-sent.
    pub rescan: bool,
    /// Instances of the behavior's component inserted or mutated.
    pub changed: Vec<Entity>,
    /// Instances removed (or despawned).
    pub removed: Vec<Entity>,
    /// Entities whose `ComponentOwner` changed (enabled, disabled, moved).
    pub relinked: &'a [Entity],
    /// Objects whose `Transform` something other than physics changed.
    pub moved: &'a HashSet<Entity>,
}

impl Touched<'_> {
    pub fn is_empty(&self) -> bool {
        !self.rescan
            && self.changed.is_empty()
            && self.removed.is_empty()
            && self.relinked.is_empty()
            && self.moved.is_empty()
    }
}

/// A component row with the object it belongs to and that object's
/// `Transform` when it was read.
#[derive(Clone)]
pub struct Row<C> {
    pub instance: Entity,
    pub owner: Entity,
    pub transform: Transform,
    /// Something other than physics changed the object's `Transform` (or
    /// this is the first read): put the simulated body there. Otherwise
    /// `transform` may be a frame behind the simulation and must not be used
    /// to move anything that already exists.
    pub moved: bool,
    pub cfg: C,
}

/// The affected rows of one component type.
pub struct Rows<C> {
    /// The upserts are the whole truth: forget instances not listed.
    pub replace: bool,
    pub upsert: Vec<Row<C>>,
    pub remove: Vec<Entity>,
}

fn row_of<C: Component + Clone>(
    world: &World,
    touched: &Touched,
    instance: Entity,
    keep: &impl Fn(&C) -> bool,
) -> Option<Row<C>> {
    let cfg = world.get::<C>(instance)?;
    let link = world.get::<ComponentOwner>(instance)?;
    let owner = link.entity();
    (link.is_enabled() && keep(cfg)).then(|| Row {
        instance,
        owner,
        transform: world.get::<Transform>(owner).copied().unwrap_or_default(),
        moved: touched.rescan || touched.moved.contains(&owner),
        cfg: cfg.clone(),
    })
}

/// The rows of component `C` that `touched` affects. `keep` filters out
/// instances that should not be simulated (a disabled component).
///
/// Cost follows the changes: journal entries, plus the instances of each
/// moved object. Only a rescan walks every row.
pub fn read_rows<C: Component + Clone>(
    world: &mut World,
    touched: &Touched,
    keep: impl Fn(&C) -> bool,
) -> Option<Rows<C>> {
    if touched.is_empty() {
        return None;
    }
    let id = component_id::<C>();
    let mut candidates: HashSet<Entity> = touched.changed.iter().copied().collect();
    candidates.extend(
        touched
            .relinked
            .iter()
            .copied()
            .filter(|e| world.has_component(*e, id)),
    );
    for &owner in touched.moved {
        candidates.extend(
            attachments::instances(world, owner)
                .into_iter()
                .filter(|e| world.has_component(*e, id)),
        );
    }
    if touched.rescan {
        candidates.extend(world.query::<&C>().map(|(instance, _)| instance));
    }

    let mut upsert = Vec::with_capacity(candidates.len());
    let mut remove = touched.removed.clone();
    for instance in candidates {
        match row_of(world, touched, instance, &keep) {
            Some(row) => upsert.push(row),
            // Disabled, filtered out or gone: the sim must forget it.
            None if !touched.rescan => remove.push(instance),
            None => {}
        }
    }
    upsert.sort_by_key(|row| row.instance.bits());
    Some(Rows {
        replace: touched.rescan,
        upsert,
        remove,
    })
}
