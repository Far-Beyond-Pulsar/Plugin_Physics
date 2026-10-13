//! A region that pushes bodies: wind, gravity wells, whirlpools, drag zones.

use std::collections::{HashMap, HashSet};

use engine_class_derive::{engine_class, register_world_component};
use pulsar_reflection::Reflectable;
use pulsar_scenedb::{component_id, Entity, World};
use rapier3d::prelude::RigidBodyHandle;
use serde::{Deserialize, Serialize};

use crate::behavior::{read_rows, Output, Part, PhysEvent, PhysicsBehavior, Row, Rows, Touched};
use crate::declare_events;
use crate::math::{array, distance, vector};
use crate::sim::Sim;

/// The class name scripts and level files use.
pub const FORCE_FIELD_CLASS_NAME: &str = "ForceFieldComponent";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflectable)]
pub enum ForceFieldKind {
    /// Along `direction`, everywhere in range (wind, conveyor).
    Directional,
    /// Toward the owner (positive strength) or away (negative).
    Radial,
    /// Around the owner's Y axis (whirlpool, tornado).
    Vortex,
    /// Against the body's velocity (mud, thick air).
    Drag,
}

impl Default for ForceFieldKind {
    fn default() -> Self {
        Self::Directional
    }
}

/// How strength falls off from the centre to `radius`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflectable)]
pub enum FalloffMode {
    Constant,
    Linear,
    Quadratic,
}

impl Default for FalloffMode {
    fn default() -> Self {
        Self::Linear
    }
}

#[engine_class(category = "Physics", clone, debug, serialize, deserialize)]
#[category("Field", category_color = "#4ECDC4")]
#[category("Range", category_color = "#A78BFA")]
#[serde(default)]
pub struct ForceFieldComponent {
    #[property(category = "Field")]
    pub enabled: bool,
    #[property(category = "Field")]
    pub kind: ForceFieldKind,
    /// Force at full strength; for drag, the drag coefficient.
    #[property(min = -100000.0, max = 100000.0, step = 0.1, category = "Field")]
    pub strength: f32,
    /// Directional fields, in the owner's space.
    #[property(category = "Field")]
    pub direction: [f32; 3],
    #[property(min = 0.0, max = 100000.0, step = 0.1, category = "Range")]
    pub radius: f32,
    #[property(category = "Range")]
    pub falloff: FalloffMode,
    /// Only bodies on this layer (from `.physlayers`) feel it. Empty: all.
    #[property(category = "Range")]
    pub affected_layer: String,
}

impl Default for ForceFieldComponent {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: ForceFieldKind::Directional,
            strength: 10.0,
            direction: [0.0, 1.0, 0.0],
            radius: 5.0,
            falloff: FalloffMode::Linear,
            affected_layer: String::new(),
        }
    }
}

impl ForceFieldComponent {
    /// Strength scale in 0..=1 at `distance` from the centre; zero outside
    /// `radius`.
    pub fn falloff_at(&self, distance: f32) -> f32 {
        if !(0.0..=self.radius).contains(&distance) || self.radius <= 0.0 {
            return 0.0;
        }
        let t = 1.0 - distance / self.radius;
        match self.falloff {
            FalloffMode::Constant => 1.0,
            FalloffMode::Linear => t,
            FalloffMode::Quadratic => t * t,
        }
    }
}

#[register_world_component]
impl ForceFieldComponent {}

// ---------------------------------------------------------------------------
// Physics behavior
// ---------------------------------------------------------------------------

declare_events! {
    /// `other` came within the field's radius.
    pub const BODY_ENTERED = (FORCE_FIELD_CLASS_NAME, "body_entered");
    /// `other` left the field's radius.
    pub const BODY_EXITED = (FORCE_FIELD_CLASS_NAME, "body_exited");
}

/// Force a field applies to a body at `position` moving at `velocity`.
pub fn field_force(
    field: &ForceFieldComponent,
    center: [f32; 3],
    position: [f32; 3],
    velocity: [f32; 3],
) -> [f32; 3] {
    let offset = [
        position[0] - center[0],
        position[1] - center[1],
        position[2] - center[2],
    ];
    let scale = field.strength * field.falloff_at(distance(center, position));
    if scale == 0.0 {
        return [0.0; 3];
    }
    let unit = |v: [f32; 3]| {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len <= f32::EPSILON {
            [0.0; 3]
        } else {
            [v[0] / len, v[1] / len, v[2] / len]
        }
    };
    let direction = match field.kind {
        ForceFieldKind::Directional => unit(field.direction),
        ForceFieldKind::Radial => unit([-offset[0], -offset[1], -offset[2]]),
        // Y x offset: counter-clockwise seen from above.
        ForceFieldKind::Vortex => unit([offset[2], 0.0, -offset[0]]),
        ForceFieldKind::Drag => {
            return [-velocity[0] * scale, -velocity[1] * scale, -velocity[2] * scale];
        }
    };
    direction.map(|d| d * scale)
}

