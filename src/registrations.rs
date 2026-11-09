//! What the physics plugin registers: the one place it submits its
//! contributions, grouped by the engine seam each plugs into. The runtime
//! part is what a game links; the editor part comes with the `editor`
//! feature.
//!
//! The plugin's own registries stay next to what they describe: each
//! component's file registers its simulation behavior
//! (`behavior::PhysicsBehavior`) and declares its events
//! (`physics_events::EventSpec`, through `declare_events!`); this module
//! only hands the plugin's contributions to the engine.

use pulsar_scenedb::component_id;
use pulsar_world_registry::{ComponentEventRegistration, ComponentTickRegistration};

use crate::components::*;

// ── Component classes ───────────────────────────────────────────────────────

pulsar_world_registry::submit_world_component!(
    RigidBodyComponent,
    ColliderComponent,
    JointComponent,
    CharacterControllerComponent,
    ForceFieldComponent,
);

// ── Simulation ──────────────────────────────────────────────────────────────

// The driver: one world-wide tick a frame moves SceneDB's changes to the
// simulation thread and its results back (`crate::runtime`).
inventory::submit! {
    ComponentTickRegistration {
        type_name: "physics_plugin::driver",
        class_name: RIGID_BODY_CLASS_NAME,
        handler_events: &[],
        component_type: component_id::<RigidBodyComponent>,
        tick: crate::runtime::tick,
        // Removal is seen through the journals, not through lifecycle sets.
        end_play: None,
    }
}

// ── Events ──────────────────────────────────────────────────────────────────

/// Offers each declared physics event (`declare_events!`) to scripts: its
/// Gamma descriptor and its script-side declaration.
macro_rules! component_events {
    ($($event:ident),* $(,)?) => {$(
        inventory::submit! {
            ComponentEventRegistration {
                class_name: $event.class,
                descriptor: || $event.descriptor(),
                declaration: || $event.declaration(),
            }
        }
    )*};
}

component_events!(
    SLEEP,
    WAKE,
    COLLISION_ENTER,
    COLLISION_EXIT,
    TRIGGER_ENTER,
    TRIGGER_EXIT,
    JOINT_BROKEN,
    GROUNDED,
    AIRBORNE,
    BODY_ENTERED,
    BODY_EXITED,
);

// ── Editor ──────────────────────────────────────────────────────────────────

/// What the editor adds: the physics asset editors and file types, and the
/// components' "Add component" entries, as a linked editor provider.
#[cfg(feature = "editor")]
mod editor {
    use std::sync::Arc;

    use crate::builtin_provider::PhysicsBuiltinProvider;

    inventory::submit! {
        plugin_manager::PluginEditorProvider {
            origin: pulsar_plugins::origin!(),
            id: PhysicsBuiltinProvider::ID,
            create: || Arc::new(PhysicsBuiltinProvider),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_component_class_is_registered() {
        for class in [
            RIGID_BODY_CLASS_NAME,
            COLLIDER_CLASS_NAME,
            JOINT_CLASS_NAME,
            CHARACTER_CONTROLLER_CLASS_NAME,
            FORCE_FIELD_CLASS_NAME,
        ] {
            assert!(
                pulsar_world_registry::component_id_for_class(class).is_some(),
                "{class} is not a registered world component"
            );
        }
    }

    /// Every event a component declares is offered to scripts, once.
    #[test]
    fn every_declared_event_is_offered_to_scripts() {
        for spec in inventory::iter::<crate::physics_events::EventSpec> {
            let offered = inventory::iter::<ComponentEventRegistration>
                .into_iter()
                .filter(|registration| {
                    registration.class_name == spec.class
                        && (registration.declaration)().name == spec.qualified_name()
                })
                .count();
            assert_eq!(offered, 1, "{}", spec.qualified_name());
        }
    }

    /// Every registration of the plugin names this crate, so it resolves to
    /// the plugin's id.
    #[test]
    fn every_registration_resolves_to_the_plugin() {
        let ours: Vec<_> = pulsar_plugins::contributions()
            .into_iter()
            .filter(|summary| summary.origin == pulsar_plugins::origin!())
            .collect();
        assert!(ours
            .iter()
            .any(|summary| summary.category == "component.class"));
        for summary in &ours {
            assert_eq!(
                pulsar_plugins::plugin_of(summary.origin).map(|plugin| plugin.id.as_str()),
                Some("pulsar.physics"),
                "{summary:?}"
            );
        }
    }
}
