# Plugin_Physics

Physics for the Pulsar editor and engine: asset types, five physics
components, and a [Rapier](https://rapier.rs) simulation that runs on its own
thread and stays in step with SceneDB.

This repository is vendored into [Pulsar-Native](https://github.com/Far-Beyond-Pulsar/Pulsar-Native)
as a git submodule at `plugins/vendor/physics_plugin`.

- [What it provides](#what-it-provides)
- [Components](#components)
- [Asset types](#asset-types)
- [Events](#events)
- [How it works](#how-it-works)
- [Adding or changing a component](#adding-or-changing-a-component)
- [Threading, profiling and timing](#threading-profiling-and-timing)
- [Building and testing](#building-and-testing)
- [Current limits](#current-limits)
- [Source layout](#source-layout)

## What it provides

| | |
|---|---|
| Components | `RigidBodyComponent`, `ColliderComponent`, `JointComponent`, `CharacterControllerComponent`, `ForceFieldComponent` |
| Asset types | Physics Material (`.physmat`), Collision Layers (`.physlayers`) |
| Simulation | Rapier 3D on a dedicated `Pulsar-Physics` thread per world |
| Events | Eleven events in Gamma's `physics` namespace, hookable from scripts |
| Profiler | `Physics::*` scopes in the editor's flamegraph |

It builds as a `cdylib` loaded by the editor's plugin manager and as an
`rlib` (with the `builtin` feature) for statically linked hosts.

## Components

All five appear in the property panel's *Add component* list under
**Physics**. Components are ordinary SceneDB components: they live on
component-instance entities attached to an object, and the object's
`Transform` is what the simulation moves.

### RigidBodyComponent

Makes an object move under the simulation.

| Property | Meaning |
|---|---|
| `body_type` | `Dynamic` (forces, gravity, collisions), `Kinematic` (follows the object's `Transform`, pushes dynamic bodies) or `Static` (never moves) |
| `mass` | Total mass in kg; `0` derives it from the colliders' density |
| `gravity_scale`, `linear_damping`, `angular_damping` | Per-body tuning |
| `can_sleep`, `continuous_collision` | Resting and anti-tunnelling |
| `lock_position_*`, `lock_rotation_*` | Per-axis constraints |
| `linear_velocity`, `angular_velocity` | The starting velocity; the simulation keeps them current while the body moves, and writing them changes the body |

One body per object: the first body component wins.

### ColliderComponent

A collision shape: box, sphere, capsule, cylinder or cone, with `center` and
`axis`. It attaches to the body on the same object, or stands as a static
fixture when there is none. `is_trigger` makes it a sensor that reports
overlaps instead of blocking. Colliders and bodies find each other in either
order, and when a body is added or removed the collider re-attaches or stays
where it was.

### JointComponent

Constrains this object's body to another object's (`connected_object`, by
name) or to the world when empty. Kinds: `Fixed`, `Hinge`, `Slider`,
`BallSocket`, `Spring`; with anchors, an axis, optional limits (degrees for
hinges), spring stiffness/damping/rest length and `collide_connected`.
`break_force` removes the joint when its load exceeds it. A joint waits
until both bodies exist and connects as soon as they do.

### CharacterControllerComponent

A kinematic capsule mover built on Rapier's `KinematicCharacterController`:
step height, slope limit, snap-to-ground and skin width. Scripts drive it:

| Property | Direction | Meaning |
|---|---|---|
| `desired_velocity` | in | Walking velocity in m/s, written each frame |
| `vertical_velocity` | in/out | Gravity changes it; set it positive to jump |
| `grounded` | out | Whether it stood on ground after its last move |

A controller owns its object's body; do not add a `RigidBodyComponent` to the
same object.

### ForceFieldComponent

A region that pushes dynamic bodies: `Directional`, `Radial`, `Vortex` or
`Drag`, with `strength`, `radius` and `falloff` (`Constant`, `Linear`,
`Quadratic`).

## Asset types

| Asset | Extension | Contents |
|---|---|---|
| Physics Material | `.physmat` | friction, restitution, their combine modes, density |
| Collision Layers | `.physlayers` | named layers and their collision matrix |

Both are plain JSON and creatable from the file manager under **physics/**.

## Events

Every component can raise events that scripts and Rust code hook like any
other engine event. They live in Gamma's `physics` namespace, so scripts name
them `physics::<Class>.<event>` (the unqualified name never resolves).
Events are published on the owning object's entity channel, and delivered at
the engine's *after physics* flush, before the script phase.

Each carries three entities, in this order:

| Field | Meaning |
|---|---|
| `entity` | The object the component is on |
| `component` | The component instance that raised it |
| `other` | The other party (the thing hit, entered, connected to), or `0` |

| Event | Raised when |
|---|---|
| `physics::ColliderComponent.collision_enter` / `collision_exit` | A collider starts or stops touching `other` |
| `physics::ColliderComponent.trigger_enter` / `trigger_exit` | `other` enters or leaves a trigger collider |
| `physics::RigidBodyComponent.sleep` / `wake` | A body comes to rest or starts moving |
| `physics::JointComponent.joint_broken` | A joint exceeded `break_force` |
| `physics::CharacterControllerComponent.grounded` / `airborne` | A controller lands or leaves the ground |
| `physics::ForceFieldComponent.body_entered` / `body_exited` | A body enters or leaves a field's radius |

Subscribing from Rust:

```rust
let id = hub
    .descriptor_by_name("physics::ColliderComponent.collision_enter")
    .unwrap()
    .id;
let _sub = hub.bus().subscribe_dyn(
    id,
    SubscribeOptions::channel(Channel::Entity(object.bits())),
    |event| { /* event.fields = [entity, component, other] as U64 entity bits */ },
);
```

## How it works

SceneDB is the source of truth. Components are never copied into a second
world; the simulation follows them through SceneDB's change journals.

```
 game thread                                     Pulsar-Physics thread
 -----------                                     ---------------------
 tick (component runtime, once per frame)
   1. Moves   Transform journal: which objects
              did something other than physics
              move?
   2. Apply   write finished results into
              SceneDB; publish events
   3. Read    per-component journals -> Rows
              messages of just what changed  --> apply rows to Rapier
              + elapsed time                     fixed 60 Hz steps
                                                 before/after-step behavior hooks
              (next frame) <-------------------- results + events
```

- **Changes, not scans.** Each component type has a journal cursor; a frame
  in which nothing changed costs a few atomic loads. A component edited by the
  inspector, a script or a level load reaches its Rapier counterpart on the
  next run. Only the first read (and a journal overflow) re-sends everything.
- **Results come back one frame late**, and nothing on the game thread waits
  for the simulation. A result for an object that was moved by someone else
  since the run began is dropped; the move is already on its way to the
  simulation.
- **What the simulation thread keeps** is Rapier's own state, change
  tokens, and the working copy of rows it must step without the `World`
  (force fields, character controllers, joints). SceneDB stays authoritative
  and every edit replaces those copies. Deleting the whole simulation loses
  nothing: one full re-read rebuilds it.

The component runtime only offers one world-wide hook per plugin (a
`ComponentTickRegistration`), so `runtime.rs` registers a single driver and
knows no component by name.

## Adding or changing a component

Behavior lives in the component's own file. A component registers a
`PhysicsBehavior` next to its definition:

```rust
inventory::submit! {
    PhysicsBehavior {
        name: "my_component",
        order: 15,                       // bodies 0, colliders 10, joints 20
        component: component_id::<MyComponent>,
        read,                            // game thread: journals -> message
        apply,                           // sim thread: message -> Rapier
        settle: None,                    // after all messages (re-attach etc.)
        before_step: None,               // every fixed step
        after_steps: Some(after_steps),  // collect results and events
        write_back: Some(write_back),    // game thread: batch -> SceneDB
    }
}
```

- `read` usually calls `read_rows::<MyComponent>(world, touched, |c| c.enabled)`.
- `apply` runs on the simulation thread with `&mut Sim`: Rapier's sets, the
  owner-to-body map and typed per-behavior state (`sim.with_state::<T, _>`).
- Declare events in the same file with `declare_events!`; they are registered
  on the hub and offered to scripts automatically.
- Push at most one result batch per run to `Output`, and apply it in
  `write_back` with `ctx.write_transform(...)`, which skips stale results.

`order` decides application order within a run: bodies before the colliders
that attach to them, colliders before the joints that connect bodies.

## Threading, profiling and timing

- One simulation thread per `World`, named `Pulsar-Physics`, shut down after
  ten idle seconds. A panic in a step restarts the simulation from the scene.
- Fixed 60 Hz steps with an accumulator (at most five per run), so results do
  not depend on the frame rate.
- `export_plugin!` shares the editor's profiler with this library, so the
  `Physics::*` scopes appear in the flamegraph: `Tick`, `Moves`, `Apply`,
  `Read` on the game thread; `Frame`, `Reconcile`, `Step`, `Collect`,
  `Publish` on the simulation thread. Plugins built before profiler sharing
  keep a private (never read) profiler.

## Building and testing

This crate depends on workspace dependencies, so it builds as a member of the
Pulsar-Native workspace:

```bash
cargo check -p physics_plugin
cargo test  -p physics_plugin
cargo check -p physics_plugin --features builtin   # statically linked hosts
```

Tests run the real component runtime against a SceneDB `World`: bodies fall
and rest on colliders, edits and teleports reach Rapier, collisions and
triggers arrive as `physics::` events, joints hold, characters walk and land,
and an idle scene sends nothing to the simulation.

## Current limits

- Mesh colliders (convex hull, triangle mesh) are skipped with a warning.
- `.physmat` materials and `.physlayers` filtering are not applied yet:
  friction and restitution use Rapier defaults.
- Object scale and parent transforms are ignored; bodies use the object's
  own `Transform`.
- `push_dynamic_bodies` on the character controller is not implemented.
- `break_force` compares a joint's impulse per second, a rough measure.

## Source layout

| File | Role |
|---|---|
| `src/lib.rs` | Plugin entry point and editor registration |
| `src/components/*.rs` | Each component, its behavior and its events |
| `src/behavior.rs` | The `PhysicsBehavior` contract and `read_rows` |
| `src/sim.rs` | The shared Rapier world and fixed-step loop |
| `src/runtime.rs` | The driver: journals in, thread, results out |
| `src/physics_events.rs` | The `physics` event namespace |
| `src/math.rs` | `Transform` to Rapier conversions |
| `src/file_types.rs` | `.physmat` and `.physlayers` |

## License

MIT, as the rest of Pulsar.
