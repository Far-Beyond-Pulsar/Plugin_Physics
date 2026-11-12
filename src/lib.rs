#![recursion_limit = "512"]

//! Physics plugin (`pulsar.physics`): physics asset file types, the physics
//! components, and the Rapier simulation behind them.
//!
//! This crate is the plugin's root crate (its only crate): [`registrations`]
//! is the one place it registers what it contributes (through the root's `build`), the runtime part always
//! and the editor part with the `editor` feature. A game links the crate
//! without `editor`, so it builds the components, the simulation and the
//! asset models but no editor; the editor links it with `editor` (the
//! asset editors, file types and editor provider). A DLL build (`editor`
//! without `builtin`) exports [`PhysicsPlugin`] through `export_plugin!`.
//!
//! Each component's file owns its simulation behavior (see `behavior`);
//! the runtime drives them from SceneDB's change journals on a dedicated
//! thread, and `physics_events` publishes what happens in the `physics`
//! event namespace.

#[cfg(feature = "editor")]
use plugin_editor_api::*;

pub mod assets;
mod behavior;
mod components;
#[cfg(feature = "editor")]
mod editors;
mod file_types;
mod math;
mod physics_events;
pub mod registrations;
mod runtime;
mod sim;
pub use components::*;
#[cfg(feature = "editor")]
pub use editors::{
    layers::EDITOR_ID as COLLISION_LAYERS_EDITOR_ID,
    material::EDITOR_ID as PHYSICS_MATERIAL_EDITOR_ID,
    rig::EDITOR_ID as SKELETAL_PHYSICS_EDITOR_ID,
};
#[cfg(feature = "editor")]
pub use events::PluginEventBus;
pub use file_types::{
    collision_layers_template, physics_material_template, skeletal_physics_template,
    COLLISION_LAYERS_EXT, COLLISION_LAYERS_ID, COMBINE_MODES, PHYSICS_MATERIAL_EXT,
    PHYSICS_MATERIAL_ID, SKELETAL_PHYSICS_EXT, SKELETAL_PHYSICS_ID,
};
pub use runtime::{simulated_counts, wait_idle};

#[cfg(test)]
mod golden_metadata;

// This plugin's descriptor (`pulsar-plugin.toml`): linking the crate installs
// it; a project enables it or not (`pulsar_plugins`).
pulsar_plugins::installed!("pulsar-plugin.toml", build);

/// Everything the plugin registers (runtime always, editor with the `editor`
/// feature).
pub fn build(p: &mut pulsar_plugins::PluginBuilder) {
    registrations::build(p);
}

/// The plugin's editor provider (`crate::registrations` registers it).
#[cfg(feature = "editor")]
mod builtin_provider;

/// The plugin as a DLL-loaded editor plugin (`export_plugin!`, a build with
/// `editor` and without `builtin`) and the editor API linked hosts call.
#[cfg(feature = "editor")]
#[derive(Default)]
pub struct PhysicsPlugin {
    events: PluginEventBus,
}

#[cfg(feature = "editor")]
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

#[cfg(feature = "editor")]
impl EditorPlugin for PhysicsPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: PluginId::new("com.pulsar.physics"),
            name: "Physics".into(),
            version: "0.1.0".into(),
            author: "Pulsar Team".into(),
            description:
                "Physics materials, collision layers, skeletal physics assets and their editors"
                    .into(),
        }
    }

    fn file_types(&self) -> Vec<FileTypeDefinition> {
        file_types::file_types()
    }

    fn editors(&self) -> Vec<EditorMetadata> {
        editors::editors()
    }
}

#[cfg(feature = "editor")]
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

#[cfg(feature = "editor")]
impl PhysicsPlugin {
    /// The editor that opens `path`, by its extension.
    pub fn editor_for_path(path: &std::path::Path) -> Option<EditorId> {
        let extension = path.extension()?.to_str()?;
        let file_type = file_types::file_types()
            .into_iter()
            .find(|t| t.extension.eq_ignore_ascii_case(extension))?;
        editors::editors()
            .into_iter()
            .find(|e| e.supported_file_types.contains(&file_type.id))
            .map(|e| e.id)
    }
}

#[cfg(feature = "editor")]
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

#[cfg(feature = "editor")]
impl EditorPluginStatusbar for PhysicsPlugin {}
#[cfg(feature = "editor")]
impl EditorPluginAi for PhysicsPlugin {}
#[cfg(feature = "editor")]
impl EditorPluginComponents for PhysicsPlugin {
    fn component_definitions(&self) -> Vec<ComponentDefinition> {
        components::definitions()
    }

    fn component_factories(&self) -> Vec<(String, ComponentFactory)> {
        components::factories()
    }
}
#[cfg(feature = "editor")]
#[cfg(all(feature = "editor", not(feature = "builtin")))]
export_plugin!(PhysicsPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "editor")]
    #[test]
    fn registers_unique_physics_file_types() {
        let types = PhysicsPlugin::default().file_types();
        let exts: Vec<_> = types.iter().map(|t| t.extension.as_str()).collect();
        assert_eq!(
            exts,
            [
                PHYSICS_MATERIAL_EXT,
                COLLISION_LAYERS_EXT,
                SKELETAL_PHYSICS_EXT
            ]
        );
        assert!(types.iter().all(|t| t.is_creatable()));
    }

    #[cfg(feature = "editor")]
    #[test]
    fn every_component_is_a_registered_world_component_with_a_factory() {
        let defs = PhysicsPlugin::default().component_definitions();
        let factories = PhysicsPlugin::default().component_factories();
        assert_eq!(defs.len(), components::definitions().len());
        assert_eq!(defs.len(), 7);
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
            assert!(
                !make().get_properties().is_empty(),
                "{} reflects nothing",
                def.id
            );
        }
    }

    #[cfg(feature = "editor")]
    #[test]
    fn each_asset_type_opens_in_its_own_editor() {
        use std::path::Path;
        let editors = PhysicsPlugin::default().editors();
        assert_eq!(editors.len(), 3);
        assert_eq!(
            PhysicsPlugin::editor_for_path(Path::new("assets/physics/Rubber.physmat"))
                .unwrap()
                .as_str(),
            PHYSICS_MATERIAL_EDITOR_ID
        );
        assert_eq!(
            PhysicsPlugin::editor_for_path(Path::new("Layers.PHYSLAYERS"))
                .unwrap()
                .as_str(),
            COLLISION_LAYERS_EDITOR_ID,
            "extensions match regardless of case"
        );
        assert!(PhysicsPlugin::editor_for_path(Path::new("notes.txt")).is_none());
        assert_eq!(
            PhysicsPlugin::editor_for_path(Path::new("rigs/Character.physasset"))
                .unwrap()
                .as_str(),
            SKELETAL_PHYSICS_EDITOR_ID
        );
        assert!(PhysicsPlugin::editor_for_path(Path::new("no_extension")).is_none());
    }

    #[test]
    fn material_defaults_are_valid() {
        let m = &physics_material_template();
        assert!(m["friction"].as_f64().unwrap() >= 0.0);
        let r = m["restitution"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&r));
        for key in ["friction_combine", "restitution_combine"] {
            assert!(COMBINE_MODES.contains(&m[key].as_str().unwrap()));
        }
    }
}
