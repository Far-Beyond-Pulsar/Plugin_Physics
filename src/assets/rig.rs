//! `.physasset`: a skeletal mesh's authored rigid bodies and joints.

use std::path::{Path, PathBuf};

use pulsar_physics::SkeletalPhysicsAsset;
use serde_json::Value;
use skeletal_assets::assets::SkeletonAsset;
use skeletal_assets::ids::SkeletalMeshId;
use skeletal_assets::skmesh::SkeletalMeshAsset;

use super::{Asset, Issue, Severity};

pub const SCHEMA_VERSION: u32 = 1;

pub type RigIssue = Issue<()>;

impl Asset for SkeletalPhysicsAsset {
    const KIND: &'static str = "skeletal physics asset";

    fn new_default() -> Self {
        Self::default()
    }

    fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if let Some(version) = value.get("version").and_then(Value::as_u64) {
            if version > u64::from(SCHEMA_VERSION) {
                return Err(format!(
                    "file format version {version} is newer than this editor supports ({SCHEMA_VERSION})"
                ));
            }
        }
        let mut asset: Self = serde_json::from_value(value).map_err(|error| error.to_string())?;
        let mut notes = Vec::new();
        if asset.version == 0 {
            asset.version = SCHEMA_VERSION;
            notes.push("Added the missing version field".into());
        }
        Ok((asset, notes))
    }

    fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

pub fn validate(asset: &SkeletalPhysicsAsset) -> Vec<RigIssue> {
    let mut issues = Vec::new();
    if asset.bodies.is_empty() {
        issues.push(Issue::new(
            Severity::Warning,
            "bodies",
            "No bone bodies are configured",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for body in &asset.bodies {
        if body.bone_id.trim().is_empty() {
            issues.push(Issue::new(
                Severity::Error,
                "bodies",
                "A body is missing its bone id",
            ));
        } else if !seen.insert(body.bone_id.as_str()) {
            issues.push(Issue::new(
                Severity::Error,
                body.bone_id.clone(),
                "More than one body is assigned to this bone",
            ));
        }
        if !body.mass.is_finite() || body.mass < 0.0 {
            issues.push(Issue::new(
                Severity::Error,
                body.bone_id.clone(),
                "Body mass must be finite and non-negative",
            ));
        }
        if body
            .dimensions
            .iter()
            .any(|dimension| !dimension.is_finite() || *dimension < 0.0)
        {
            issues.push(Issue::new(
                Severity::Error,
                body.bone_id.clone(),
                "Shape dimensions must be finite and non-negative",
            ));
        }
    }
    for joint in &asset.constraints {
        if joint.bone_id == joint.connected_bone_id {
            issues.push(Issue::new(
                Severity::Error,
                joint.bone_id.clone(),
                "A bone cannot be constrained to itself",
            ));
        }
        if joint.limits_enabled && joint.limits_degrees.iter().any(|limit| !limit.is_finite()) {
            issues.push(Issue::new(
                Severity::Error,
                joint.bone_id.clone(),
                "Joint limits must be finite",
            ));
        }
    }
    issues
}

/// Resolve the referenced mesh and its skeleton through the project's typed
/// asset ID index. This keeps `.physasset` references valid after files move.
pub fn load_skeleton_for_mesh(
    physasset_path: &Path,
    mesh_id: &SkeletalMeshId,
) -> Result<(SkeletalMeshAsset, SkeletonAsset), String> {
    let index = engine_fs::project_file_index_for_path(physasset_path)
        .ok_or_else(|| "The project asset index is not available for this file".to_string())?;
    let mesh_path = index
        .path_for_asset_id_kind("skmesh", mesh_id.as_str())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("No skeletal mesh has id {mesh_id}"))?;
    let mesh_bytes = engine_fs::virtual_fs::read_file(&mesh_path).map_err(|e| e.to_string())?;
    let mesh = SkeletalMeshAsset::decode(&mesh_bytes)?;
    let skeleton_id = mesh
        .skeleton
        .as_ref()
        .ok_or_else(|| "The selected mesh has no skeleton reference".to_string())?;
    let skeleton_path = index
        .path_for_asset_id_kind("skeleton", skeleton_id.as_str())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("No skeleton has id {skeleton_id}"))?;
    let skeleton_bytes =
        engine_fs::virtual_fs::read_file(&skeleton_path).map_err(|error| error.to_string())?;
    let skeleton_text = String::from_utf8(skeleton_bytes).map_err(|error| error.to_string())?;
    let skeleton = SkeletonAsset::parse(&skeleton_text)?;
    Ok((mesh, skeleton))
}

/// Return every `.physasset` path that resolves to the requested mesh. More
/// than one result makes runtime selection ambiguous, so callers can surface
/// the condition instead of silently hiding the rig.
pub fn physics_assets_for_mesh(
    physasset_path: &Path,
    mesh_id: &SkeletalMeshId,
) -> Result<Vec<PathBuf>, String> {
    let index = engine_fs::project_file_index_for_path(physasset_path)
        .ok_or_else(|| "The project asset index is not available for this file".to_string())?;
    let files = index
        .query(&engine_fs::FileIndexQuery {
            extension: Some("physasset".to_owned()),
            sort: engine_fs::FileIndexSort::Path,
            ..Default::default()
        })
        .map_err(|error| error.to_string())?;
    let mut matches = Vec::new();
    for file in files {
        let path = index
            .project_root()
            .join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(bytes) = engine_fs::virtual_fs::read_file(&path) else {
            continue;
        };
        let Ok(candidate) = serde_json::from_slice::<SkeletalPhysicsAsset>(&bytes) else {
            continue;
        };
        if candidate.skeletal_mesh == *mesh_id {
            matches.push(path);
        }
    }
    Ok(matches)
}
