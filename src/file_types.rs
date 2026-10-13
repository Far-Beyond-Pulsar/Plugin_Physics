//! Physics asset file types.
//!
//! Default content mirrors `pulsar_physics`'s `MaterialPhysicsProps` defaults
//! (friction 0.4, restitution 0.2) so a new material behaves like an
//! unauthored component.

use plugin_editor_api::{FileStructure, FileTypeDefinition, FileTypeId};
use serde_json::json;

pub const PHYSICS_MATERIAL_ID: &str = "physics_material";
pub const PHYSICS_MATERIAL_EXT: &str = "physmat";
pub const COLLISION_LAYERS_ID: &str = "collision_layers";
pub const COLLISION_LAYERS_EXT: &str = "physlayers";

const CATEGORY: &str = "Physics";
const CREATION_DIR: &str = "physics";

/// How two touching materials combine their coefficients.
pub const COMBINE_MODES: [&str; 4] = ["average", "min", "multiply", "max"];

pub fn file_types() -> Vec<FileTypeDefinition> {
    vec![
        FileTypeDefinition {
            id: FileTypeId::new(PHYSICS_MATERIAL_ID),
            extension: PHYSICS_MATERIAL_EXT.to_string(),
            display_name: "Physics Material".to_string(),
            icon: ui::IconName::Droplet,
            color: gpui::rgb(0x4ECDC4).into(),
            structure: FileStructure::Standalone,
            default_content: json!({
                "version": 1,
                "friction": 0.4,
                "restitution": 0.2,   
                "friction_combine": "average",
                "restitution_combine": "average",
                "density": 1.0,
            }),
            categories: vec![CATEGORY.to_string()],
            creation_directory: Some(CREATION_DIR.to_string()),
        },
        FileTypeDefinition {
            id: FileTypeId::new(COLLISION_LAYERS_ID),
            extension: COLLISION_LAYERS_EXT.to_string(),
            display_name: "Collision Layers".to_string(),
            icon: ui::IconName::List,
            color: gpui::rgb(0x4ECDC4).into(),
            structure: FileStructure::Standalone,
            default_content: json!({
                "version": 1,
                "layers": [{ "name": "Default", "collides_with": ["Default"] }],
            }),
            categories: vec![CATEGORY.to_string()],
            creation_directory: Some(CREATION_DIR.to_string()),
        },
    ]
}