/// Simulation-side state: the fields as stepped each run. The working copy
/// is replaced by every edit in SceneDB; the field's centre follows its
/// object when that moves.
#[derive(Default)]
struct State {
    fields: HashMap<Entity, Row<ForceFieldComponent>>,
    /// `(field object, body object)` pairs currently in range.
    inside: HashSet<(Entity, Entity)>,
}

fn read(world: &mut World, touched: &Touched) -> Option<Part> {
    read_rows::<ForceFieldComponent>(world, touched, |c| c.enabled).map(|rows| Box::new(rows) as Part)
}

fn apply(sim: &mut Sim, part: Part) {
    let Ok(rows) = part.downcast::<Rows<ForceFieldComponent>>() else {
        return;
    };
    sim.with_state::<State, _>(|state, _| {
        if rows.replace {
            state.fields.clear();
        }
        for instance in &rows.remove {
            state.fields.remove(instance);
        }
        for mut row in rows.upsert {
            // Rows that did not move arrive with a possibly stale transform:
            // keep the centre the field already had.
            if let (false, Some(old)) = (row.moved, state.fields.get(&row.instance)) {
                row.transform = old.transform;
            }
            state.fields.insert(row.instance, row);
        }
    });
}

/// Push every dynamic body in range, and report who came and went.
fn before_step(sim: &mut Sim, out: &mut Output) {
    let mut forces: Vec<(RigidBodyHandle, [f32; 3])> = Vec::new();
    let mut inside = HashSet::new();
    sim.with_state::<State, _>(|state, sim| {
        if state.fields.is_empty() && state.inside.is_empty() {
            return;
        }
        let owners: Vec<(Entity, RigidBodyHandle)> =
            sim.owner_body.iter().map(|(o, h)| (*o, *h)).collect();
        for (owner, handle) in owners {
            let Some(body) = sim.bodies.get(handle) else {
                continue;
            };
            if !body.is_dynamic() {
                continue;
            }
            let (position, velocity) = (array(body.translation()), array(body.linvel()));
            for field in state.fields.values() {
                let center = field.transform.position;
                let radius = field.cfg.radius;
                if radius > 0.0 && distance(center, position) <= radius {
                    inside.insert((field.owner, owner));
                }
                let force = field_force(&field.cfg, center, position, velocity);
                if force != [0.0; 3] {
                    forces.push((handle, force));
                }
            }
        }
        let by_owner: HashMap<Entity, Entity> = state
            .fields
            .values()
            .map(|f| (f.owner, f.instance))
            .collect();
        for &(field, body) in inside.difference(&state.inside) {
            if let Some(&component) = by_owner.get(&field) {
                out.events.push(PhysEvent {
                    spec: BODY_ENTERED,
                    owner: field,
                    component,
                    other: Some(body),
                });
            }
        }
        for &(field, body) in state.inside.difference(&inside) {
            if let Some(&component) = by_owner.get(&field) {
                out.events.push(PhysEvent {
                    spec: BODY_EXITED,
                    owner: field,
                    component,
                    other: Some(body),
                });
            }
        }
        state.inside = inside;
    });
    // Forces last one step: clear last step's, then add this one's.
    for (_, body) in sim.bodies.iter_mut() {
        body.reset_forces(false);
    }
    for (handle, force) in forces {
        if let Some(body) = sim.bodies.get_mut(handle) {
            body.add_force(vector(force), true);
        }
    }
}

inventory::submit! {
    PhysicsBehavior {
        name: "force_field",
        order: 30,
        component: component_id::<ForceFieldComponent>,
        read,
        apply,
        settle: None,
        before_step: Some(before_step),
        after_steps: None,
        write_back: None,
    }
}

#[cfg(test)]
mod behavior_tests {
    use super::*;

    #[test]
    fn force_fields_fall_off_and_point_the_right_way() {
        let mut field = ForceFieldComponent::default();
        field.kind = ForceFieldKind::Radial;
        field.strength = 10.0;
        field.radius = 10.0;
        field.falloff = FalloffMode::Linear;
        let f = field_force(&field, [0.0; 3], [5.0, 0.0, 0.0], [0.0; 3]);
        assert!(f[0] < 0.0 && f[1] == 0.0, "pulls toward the centre: {f:?}");
        assert_eq!(field_force(&field, [0.0; 3], [11.0, 0.0, 0.0], [0.0; 3]), [0.0; 3]);
        field.kind = ForceFieldKind::Drag;
        let f = field_force(&field, [0.0; 3], [1.0, 0.0, 0.0], [4.0, 0.0, 0.0]);
        assert!(f[0] < 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_is_zero_outside_range() {
        let mut f = ForceFieldComponent::default();
        assert_eq!(f.falloff_at(5.5), 0.0);
        assert_eq!(f.falloff_at(-1.0), 0.0);
        assert_eq!(f.falloff_at(0.0), 1.0);
        assert!((f.falloff_at(2.5) - 0.5).abs() < 1e-6);
        f.falloff = FalloffMode::Quadratic;
        assert!((f.falloff_at(2.5) - 0.25).abs() < 1e-6);
        f.falloff = FalloffMode::Constant;
        assert_eq!(f.falloff_at(5.0), 1.0);
    }
}
