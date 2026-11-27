//! The triangles a mesh collider is made from: the geometry an object shows,
//! its own and that of every object under it in the hierarchy. A level's
//! whole street becomes walkable with one `TriangleMesh` collider on the
//! folder that holds it (Unreal's "use complex collision as simple", gathered
//! per subtree).
//!
//! The triangles come from the engine's content geometry
//! (`engine_backend`'s `content_triangles`, which every kind that draws
//! triangles registers with), so physics names no kind.

use std::collections::HashMap;

use engine_backend::subsystems::render::helio_renderer::content_triangles;
use pulsar_scenedb::{Entity, World};
use pulsar_world_registry::pulsar_scene_model::components::Parent;
use pulsar_world_registry::pulsar_scene_model::hierarchy_transform::world_matrix_of;

/// How deep a hierarchy is followed (as the renderer resolves it).
const MAX_DEPTH: usize = 64;

/// An indexed triangle mesh in one collider owner's simulated frame.
#[derive(Debug, Default)]
pub struct Geometry {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Default)]
struct Builder {
    geometry: Geometry,
    /// Vertex index by the vertex's bits: meshes repeat their vertices.
    welded: HashMap<[u32; 3], u32>,
}

impl Builder {
    fn push(&mut self, corners: [[f32; 3]; 3]) {
        let triangle = corners.map(|corner| {
            *self
                .welded
                .entry(corner.map(f32::to_bits))
                .or_insert_with(|| {
                    self.geometry.vertices.push(corner);
                    (self.geometry.vertices.len() - 1) as u32
                })
        });
        // A triangle whose corners meet has no surface to collide with.
        if triangle[0] != triangle[1] && triangle[1] != triangle[2] && triangle[0] != triangle[2] {
            self.geometry.triangles.push(triangle);
        }
    }
}

/// The nearest of `entity` and its ancestors that is in `wanted`.
fn claimed_by(world: &World, wanted: &HashMap<Entity, glam::DMat4>, entity: Entity) -> Option<Entity> {
    let mut at = entity;
    for _ in 0..MAX_DEPTH {
        if wanted.contains_key(&at) {
            return Some(at);
        }
        at = world.get::<Parent>(at)?.0;
    }
    None
}

/// The geometry of each of `owners`' subtrees, in one pass over the world's
/// content. `owners` maps an object to the matrix taking the world into the
/// frame its collider is placed in. A triangle belongs to the nearest owner
/// above it. An owner whose subtree shows nothing is absent from the result.
pub fn gather(world: &World, owners: &HashMap<Entity, glam::DMat4>) -> HashMap<Entity, Geometry> {
    let mut builders: HashMap<Entity, Builder> = HashMap::new();
    // The owner and the vertex transform of the object whose triangles are
    // being visited: a mesh's triangles arrive together.
    let mut current: Option<(Entity, Option<(Entity, glam::DMat4)>)> = None;
    content_triangles(world, &mut |object, corners| {
        if current.map_or(true, |(last, _)| last != object) {
            let placed = claimed_by(world, owners, object)
                .and_then(|owner| Some((owner, owners[&owner] * world_matrix_of(world, object)?)));
            current = Some((object, placed));
        }
        if let Some((_, Some((owner, matrix)))) = current {
            builders.entry(owner).or_default().push(corners.map(|corner| {
                let point = matrix.transform_point3(glam::DVec3::new(
                    f64::from(corner.x),
                    f64::from(corner.y),
                    f64::from(corner.z),
                ));
                [point.x as f32, point.y as f32, point.z as f32]
            }));
        }
        true
    });
    builders
        .into_iter()
        .filter(|(_, builder)| !builder.geometry.triangles.is_empty())
        .map(|(owner, builder)| (owner, builder.geometry))
        .collect()
}
