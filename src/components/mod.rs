//! Physics World components defined by this plugin.
//!
//! Each class is registered with `#[register_world_component]`; the host sees
//! it once `export_plugin!`'s world-runtime attach has run (Pulsar-Native#1083),
//! so the classes are live components of the editor's `World`.

mod character_controller;
mod collider;
mod force_field;
mod joint;
mod rigid_body;
mod skeletal_rig;

pub use character_controller::{
    CharacterControllerComponent, AIRBORNE, CHARACTER_CONTROLLER_CLASS_NAME, GROUNDED,
};
pub use collider::{
    ColliderAxis, ColliderComponent, ColliderShape, ColliderShapeData, COLLIDER_CLASS_NAME,
    COLLISION_ENTER, COLLISION_EXIT, TRIGGER_ENTER, TRIGGER_EXIT,
};
pub use force_field::{
    FalloffMode, ForceFieldComponent, ForceFieldKind, BODY_ENTERED, BODY_EXITED,
    FORCE_FIELD_CLASS_NAME,
};
pub use joint::{JointComponent, JointKind, JOINT_BROKEN, JOINT_CLASS_NAME};
pub use rigid_body::{BodyType, RigidBodyComponent, RIGID_BODY_CLASS_NAME, SLEEP, WAKE};

use plugin_editor_api::{ComponentDefinition, ComponentFactory};
use pulsar_reflection::EngineClass;

fn factory<T: EngineClass + Default + 'static>() -> ComponentFactory {
    Box::new(|| Box::new(T::default()) as Box<dyn EngineClass>)
}

/// The property panel's "Add component" entries.
pub fn definitions() -> Vec<ComponentDefinition> {
    let def = |id: &str, name: &str, description: &str| ComponentDefinition {
        id: id.into(),
        display_name: name.into(),
        category: "Physics".into(),
        description: description.into(),
        icon: None,
    };
    vec![
        def(
            RIGID_BODY_CLASS_NAME,
            "Rigid Body",
            "Makes this object move under the physics simulation: dynamic, kinematic or static",
        ),
        def(
            COLLIDER_CLASS_NAME,
            "Collider",
            "Collision shape: box, sphere, capsule, cylinder, cone or mesh",
        ),
        def(
            JOINT_CLASS_NAME,
            "Joint",
            "Constrains this body to another: fixed, hinge, slider, ball socket or spring",
        ),
        def(
            CHARACTER_CONTROLLER_CLASS_NAME,
            "Character Controller",
            "Kinematic capsule mover with step and slope handling",
        ),
        def(
            FORCE_FIELD_CLASS_NAME,
            "Force Field",
            "Applies directional, radial, vortex or drag forces to bodies in range",
        ),
    ]
}

/// Default-instance factories keyed by class name.
pub fn factories() -> Vec<(String, ComponentFactory)> {
    vec![
        (
            RIGID_BODY_CLASS_NAME.into(),
            factory::<RigidBodyComponent>(),
        ),
        (COLLIDER_CLASS_NAME.into(), factory::<ColliderComponent>()),
        (JOINT_CLASS_NAME.into(), factory::<JointComponent>()),
        (
            CHARACTER_CONTROLLER_CLASS_NAME.into(),
            factory::<CharacterControllerComponent>(),
        ),
        (
            FORCE_FIELD_CLASS_NAME.into(),
            factory::<ForceFieldComponent>(),
        ),
    ]
}
