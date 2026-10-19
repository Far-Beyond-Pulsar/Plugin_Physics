//! Drives the physics behaviors: SceneDB's change journals in, a simulation
//! on its own thread, results and events back out.
//!
//! This file knows no component. Each component's file registers a
//! [`PhysicsBehavior`] (see [`crate::behavior`]) saying what its changes
//! mean to Rapier; the driver only moves data between SceneDB and the
//! behaviors.
//!
//! The component runtime ticks one hand-written [`ComponentTickRegistration`]
//! per frame with the whole `World`. That is the hook plugins have for a
//! world-wide pass; per-instance callbacks (`#[register_component_runtime]`)
//! cannot see the other rows that colliders, joints and force fields relate
//! to.
//!
//! # Per frame, on the game thread (`tick`)
//!
//! 1. **Moves**: the `Transform` journal says which objects changed; the
//!    ones whose new value is not what physics last wrote were moved by
//!    someone else (the editor, a script, a level load).
//! 2. **Apply**: what the simulation finished since last frame is written
//!    into SceneDB and its events are published in the `physics` namespace.
//!    A result is dropped for an object that was moved since the run it
//!    comes from began; the move is already on its way to the simulation.
//! 3. **Read**: every behavior's component journal (plus `ComponentOwner`
//!    and moves) says what to send. A frame in which nothing changed sends
//!    only the elapsed time.
//!
//! So an edited component reaches its Rapier counterpart on the next run,
//! and the simulation's results come back a frame later. Nothing on the game
//! thread waits for the simulation.
//!
//! # The simulation thread
//!
//! One thread per `World`, named `Pulsar-Physics`. Its scopes (`Physics::*`)
//! are recorded in the editor's profiler: `export_plugin!` shares the host
//! profiler with this library when it loads.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pulsar_events::EventHub;
use pulsar_scenedb::{
    component_id, ChangeCursor, ChangeRead, ComponentChange, ComponentChangeKind, Entity, World,
};
use pulsar_world_registry::pulsar_scene_model::{ComponentOwner, Transform};
use pulsar_world_registry::{inventory, ComponentRuntimeState, ComponentTickRegistration};

use crate::behavior::{behaviors, Output, Part, PhysEvent, Touched, WriteCtx};
use crate::components::{RigidBodyComponent, RIGID_BODY_CLASS_NAME};
use crate::physics_events::{self, EventIds};
use crate::sim::Sim;

/// A session nobody ticked for this long is shut down.
const IDLE_SESSION: Duration = Duration::from_secs(10);
/// Events kept for a frame the game thread has not collected yet.
const MAX_PENDING_EVENTS: usize = 10_000;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct Inbox {
    parts: Vec<(usize, Part)>,
    dt: f32,
    shutdown: bool,
}

