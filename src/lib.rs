#![recursion_limit = "512"]

//! Physics plugin: physics asset file types, the physics components, and the
//! Rapier simulation behind them.
//!
//! The plugin ships as a `cdylib` loaded by `plugin_manager`. Its components
//! register through `#[register_world_component]`; `export_plugin!` attaches
//! this library's world crates to the editor's runtime (Pulsar-Native#1083)
//! so they reach the host World, and shares the editor's profiler.
//!
//! Each component's file owns its simulation behavior (see [`behavior`]);
//! [`runtime`] drives them from SceneDB's change journals on a dedicated
//! thread, and [`physics_events`] publishes what happens in the `physics`
//! event namespace.

use plugin_editor_api::*;

pub mod assets;
mod components;
mod editors;
mod file_types;
mod behavior;
mod math;
mod physics_events;
mod runtime;
mod sim;
pub use components::*;
pub use runtime::{simulated_counts, wait_idle};
pub use editors::{layers::EDITOR_ID as COLLISION_LAYERS_EDITOR_ID, material::EDITOR_ID as PHYSICS_MATERIAL_EDITOR_ID};
pub use file_types::{
    COLLISION_LAYERS_EXT, COLLISION_LAYERS_ID, COMBINE_MODES, PHYSICS_MATERIAL_EXT,
    PHYSICS_MATERIAL_ID,
};
pub use events::PluginEventBus;

#[derive(Default)]
pub struct PhysicsPlugin {
    events: PluginEventBus,
}

impl PluginRuntime for PhysicsPlugin {
    fn start(&mut self) -> &mut dyn PluginRuntime {
        // Initialize the physics plugin runtime here.
        self.events = PluginEventBus::new();
        self
    }
    
    fn stop(&mut self) {
        // Clean up the physics plugin runtime here.
        
    }
}

impl EditorPlugin for PhysicsPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: PluginId::new("com.pulsar.physics"),
            name: "Physics".into(),
            version: "0.1.0".into(),
            author: "Pulsar Team".into(),
            description: "Physics materials, collision layers and the editors for them".into(),
        }
    }

    fn file_types(&self) -> Vec<FileTypeDefinition> {
        file_types::file_types()
    }

    fn editors(&self) -> Vec<EditorMetadata> {
        editors::editors()
    }
}

impl EditorPluginEditor for PhysicsPlugin {
    fn register_editors(&'static self, registry: &mut EditorFactoryRegistry) {
        for editor in editors::editors() {
            let id = editor.id.clone();
            // A dynamically loaded plugin gets the host's undo service through
            // the settings snapshot, so every view of a file shares one history.
            registry.register_fn_with_settings(editor.id, move |path, settings, window, cx| {
                editors::create_editor(&id, path, Some(&settings.services().undo), window, cx)
            });
        }
    }
}

impl PhysicsPlugin {
    /// Open `path` in the editor `editor_id` (for hosts that link this crate).
    /// `undo` is the host's undo service; with it, every view of a file
    /// shares one history.
    pub fn create_editor(
        editor_id: &EditorId,
        path: std::path::PathBuf,
        undo: Option<&UndoService>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<std::sync::Arc<dyn PanelView>, PluginError> {
        editors::create_editor(editor_id, path, undo, window, cx)
    }
}

impl EditorPluginStatusbar for PhysicsPlugin {}
impl EditorPluginAi for PhysicsPlugin {}
impl EditorPluginComponents for PhysicsPlugin {
    fn component_definitions(&self) -> Vec<ComponentDefinition> {
        components::definitions()
    }

    fn component_factories(&self) -> Vec<(String, ComponentFactory)> {
        components::factories()
    }
}
impl EditorPluginSubsystems for PhysicsPlugin {
    fn subsystems(&self) -> Vec<Box<dyn Subsystem>> {
        Vec::new()
    }
}

#[cfg(not(feature = "builtin"))]
export_plugin!(PhysicsPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_unique_physics_file_types() {
        let types = PhysicsPlugin::default().file_types();
        let exts: Vec<_> = types.iter().map(|t| t.extension.as_str()).collect();
        assert_eq!(exts, [PHYSICS_MATERIAL_EXT, COLLISION_LAYERS_EXT]);
        assert!(types.iter().all(|t| t.is_creatable()));
    }

    #[test]
    fn every_component_is_a_registered_world_component_with_a_factory() {
        let defs = PhysicsPlugin::default().component_definitions();
        let factories = PhysicsPlugin::default().component_factories();
        assert_eq!(defs.len(), 5);
        for def in &defs {
            assert!(
                pulsar_world_registry::component_id_for_class(&def.id).is_some(),
                "{} is not a registered world component",
                def.id
            );
            let (_, make) = factories
                .iter()
                .find(|(name, _)| *name == def.id)
                .unwrap_or_else(|| panic!("no factory for {}", def.id));
            assert!(!make().get_properties().is_empty(), "{} reflects nothing", def.id);
        }
    }

    #[test]
    fn material_defaults_are_valid() {
        let types = PhysicsPlugin::default().file_types();
        let m = &types[0].default_content;
        assert!(m["friction"].as_f64().unwrap() >= 0.0);
        let r = m["restitution"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&r));
        for key in ["friction_combine", "restitution_combine"] {
            assert!(COMBINE_MODES.contains(&m[key].as_str().unwrap()));
        }
    }
}
