//! The physics components' lifecycle: each registers itself in the session's
//! simulation when it begins play and leaves it when it ends
//! (`crate::runtime::begin`/`end`).
//!
//! They use the world form (`WorldActor`, no component borrow) because the
//! simulation is the session's, not the component's: the callback only needs
//! the instance entity and the session scope. Nothing here is a reflected
//! method, so none of it is a Blueprint node. The tables are registered by
//! `crate::registrations` through `ClassSpec::actor`.
//!
//! The world-wide step stays the `pulsar.physics.step` system; what a
//! running component's edits mean to Rapier still flows through SceneDB's
//! journals (`crate::behavior`).

use engine_backend::scene::class_spec::{ActorCx, WorldActor};
use pulsar_scenedb::{Entity, World};

use crate::components::{
    CharacterControllerComponent, ColliderComponent, ForceFieldComponent, JointComponent,
    RigidBodyComponent,
};

macro_rules! physics_lifecycle {
    ($($component:ty),* $(,)?) => {$(
        impl WorldActor for $component {
            fn begin_play(_world: &mut World, cx: &mut ActorCx<'_>, instance: Entity) {
                crate::runtime::begin(cx, instance);
            }

            fn end_play(_world: &mut World, cx: &mut ActorCx<'_>) {
                crate::runtime::end(cx);
            }
        }
    )*};
}

physics_lifecycle!(
    RigidBodyComponent,
    ColliderComponent,
    JointComponent,
    CharacterControllerComponent,
    ForceFieldComponent,
);
