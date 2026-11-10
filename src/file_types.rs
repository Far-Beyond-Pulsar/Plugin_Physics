//! Physics asset file types: their ids, extensions and new-file contents,
//! and (with the `editor` feature) the editor's file type definitions.
//!
//! A new material holds the physics defaults (`assets::material`), so it
//! behaves like an unauthored component.

#[cfg(feature = "editor")]
use plugin_editor_api::{FileStructure, FileTypeDefinition, FileTypeId};
use serde_json::{json, Value};

pub const PHYSICS_MATERIAL_ID: &str = "physics_material";
pub const PHYSICS_MATERIAL_EXT: &str = "physmat";
pub const COLLISION_LAYERS_ID: &str = "collision_layers";
pub const COLLISION_LAYERS_EXT: &str = "physlayers";
pub const SKELETAL_PHYSICS_ID: &str = "skeletal_physics";
pub const SKELETAL_PHYSICS_EXT: &str = "physasset";

#[cfg(feature = "editor")]
const CATEGORY: &str = "Physics";
#[cfg(feature = "editor")]
const CREATION_DIR: &str = "physics";

/// How two touching materials combine their coefficients.
pub const COMBINE_MODES: [&str; 4] = ["average", "min", "multiply", "max"];

/// A new physics material (`.physmat`).
pub fn physics_material_template() -> Value {
    json!({
        "version": 1,
        "friction": crate::assets::material::DEFAULT_FRICTION,
        "restitution": crate::assets::material::DEFAULT_RESTITUTION,
        "friction_combine": "average",
        "restitution_combine": "average",
        "density": 1.0,
    })
}

/// A new collision layer table (`.physlayers`).
pub fn collision_layers_template() -> Value {
    json!({
        "version": 1,
        "layers": [{ "name": "Default", "collides_with": ["Default"] }],
    })
}

/// A new skeletal physics asset (`.physasset`).
pub fn skeletal_physics_template() -> Value {
    json!({
        "version": 1,
        "skeletal_mesh": "00000000-0000-4000-8000-000000000000",
        "bodies": [],
        "constraints": [],
    })
}

/// The editor's physics file types.
#[cfg(feature = "editor")]
pub fn file_types() -> Vec<FileTypeDefinition> {
    vec![
        FileTypeDefinition {
            id: FileTypeId::new(PHYSICS_MATERIAL_ID),
            extension: PHYSICS_MATERIAL_EXT.to_string(),
            display_name: "Physics Material".to_string(),
            icon: ui::IconName::Droplet,
            color: gpui::rgb(0x4ECDC4).into(),
            structure: FileStructure::Standalone,
            default_content: physics_material_template(),
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
            default_content: collision_layers_template(),
            categories: vec![CATEGORY.to_string()],
            creation_directory: Some(CREATION_DIR.to_string()),
        },
        FileTypeDefinition {
            id: FileTypeId::new(SKELETAL_PHYSICS_ID),
            extension: SKELETAL_PHYSICS_EXT.to_string(),
            display_name: "Skeletal Physics Asset".to_string(),
            icon: ui::IconName::Box,
            color: gpui::rgb(0xF4C542).into(),
            structure: FileStructure::Standalone,
            default_content: skeletal_physics_template(),
            categories: vec![CATEGORY.to_string()],
            creation_directory: Some(CREATION_DIR.to_string()),
        },
    ]
}