#[derive(Default)]
struct Outbox {
    /// Batches in run order, with the run each came from.
    batches: Vec<(u64, &'static str, Part)>,
    events: Vec<PhysEvent>,
}

#[derive(Default)]
struct Shared {
    inbox: Mutex<Inbox>,
    wake: Condvar,
    outbox: Mutex<Outbox>,
    /// (bodies, colliders, joints) after the last run.
    counts: Mutex<(usize, usize, usize)>,
    submitted: AtomicU64,
    processed: Mutex<u64>,
    settled: Condvar,
}

impl Shared {
    /// Queue this frame's messages; returns the submission's sequence number.
    fn submit(&self, parts: Vec<(usize, Part)>, dt: f32) -> u64 {
        let seq = {
            let mut inbox = lock(&self.inbox);
            inbox.parts.extend(parts);
            inbox.dt += dt;
            self.submitted.fetch_add(1, Ordering::AcqRel) + 1
        };
        self.wake.notify_one();
        seq
    }
}

/// The game thread's view of SceneDB's journals for one session.
#[derive(Default)]
struct Streams {
    opened: bool,
    /// One cursor per behavior, in `behaviors()` order.
    components: Vec<ChangeCursor>,
    links: Option<ChangeCursor>,
    transforms: Option<ChangeCursor>,
    /// The `Transform` physics last wrote per object.
    self_writes: HashMap<Entity, Transform>,
    /// Writes from runs before this submission are stale for the object.
    fences: HashMap<Entity, u64>,
    scratch: Vec<ComponentChange>,
}

/// One `World`'s simulation thread and its mailboxes.
struct Session {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    last_tick: Instant,
    streams: Streams,
    /// The hub the physics events are registered on, and their ids there.
    events: Option<(EventHub, EventIds)>,
}

impl Session {
    fn spawn() -> Self {
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("Pulsar-Physics".into())
            .spawn(move || simulation_thread(worker))
            .map_err(|error| tracing::error!("could not start the physics thread: {error}"))
            .ok();
        Self {
            shared,
            thread,
            last_tick: Instant::now(),
            streams: Streams::default(),
            events: None,
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        lock(&self.shared.inbox).shutdown = true;
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn simulation_thread(shared: Arc<Shared>) {
    profiling::set_thread_name("Pulsar-Physics");
    let mut sim = Sim::default();
    loop {
        let (parts, dt, handled) = {
            let mut inbox = lock(&shared.inbox);
            while inbox.parts.is_empty() && inbox.dt == 0.0 && !inbox.shutdown {
                inbox = shared
                    .wake
                    .wait(inbox)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if inbox.shutdown {
                return;
            }
            // Submissions are counted under the inbox lock, so this covers
            // exactly what is taken here.
            let handled = shared.submitted.load(Ordering::Acquire);
            (
                std::mem::take(&mut inbox.parts),
                std::mem::take(&mut inbox.dt),
                handled,
            )
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            profiling::profile_scope!("Physics::Frame");
            sim.run(parts, dt)
        }));
        match result {
            Ok(output) => {
                profiling::profile_scope!("Physics::Publish");
                publish(&shared, output, handled);
                *lock(&shared.counts) = sim.counts();
            }
            Err(_) => {
                tracing::error!(
                    "the physics simulation panicked; it restarts from the scene on the next change"
                );
                sim = Sim::default();
            }
        }
        *lock(&shared.processed) = handled;
        shared.settled.notify_all();
    }
}

fn publish(shared: &Shared, output: Output, seq: u64) {
    let mut outbox = lock(&shared.outbox);
    outbox.batches.extend(
        output
            .batches
            .into_iter()
            .map(|(name, batch)| (seq, name, batch)),
    );
    outbox.events.extend(output.events);
    let excess = outbox.events.len().saturating_sub(MAX_PENDING_EVENTS);
    outbox.events.drain(..excess);
}

/// One simulation per `World`, keyed by the world's address: tick
/// registrations are plain function pointers and carry no session state. A
/// reused address starts from a stale session; its first read is a full
/// rescan that replaces whatever the old one knew.
static SESSIONS: LazyLock<Mutex<HashMap<usize, Session>>> = LazyLock::new(Mutex::default);

fn key_of(world: &World) -> usize {
    world as *const World as usize
}

// ---------------------------------------------------------------------------
// The component-runtime registration
// ---------------------------------------------------------------------------

fn tick(
    world: &mut World,
    events: &EventHub,
    delta_seconds: f32,
    _previous: &HashMap<Entity, Entity>,
    _current: &mut HashMap<Entity, Entity>,
    _runtime: &mut ComponentRuntimeState,
) {
    profiling::profile_scope!("Physics::Tick");
    let key = key_of(world);
    let mut sessions = lock(&SESSIONS);
    let now = Instant::now();
    sessions.retain(|&other, session| other == key || now - session.last_tick < IDLE_SESSION);
    let session = sessions.entry(key).or_insert_with(Session::spawn);
    session.last_tick = now;
    if !session
        .events
        .as_ref()
        .is_some_and(|(hub, _)| hub.ptr_eq(events))
    {
        session.events = Some((events.clone(), physics_events::register_all(events)));
    }

    let Session {
        shared,
        streams,
        events: hub,
        ..
    } = session;
    let ids = hub.as_ref().map(|(_, ids)| ids);
    let (moved, rescan_all) = {
        profiling::profile_scope!("Physics::Moves");
        read_moves(world, streams)
    };
    {
        profiling::profile_scope!("Physics::Apply");
        let finished = std::mem::take(&mut *lock(&shared.outbox));
        if let Some(ids) = ids {
            apply(world, events, ids, finished, streams, &moved);
        }
    }
    let parts = {
        profiling::profile_scope!("Physics::Read");
        read_parts(world, streams, &moved, rescan_all)
    };
    let seq = shared.submit(parts, delta_seconds);
    for owner in &moved {
        streams.fences.insert(*owner, seq);
    }
}

inventory::submit! {
    ComponentTickRegistration {
        type_name: "physics_plugin::driver",
        class_name: RIGID_BODY_CLASS_NAME,
        handler_events: &[],
        component_type: component_id::<RigidBodyComponent>,
        tick,
        // Removal is seen through the journals, not through lifecycle sets.
        end_play: None,
    }
}

// ---------------------------------------------------------------------------
// SceneDB <-> messages
// ---------------------------------------------------------------------------

/// Open the journals on first use and return the objects someone other than
/// physics moved this frame, and whether everything must be re-read.
fn read_moves(world: &World, streams: &mut Streams) -> (HashSet<Entity>, bool) {
    let mut rescan_all = false;
    if !streams.opened {
        streams.opened = true;
        rescan_all = true;
        streams.components = behaviors()
            .iter()
            .map(|b| world.open_change_cursor_id((b.component)()))
            .collect();
        streams.links = Some(world.open_change_cursor::<ComponentOwner>());
        streams.transforms = Some(world.open_change_cursor::<Transform>());
    }
    let mut moved = HashSet::new();
    let Some(cursor) = streams.transforms.as_mut() else {
        return (moved, rescan_all);
    };
    streams.scratch.clear();
    match world.read_changes(cursor, &mut streams.scratch) {
        ChangeRead::Overflowed => rescan_all = true,
        ChangeRead::Complete => {
            for change in &streams.scratch {
                if change.kind == ComponentChangeKind::Removed {
                    continue;
                }
                let entity = change.entity;
                let now = world.get::<Transform>(entity).copied();
                if now.is_some() && streams.self_writes.get(&entity) != now.as_ref() {
                    moved.insert(entity);
                }
            }
        }
    }
    (moved, rescan_all)
}

fn apply(
    world: &mut World,
    events: &EventHub,
    ids: &EventIds,
    finished: Outbox,
    streams: &mut Streams,
    moved: &HashSet<Entity>,
) {
    let mut applied_through = 0;
    for (seq, name, batch) in finished.batches {
        applied_through = applied_through.max(seq);
        let Some(write_back) = behaviors()
            .iter()
            .find(|b| b.name == name)
            .and_then(|b| b.write_back)
        else {
            continue;
        };
        // Moved by someone else since the run this batch comes from began.
        let fences = &streams.fences;
        let stale = |owner: Entity| {
            moved.contains(&owner) || fences.get(&owner).is_some_and(|&fence| seq < fence)
        };
        let mut ctx = WriteCtx {
            stale: &stale,
            written: &mut streams.self_writes,
        };
        write_back(world, batch, &mut ctx);
    }
    // Batches arrive in run order: a fence the applied runs have reached
    // can no longer make anything stale.
    streams.fences.retain(|_, fence| *fence > applied_through);
    if streams.self_writes.len() > 100_000 {
        streams.self_writes.retain(|e, _| world.is_alive(*e));
    }
    for event in &finished.events {
        physics_events::publish(events, ids, event);
    }
}

fn read_parts(
    world: &mut World,
    streams: &mut Streams,
    moved: &HashSet<Entity>,
    rescan_all: bool,
) -> Vec<(usize, Part)> {
    // Instances whose owner link changed, for every behavior.
    let mut links = Vec::new();
    let mut rescan = rescan_all;
    if let Some(cursor) = streams.links.as_mut() {
        streams.scratch.clear();
        match world.read_changes(cursor, &mut streams.scratch) {
            ChangeRead::Overflowed => rescan = true,
            ChangeRead::Complete => links.extend(streams.scratch.iter().map(|c| c.entity)),
        }
    }

    let mut parts = Vec::new();
    for (index, behavior) in behaviors().iter().enumerate() {
        let mut touched = Touched {
            rescan,
            changed: Vec::new(),
            removed: Vec::new(),
            relinked: &links,
            moved,
        };
        streams.scratch.clear();
        match world.read_changes(&mut streams.components[index], &mut streams.scratch) {
            ChangeRead::Overflowed => touched.rescan = true,
            ChangeRead::Complete => {
                for change in &streams.scratch {
                    match change.kind {
                        ComponentChangeKind::Removed => touched.removed.push(change.entity),
                        _ => touched.changed.push(change.entity),
                    }
                }
            }
        }
        if let Some(part) = (behavior.read)(world, &touched) {
            parts.push((index, part));
        }
    }
    parts
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Number of bodies and colliders simulated for `world` after the last
/// completed run (for tests and diagnostics).
pub fn simulated_counts(world: &World) -> (usize, usize) {
    lock(&SESSIONS)
        .get(&key_of(world))
        .map_or((0, 0), |session| {
            let counts = *lock(&session.shared.counts);
            (counts.0, counts.1)
        })
}

/// Block until the simulation thread of `world` has handled everything
/// submitted so far. Headless tools and tests use it to make the
/// one-frame-late results deterministic; the game never calls it.
pub fn wait_idle(world: &World) {
    // The session lock is not held while waiting, so ticks of other worlds
    // are not blocked.
    let shared = lock(&SESSIONS)
        .get(&key_of(world))
        .map(|session| Arc::clone(&session.shared));
    if let Some(shared) = shared {
        let target = shared.submitted.load(Ordering::Acquire);
        let mut processed = lock(&shared.processed);
        let deadline = Instant::now() + Duration::from_secs(10);
        while *processed < target && Instant::now() < deadline {
            processed = shared
                .settled
                .wait_timeout(processed, Duration::from_millis(50))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{
        BodyType, CharacterControllerComponent, ColliderComponent, JointComponent, JointKind,
    };
    use pulsar_events::gamma::{Channel, DynValue, SubscribeOptions};
    use pulsar_scenedb::Component;
    use pulsar_world_registry::pulsar_scene_model::attachments::{self, NewInstance};
    use pulsar_world_registry::tick_live_components;
    use std::sync::atomic::AtomicUsize;

    /// A component instance entity attached to `owner`.
    fn attach<T: Component>(world: &mut World, owner: Entity, value: T) -> Entity {
        let instance = attachments::spawn_instance(world, owner, NewInstance::new("test")).unwrap();
        world.insert(instance, value);
        instance
    }

    fn object(world: &mut World, position: [f64; 3]) -> Entity {
        let e = world.spawn();
        world.insert(
            e,
            Transform {
                position,
                ..Transform::default()
            },
        );
        e
    }

    /// Frames of 1/60 s, waiting for the simulation thread after each so
    /// results arrive deterministically.
    fn run(world: &mut World, state: &mut ComponentRuntimeState, hub: &EventHub, seconds: f32) {
        for _ in 0..(seconds * 60.0) as usize {
            tick_live_components(world, hub, 1.0 / 60.0, 1, state);
            wait_idle(world);
            hub.flush(pulsar_events::FlushPoint::AfterPhysics);
        }
    }

    fn y(world: &World, e: Entity) -> f64 {
        world.get::<Transform>(e).unwrap().position[1]
    }

    fn slab(world: &mut World) -> Entity {
        let ground = object(world, [0.0, 0.0, 0.0]);
        let mut slab = ColliderComponent::default();
        slab.half_extents = [50.0, 0.5, 50.0];
        attach(world, ground, slab);
        ground
    }

    #[test]
    fn a_dynamic_box_falls_onto_a_static_collider_through_scenedb() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let ground = slab(&mut world);

        let crate_ = object(&mut world, [0.0, 5.0, 0.0]);
        attach(&mut world, crate_, ColliderComponent::default()); // 1 m box
        let body = attach(&mut world, crate_, RigidBodyComponent::default());

        run(&mut world, &mut state, &hub, 3.0);

        // Slab top at 0.5, box half-height 0.5: resting centre near 1.0.
        let rest = y(&world, crate_);
        assert!(
            (0.9..1.1).contains(&rest),
            "box rests on the slab, y = {rest}"
        );
        assert_eq!(world.get::<Transform>(ground).unwrap().position, [0.0; 3]);
        let v = world
            .get::<RigidBodyComponent>(body)
            .unwrap()
            .linear_velocity;
        assert!(v[1].abs() < 0.5, "settled: {v:?}");
        assert_eq!(simulated_counts(&world), (1, 2));
    }

    #[test]
    fn removing_the_body_component_ends_its_simulation() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let owner = object(&mut world, [0.0, 10.0, 0.0]);
        attach(&mut world, owner, ColliderComponent::default());
        let body = attach(&mut world, owner, RigidBodyComponent::default());
        run(&mut world, &mut state, &hub, 0.2);
        assert_eq!(simulated_counts(&world).0, 1);

        world.remove::<RigidBodyComponent>(body);
        run(&mut world, &mut state, &hub, 0.1);
        assert_eq!(simulated_counts(&world).0, 0);
        let before = y(&world, owner);
        run(&mut world, &mut state, &hub, 0.5);
        assert_eq!(y(&world, owner), before, "frozen");
    }

    #[test]
    fn editing_a_component_updates_its_physics_partner() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let owner = object(&mut world, [0.0, 50.0, 0.0]);
        attach(&mut world, owner, ColliderComponent::default());
        let body = attach(&mut world, owner, RigidBodyComponent::default());
        run(&mut world, &mut state, &hub, 0.5);
        assert!(y(&world, owner) < 50.0);

        // Zero gravity: the body should now coast, not accelerate.
        world
            .get_mut::<RigidBodyComponent>(body)
            .unwrap()
            .gravity_scale = 0.0;
        run(&mut world, &mut state, &hub, 0.1);
        let v1 = world
            .get::<RigidBodyComponent>(body)
            .unwrap()
            .linear_velocity[1];
        run(&mut world, &mut state, &hub, 0.5);
        let v2 = world
            .get::<RigidBodyComponent>(body)
            .unwrap()
            .linear_velocity[1];
        assert!(
            (v1 - v2).abs() < 0.2,
            "no longer accelerating: {v1} -> {v2}"
        );

        // Teleporting the object moves the body there.
        world.get_mut::<Transform>(owner).unwrap().position = [7.0, 100.0, 0.0];
        run(&mut world, &mut state, &hub, 0.1);
        let t = world.get::<Transform>(owner).unwrap().position;
        assert!((t[0] - 7.0).abs() < 0.01 && t[1] > 90.0, "moved: {t:?}");

        // Making it static freezes it.
        world.get_mut::<RigidBodyComponent>(body).unwrap().body_type = BodyType::Static;
        run(&mut world, &mut state, &hub, 0.2);
        let frozen = y(&world, owner);
        run(&mut world, &mut state, &hub, 0.3);
        assert_eq!(y(&world, owner), frozen);
    }

    #[test]
    fn colliders_and_bodies_find_each_other_in_either_order() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        slab(&mut world);

        // Body first, collider added later (a body with mass but no shape falls
        // through everything).
        let late_collider = object(&mut world, [-5.0, 3.0, 0.0]);
        let mut body = RigidBodyComponent::default();
        body.mass = 1.0;
        attach(&mut world, late_collider, body);
        run(&mut world, &mut state, &hub, 0.3);
        assert!(y(&world, late_collider) < 3.0, "falls without a shape");
        attach(&mut world, late_collider, ColliderComponent::default());
        world.get_mut::<Transform>(late_collider).unwrap().position = [-5.0, 3.0, 0.0];

        // Collider first (a static fixture), body added later.
        let late_body = object(&mut world, [5.0, 3.0, 0.0]);
        attach(&mut world, late_body, ColliderComponent::default());
        run(&mut world, &mut state, &hub, 0.3);
        assert_eq!(y(&world, late_body), 3.0, "a lone collider does not move");
        attach(&mut world, late_body, RigidBodyComponent::default());

        run(&mut world, &mut state, &hub, 3.0);
        for e in [late_collider, late_body] {
            let rest = y(&world, e);
            assert!((0.9..1.1).contains(&rest), "rests on the slab: {rest}");
        }
    }

    #[test]
    fn collisions_reach_subscribers_in_the_physics_namespace() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let _ = physics_events::register_all(&hub);
        let ground = slab(&mut world);
        let crate_ = object(&mut world, [0.0, 3.0, 0.0]);
        attach(&mut world, crate_, ColliderComponent::default());
        attach(&mut world, crate_, RigidBodyComponent::default());

        let spec = crate::components::COLLISION_ENTER;
        let id = hub
            .descriptor_by_name(&spec.qualified_name())
            .expect("registered in the physics namespace")
            .id;
        // Not reachable without the namespace.
        assert!(hub.descriptor_by_name(&spec.local_name()).is_none());

        let hits = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&hits);
        let other = ground.bits();
        let _sub = hub.bus().subscribe_dyn(
            id,
            SubscribeOptions::channel(Channel::Entity(crate_.bits())),
            move |event| {
                assert_eq!(event.fields[2], DynValue::U64(other));
                seen.fetch_add(1, Ordering::SeqCst);
            },
        );
        run(&mut world, &mut state, &hub, 2.0);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "one collision_enter for the crate"
        );
    }

    #[test]
    fn a_trigger_reports_who_entered_it() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let _ = physics_events::register_all(&hub);

        let zone = object(&mut world, [0.0, 0.0, 0.0]);
        let mut sensor = ColliderComponent::default();
        sensor.is_trigger = true;
        sensor.half_extents = [5.0, 5.0, 5.0];
        attach(&mut world, zone, sensor);
        let mut fixed = RigidBodyComponent::default();
        fixed.body_type = BodyType::Static;
        attach(&mut world, zone, fixed);

        let ball = object(&mut world, [0.0, 20.0, 0.0]);
        let mut shape = ColliderComponent::default();
        shape.shape = crate::components::ColliderShape::Sphere;
        attach(&mut world, ball, shape);
        attach(&mut world, ball, RigidBodyComponent::default());

        let id = hub
            .descriptor_by_name(&crate::components::TRIGGER_ENTER.qualified_name())
            .unwrap()
            .id;
        let entered = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&entered);
        let _sub = hub.bus().subscribe_dyn(
            id,
            SubscribeOptions::channel(Channel::Entity(zone.bits())),
            move |event| log.lock().unwrap().push(event.fields[2].clone()),
        );
        run(&mut world, &mut state, &hub, 3.0);
        assert_eq!(
            entered.lock().unwrap().as_slice(),
            [DynValue::U64(ball.bits())]
        );
    }

    #[test]
    fn a_joint_keeps_a_body_attached_to_the_world() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let pendulum = object(&mut world, [0.0, 10.0, 0.0]);
        attach(&mut world, pendulum, ColliderComponent::default());
        attach(&mut world, pendulum, RigidBodyComponent::default());
        let mut joint = JointComponent::default();
        joint.kind = JointKind::BallSocket;
        joint.anchor = [0.0, 2.0, 0.0]; // pivot above the body
        joint.connected_anchor = [0.0, 12.0, 0.0]; // world point
        attach(&mut world, pendulum, joint);
        run(&mut world, &mut state, &hub, 2.0);
        let t = world.get::<Transform>(pendulum).unwrap().position;
        // Without the joint it would have fallen ~19 m.
        assert!(t[1] > 7.0, "held by the joint: {t:?}");
    }

    #[test]
    fn a_character_controller_walks_and_lands() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        slab(&mut world);
        let hero = object(&mut world, [0.0, 3.0, 0.0]);
        let mut walker = CharacterControllerComponent::default();
        walker.desired_velocity = [2.0, 0.0, 0.0];
        let controller = attach(&mut world, hero, walker);
        run(&mut world, &mut state, &hub, 2.0);

        let t = world.get::<Transform>(hero).unwrap().position;
        assert!(t[0] > 1.5, "walked: {t:?}");
        // Capsule 1.8 tall standing on a slab whose top is 0.5.
        assert!((1.3..1.6).contains(&t[1]), "stands on the ground: {t:?}");
        assert!(
            world
                .get::<CharacterControllerComponent>(controller)
                .unwrap()
                .grounded
        );
    }

    #[test]
    fn an_idle_scene_sends_nothing_to_the_simulation() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let ground = slab(&mut world);
        run(&mut world, &mut state, &hub, 0.2);
        // Nothing changed since the first frames: the journals are empty, so
        // a frame's parts are empty and only time is submitted.
        let mut streams = lock(&SESSIONS)
            .get_mut(&key_of(&world))
            .map(|s| std::mem::take(&mut s.streams))
            .unwrap();
        let (moved, rescan) = read_moves(&world, &mut streams);
        assert!(moved.is_empty() && !rescan);
        assert!(read_parts(&mut world, &mut streams, &moved, rescan).is_empty());
        // Touching one component sends exactly that component.
        world.get_mut::<Transform>(ground).unwrap().position = [1.0, 0.0, 0.0];
        let (moved, rescan) = read_moves(&world, &mut streams);
        assert!(moved.contains(&ground));
        assert!(!read_parts(&mut world, &mut streams, &moved, rescan).is_empty());
    }

    #[test]
    fn the_simulation_runs_on_its_own_named_thread() {
        let mut world = World::new();
        let (mut state, hub) = (ComponentRuntimeState::default(), EventHub::new());
        let owner = object(&mut world, [0.0, 10.0, 0.0]);
        attach(&mut world, owner, RigidBodyComponent::default());
        tick_live_components(&mut world, &hub, 1.0 / 60.0, 1, &mut state);
        wait_idle(&world);
        let name = lock(&SESSIONS).get(&key_of(&world)).and_then(|s| {
            s.thread
                .as_ref()
                .map(|t| t.thread().name().map(str::to_owned))
        });
        assert_eq!(name, Some(Some("Pulsar-Physics".to_owned())));
    }
}
