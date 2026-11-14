//! The plugin's `examples/` folder: varied, valid example files of each file
//! type it registers (`.physmat`, `.physlayers`, `.physasset`), served
//! read-only in the plugin's virtual folder and embedded from
//! `examples_assets/`.

use std::sync::Arc;

use engine_fs::{FolderSource, InGame, MemoryFolder, PluginFolder};

/// The example files, embedded; each path is where it sits in the plugin's
/// folder.
pub const FILES: &[engine_fs::EmbeddedFile] = engine_fs::embedded_files!("examples_assets":
    "examples/README.md",
    "examples/materials/default.physmat",
    "examples/materials/concrete.physmat",
    "examples/materials/steel.physmat",
    "examples/materials/wood.physmat",
    "examples/materials/rubber.physmat",
    "examples/materials/ice.physmat",
    "examples/materials/glass.physmat",
    "examples/materials/sand.physmat",
    "examples/materials/grass.physmat",
    "examples/materials/bouncy_ball.physmat",
    "examples/materials/foam.physmat",
    "examples/materials/flesh.physmat",
    "examples/collision_layers/simple.physlayers",
    "examples/collision_layers/platformer.physlayers",
    "examples/collision_layers/vehicles.physlayers",
    "examples/skeletal_physics/humanoid_ragdoll.physasset",
    "examples/skeletal_physics/quadruped_partial.physasset",
    "examples/skeletal_physics/door_hinge.physasset",
);

fn open(folder: PluginFolder) -> Arc<dyn FolderSource> {
    Arc::new(MemoryFolder::with_files(folder, FILES))
}

/// The plugin's one file provider.
pub(crate) fn provider(origin: &'static str) -> engine_fs::FileProvider {
    engine_fs::FileProvider {
        origin,
        open,
        // A game does not ask the plugin: it serves the packager's snapshot.
        in_game: InGame::Snapshot,
    }
}

#[cfg(test)]
mod tests {
    use physics_api::SkeletalPhysicsAsset;

    use super::*;
    use crate::assets::layers::CollisionLayers;
    use crate::assets::material::PhysicsMaterial;
    use crate::assets::{rig, Asset, Severity};

    fn with_ext(ext: &'static str) -> Vec<&'static engine_fs::EmbeddedFile> {
        FILES.iter().filter(|f| f.path.ends_with(ext)).collect()
    }

    fn text(file: &engine_fs::EmbeddedFile) -> &'static str {
        std::str::from_utf8(file.bytes).expect("example is UTF-8")
    }

    #[test]
    fn every_material_parses_cleanly_and_validates() {
        let files = with_ext(".physmat");
        assert!(files.len() >= 8);
        for file in files {
            let (material, notes) =
                PhysicsMaterial::parse(text(file)).unwrap_or_else(|e| panic!("{}: {e}", file.path));
            assert!(notes.is_empty(), "{}: {notes:?}", file.path);
            assert!(!material.description.is_empty(), "{}", file.path);
            let issues = material.validate();
            assert!(
                issues.iter().all(|i| i.severity < Severity::Warning),
                "{}: {issues:?}",
                file.path
            );
            // Nothing the loader would drop.
            assert!(material.extra.is_empty(), "{}: {:?}", file.path, material.extra);
        }
    }

    #[test]
    fn the_materials_show_every_combine_rule_and_many_surfaces() {
        use crate::assets::material::CombineRule;
        let materials: Vec<_> = with_ext(".physmat")
            .into_iter()
            .map(|f| PhysicsMaterial::parse(text(f)).unwrap().0)
            .collect();
        for rule in CombineRule::ALL {
            assert!(
                materials
                    .iter()
                    .any(|m| m.friction_combine == rule || m.restitution_combine == rule),
                "no material uses {rule:?}"
            );
        }
        let surfaces: std::collections::HashSet<_> = materials.iter().map(|m| m.surface).collect();
        assert!(surfaces.len() >= 8);
    }

    #[test]
    fn every_layer_table_parses_cleanly_and_has_no_problems() {
        let files = with_ext(".physlayers");
        assert!(files.len() >= 3);
        for file in files {
            let (layers, notes) = CollisionLayers::parse(text(file))
                .unwrap_or_else(|e| panic!("{}: {e}", file.path));
            assert!(notes.is_empty(), "{}: {notes:?}", file.path);
            let issues = layers.validate();
            assert!(
                issues.iter().all(|i| i.severity < Severity::Warning),
                "{}: {issues:?}",
                file.path
            );
        }
    }

    #[test]
    fn every_skeletal_physics_asset_parses_and_validates() {
        let files = with_ext(".physasset");
        assert!(files.len() >= 3);
        for file in files {
            let (asset, notes) = SkeletalPhysicsAsset::parse(text(file))
                .unwrap_or_else(|e| panic!("{}: {e}", file.path));
            assert!(notes.is_empty(), "{}: {notes:?}", file.path);
            assert!(!asset.bodies.is_empty(), "{}", file.path);
            assert!(asset.extra.is_empty(), "{}", file.path);
            let issues = rig::validate(&asset);
            assert!(issues.is_empty(), "{}: {issues:?}", file.path);
            // Every joint joins two bones that have bodies.
            for joint in &asset.constraints {
                for bone in [&joint.bone_id, &joint.connected_bone_id] {
                    assert!(
                        asset.bodies.iter().any(|b| &b.bone_id == bone),
                        "{}: joint names `{bone}`, which has no body",
                        file.path
                    );
                }
            }
        }
    }

    #[test]
    fn the_folder_has_a_readme() {
        assert!(FILES.iter().any(|f| f.path == "examples/README.md"));
    }
}
