//! The plugin's editor provider: its file types and editors, offered to
//! the editor while the plugin is enabled (`plugin_manager::PluginEditorProvider`).

use gpui::{App, Window};
use plugin_editor_api::*;
use plugin_manager::{BuiltinEditorProvider, EditorContext};
use std::path::PathBuf;
use std::sync::Arc;
use ui::dock::PanelView;

/// Wraps the physics_plugin crate as a built-in provider: the `.physmat` and
/// `.physlayers` file types with their editors (Physics Material and Collision
/// Layers), and the physics components. Linking the crate is what registers
/// the components, their simulation behaviors and the `physics::` events, so
/// the simulation runs in every build of the editor and engine.
pub struct PhysicsBuiltinProvider;

impl BuiltinEditorProvider for PhysicsBuiltinProvider {
    fn provider_id(&self) -> &str {
        "com.pulsar.physics"
    }

    fn file_types(&self) -> Vec<FileTypeDefinition> {
        crate::PhysicsPlugin::default().file_types()
    }

    fn editors(&self) -> Vec<EditorMetadata> {
        crate::PhysicsPlugin::default().editors()
    }

    fn can_handle(&self, editor_id: &EditorId) -> bool {
        crate::PhysicsPlugin::default()
            .editors()
            .iter()
            .any(|editor| editor.id == *editor_id)
    }

    fn component_definitions(&self) -> Vec<ComponentDefinition> {
        crate::PhysicsPlugin::default().component_definitions()
    }

    fn create_editor(
        &self,
        file_path: PathBuf,
        editor_context: &EditorContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Arc<dyn PanelView>, PluginError> {
        // Without an editor id, the file's type says which editor it opens in.
        let Some(editor_id) = crate::PhysicsPlugin::editor_for_path(&file_path) else {
            return Err(PluginError::Other {
                message: format!(
                    "the physics plugin has no editor for {}",
                    file_path.display()
                ),
            });
        };
        self.create_editor_for(&editor_id, file_path, editor_context, window, cx)
    }

    fn create_editor_for(
        &self,
        editor_id: &EditorId,
        file_path: PathBuf,
        editor_context: &EditorContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Arc<dyn PanelView>, PluginError> {
        crate::PhysicsPlugin::create_editor(
            editor_id,
            file_path,
            Some(&editor_context.services.undo),
            window,
            cx,
        )
    }
}

plugin_manager::inventory::submit! {
    plugin_manager::PluginEditorProvider {
        origin: pulsar_plugins::origin!(),
        create: || Arc::new(PhysicsBuiltinProvider),
    }
}
