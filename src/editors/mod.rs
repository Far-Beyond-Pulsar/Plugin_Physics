//! The physics asset editors: one per file type, each a docked workspace of
//! panels over a shared [`common::document::Document`].

pub mod common;
pub mod layers;
pub mod material;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext as _, Window};
use plugin_editor_api::{EditorId, EditorMetadata, FileTypeId, PluginError, UndoService};
use ui::dock::PanelView;

use crate::file_types::{COLLISION_LAYERS_ID, PHYSICS_MATERIAL_ID};

/// The editors this plugin provides.
pub fn editors() -> Vec<EditorMetadata> {
    vec![
        EditorMetadata {
            id: EditorId::new(material::EDITOR_ID),
            display_name: "Physics Material Editor".into(),
            supported_file_types: vec![FileTypeId::new(PHYSICS_MATERIAL_ID)],
        },
        EditorMetadata {
            id: EditorId::new(layers::EDITOR_ID),
            display_name: "Collision Layers Editor".into(),
            supported_file_types: vec![FileTypeId::new(COLLISION_LAYERS_ID)],
        },
    ]
}

/// Open `path` in the editor named `editor_id`. `undo` is the host's undo
/// service, when it has one: with it, every view of a file shares one history.
pub fn create_editor(
    editor_id: &EditorId,
    path: PathBuf,
    undo: Option<&UndoService>,
    window: &mut Window,
    cx: &mut App,
) -> Result<Arc<dyn PanelView>, PluginError> {
    match editor_id.as_str() {
        material::EDITOR_ID => {
            let panel = cx.new(|cx| material::MaterialEditor::new(path, undo, window, cx));
            Ok(Arc::new(panel))
        }
        layers::EDITOR_ID => {
            let panel = cx.new(|cx| layers::LayersEditor::new(path, undo, window, cx));
            Ok(Arc::new(panel))
        }
        other => Err(PluginError::Other {
            message: format!("the physics plugin has no editor `{other}`"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::editors;
    use crate::file_types::file_types;

    #[test]
    fn every_physics_file_type_has_exactly_one_editor() {
        let editors = editors();
        for file_type in file_types() {
            let handling: Vec<_> = editors
                .iter()
                .filter(|e| e.supported_file_types.contains(&file_type.id))
                .collect();
            assert_eq!(
                handling.len(),
                1,
                "{} should have one editor",
                file_type.display_name
            );
        }
        let mut ids: Vec<_> = editors.iter().map(|e| e.id.as_str().to_string()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), editors.len(), "editor ids are unique");
    }
}

#[cfg(test)]
mod render_tests;
