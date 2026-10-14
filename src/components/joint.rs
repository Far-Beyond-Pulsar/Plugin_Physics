//! A constraint between this object's body and another's.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use engine_class_derive::{engine_class, register_world_component};
use pulsar_reflection::Reflectable;
use pulsar_scenedb::{component_id, Entity, World};
use pulsar_world_registry::pulsar_scene_model::Name;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::behavior::{read_rows, Output, Part, PhysEvent, PhysicsBehavior, Row, Rows, Touched};
use crate::declare_events;
use crate::math::vector;
use crate::sim::Sim;

/// The class name scripts and level files use.
pub const JOINT_CLASS_NAME: &str = "JointComponent";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflectable)]
pub enum JointKind {
    /// Welds the bodies together.
    Fixed,
    /// Rotates about `axis` (a door, a wheel). Limits are angles in degrees.
    Hinge,
    /// Slides along `axis`. Limits are distances.
    Slider,
    /// Free rotation about the anchor, like a shoulder.
    BallSocket,
    /// Pulls toward `rest_length` apart with `stiffness` and `damping`.
    Spring,
}

impl Default for JointKind {
    fn default() -> Self {
        Self::Fixed
    }
}

#[engine_class(category = "Physics", clone, debug, serialize, deserialize)]
#[category("Joint", category_color = "#F4C542")]
#[category("Limits", category_color = "#A78BFA")]
#[category("Spring", category_color = "#4ECDC4", default_collapsed = true)]
#[serde(default)]
pub struct JointComponent {
    #[property(category = "Joint")]
    pub kind: JointKind,
    /// Name of the object this joint connects to. Empty: the world.
    #[property(category = "Joint")]
    pub connected_object: String,
    /// Anchor on this object, in its space.
    #[property(category = "Joint")]
    pub anchor: [f32; 3],
    /// Anchor on the connected object, in its space.
    #[property(category = "Joint")]
    pub connected_anchor: [f32; 3],
    /// Hinge and slider axis, in this object's space.
    #[property(category = "Joint")]
    pub axis: [f32; 3],
    #[property(category = "Joint")]
    pub collide_connected: bool,
    /// Force at which the joint breaks. Zero: unbreakable.
    #[property(min = 0.0, max = 1000000.0, step = 1.0, category = "Joint")]
    pub break_force: f32,
    #[property(category = "Limits")]
    pub limits_enabled: bool,
    #[property(min = -100000.0, max = 100000.0, step = 0.1, category = "Limits")]
    pub limit_min: f32,
    #[property(min = -100000.0, max = 100000.0, step = 0.1, category = "Limits")]
    pub limit_max: f32,
    #[property(min = 0.0, max = 1000000.0, step = 0.1, category = "Spring")]
    pub stiffness: f32,
    #[property(min = 0.0, max = 1000000.0, step = 0.1, category = "Spring")]
    pub damping: f32,
    #[property(min = 0.0, max = 100000.0, step = 0.01, category = "Spring")]
    pub rest_length: f32,
}

impl Default for JointComponent {
    fn default() -> Self {
        Self {
            kind: JointKind::Fixed,
            connected_object: String::new(),
            anchor: [0.0; 3],
            connected_anchor: [0.0; 3],
            axis: [1.0, 0.0, 0.0],
            collide_connected: false,
            break_force: 0.0,
            limits_enabled: false,
            limit_min: -45.0,
            limit_max: 45.0,
            stiffness: 100.0,
            damping: 5.0,
            rest_length: 1.0,
        }
    }
}

impl JointComponent {
    /// `(min, max)` in order, or `None` when limits are off.
    pub fn limits(&self) -> Option<(f32, f32)> {
        self.limits_enabled.then(|| {
            (
                self.limit_min.min(self.limit_max),
                self.limit_min.max(self.limit_max),
            )
        })
    }

    /// The axis as a unit vector; X when it is zero or not finite.
    pub fn unit_axis(&self) -> [f32; 3] {
        let [x, y, z] = self.axis;
        let len = (x * x + y * y + z * z).sqrt();
        if len.is_finite() && len > 1e-6 {
            [x / len, y / len, z / len]
        } else {
            [1.0, 0.0, 0.0]
        }
    }
}

#[register_world_component]
impl JointComponent {}

// ---------------------------------------------------------------------------
// Physics behavior
// ---------------------------------------------------------------------------

declare_events! {
    /// The joint's load exceeded `break_force` and it was removed. `other` is
    /// the object it connected to, if any.
    pub const JOINT_BROKEN = (JOINT_CLASS_NAME, "joint_broken");
}

impl JointComponent {
    fn signature(&self, body1: RigidBodyHandle, body2: RigidBodyHandle) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.kind as u8, self.collide_connected, self.limits_enabled).hash(&mut h);
        body1.into_raw_parts().hash(&mut h);
        body2.into_raw_parts().hash(&mut h);
        for v in [
            self.anchor,
            self.connected_anchor,
            self.axis,
            [self.limit_min, self.limit_max, self.stiffness],
            [self.damping, self.rest_length, 0.0],
        ] {
            v.map(f32::to_bits).hash(&mut h);
        }
        h.finish()
    }

    fn joint(&self) -> GenericJoint {
        let axis = vector(self.unit_axis());
        let (a1, a2) = (vector(self.anchor), vector(self.connected_anchor));
        match self.kind {
            JointKind::Fixed => FixedJointBuilder::new()
                .local_anchor1(a1)
                .local_anchor2(a2)
                .contacts_enabled(self.collide_connected)
                .build()
                .into(),
            JointKind::Hinge => {
                let mut b = RevoluteJointBuilder::new(axis)
                    .local_anchor1(a1)
                    .local_anchor2(a2)
                    .contacts_enabled(self.collide_connected);
                if let Some((lo, hi)) = self.limits() {
                    b = b.limits([lo.to_radians(), hi.to_radians()]);
                }
                b.build().into()
            }
            JointKind::Slider => {
                let mut b = PrismaticJointBuilder::new(axis)
                    .local_anchor1(a1)
                    .local_anchor2(a2)
                    .contacts_enabled(self.collide_connected);
                if let Some((lo, hi)) = self.limits() {
                    b = b.limits([lo, hi]);
                }
                b.build().into()
            }
            JointKind::BallSocket => SphericalJointBuilder::new()
                .local_anchor1(a1)
                .local_anchor2(a2)
                .contacts_enabled(self.collide_connected)
                .build()
                .into(),
            JointKind::Spring => {
                SpringJointBuilder::new(self.rest_length, self.stiffness, self.damping)
                    .local_anchor1(a1)
                    .local_anchor2(a2)
                    .contacts_enabled(self.collide_connected)
                    .build()
                    .into()
            }
        }
    }
}

/// A joint row plus the object its `connected_object` name resolved to when
/// it was read.
struct JointPart {
    rows: Rows<JointComponent>,
    connected: HashMap<Entity, Entity>,
}

/// A joint the simulation wants. It is kept without a Rapier joint while
/// either of its bodies is missing, and connected as soon as both exist.
struct Wanted {
    row: Row<JointComponent>,
    connected: Option<Entity>,
    joint: Option<(ImpulseJointHandle, u64)>,
    broken: bool,
}

#[derive(Default)]
struct State {
    joints: HashMap<Entity, Wanted>,
}

fn read(world: &mut World, touched: &Touched) -> Option<Part> {
    let rows = read_rows::<JointComponent>(world, touched, |_| true)?;
    // Names are only looked up for joints that name something.
    let mut connected = HashMap::new();
    if rows
        .upsert
        .iter()
        .any(|r| !r.cfg.connected_object.is_empty())
    {
        let names: HashMap<String, Entity> = world
            .query::<&Name>()
            .map(|(entity, name)| (name.0.clone(), entity))
            .collect();
        for row in &rows.upsert {
            if let Some(&other) = names.get(&row.cfg.connected_object) {
                connected.insert(row.instance, other);
            }
        }
    }
    Some(Box::new(JointPart { rows, connected }) as Part)
}

fn disconnect(sim: &mut Sim, wanted: &mut Wanted) {
    if let Some((handle, _)) = wanted.joint.take() {
        sim.impulse_joints.remove(handle, true);
    }
}

/// Bring the Rapier joint in line with `wanted` and the bodies that exist.
fn connect(sim: &mut Sim, wanted: &mut Wanted) {
    let cfg = &wanted.row.cfg;
    let body1 = sim.owner_body.get(&wanted.row.owner).copied();
    let body2 = match wanted.connected {
        Some(other) => sim.owner_body.get(&other).copied(),
        // Empty name: the world. A name nobody has: not connected yet.
        None if cfg.connected_object.is_empty() => Some(sim.anchor),
        None => None,
    };
    let (Some(body1), Some(body2)) = (body1, body2) else {
        disconnect(sim, wanted);
        return;
    };
    if wanted.broken {
        return;
    }
    let signature = cfg.signature(body1, body2);
    let joint = cfg.joint();
    if let Some((handle, current)) = wanted.joint {
        if current == signature && sim.impulse_joints.get(handle).is_some() {
            return;
        }
        disconnect(sim, wanted);
    }
    let handle = sim.impulse_joints.insert(body1, body2, joint, true);
    wanted.joint = Some((handle, signature));
}

fn forget(state: &mut State, sim: &mut Sim, instance: Entity) {
    if let Some(mut wanted) = state.joints.remove(&instance) {
        disconnect(sim, &mut wanted);
    }
}

fn apply(sim: &mut Sim, part: Part) {
    let Ok(part) = part.downcast::<JointPart>() else {
        return;
    };
    let JointPart { rows, connected } = *part;
    sim.with_state::<State, _>(|state, sim| {
        if rows.replace {
            let live: HashSet<Entity> = rows.upsert.iter().map(|r| r.instance).collect();
            let stale: Vec<Entity> = state
                .joints
                .keys()
                .filter(|i| !live.contains(i))
                .copied()
                .collect();
            for instance in stale {
                forget(state, sim, instance);
            }
        }
        for instance in rows.remove {
            forget(state, sim, instance);
        }
        for row in rows.upsert {
            let connected = connected.get(&row.instance).copied();
            let instance = row.instance;
            // An edit gives a broken joint another chance.
            let mut wanted = match state.joints.remove(&instance) {
                Some(mut old) => {
                    old.row = row;
                    old.connected = connected;
                    old.broken = false;
                    old
                }
                None => Wanted {
                    row,
                    connected,
                    joint: None,
                    broken: false,
                },
            };
            connect(sim, &mut wanted);
            state.joints.insert(instance, wanted);
        }
    });
}

/// A body one end needs appeared or went away: reconnect.
fn settle(sim: &mut Sim) {
    if sim.dirty.is_empty() {
        return;
    }
    sim.with_state::<State, _>(|state, sim| {
        for wanted in state.joints.values_mut() {
            let touches = sim.dirty.contains(&wanted.row.owner)
                || wanted
                    .connected
                    .is_some_and(|other| sim.dirty.contains(&other));
            if touches {
                connect(sim, wanted);
            }
        }
    });
}

fn after_steps(sim: &mut Sim, seconds: f32, out: &mut Output) {
    sim.with_state::<State, _>(|state, sim| {
        for (&instance, wanted) in &mut state.joints {
            let limit = wanted.row.cfg.break_force;
            let Some((handle, _)) = wanted.joint else {
                continue;
            };
            if limit <= 0.0 {
                continue;
            }
            let Some(joint) = sim.impulse_joints.get(handle) else {
                continue;
            };
            let i = joint.impulses;
            let load = (i[0] * i[0] + i[1] * i[1] + i[2] * i[2]).sqrt() / seconds.max(f32::EPSILON);
            if load > limit {
                disconnect(sim, wanted);
                wanted.broken = true;
                out.events.push(PhysEvent {
                    spec: JOINT_BROKEN,
                    owner: wanted.row.owner,
                    component: instance,
                    other: wanted.connected,
                });
            }
        }
    });
}

inventory::submit! {
    PhysicsBehavior {
        name: "joint",
        order: 20,
        component: component_id::<JointComponent>,
        read,
        apply,
        settle: Some(settle),
        before_step: None,
        after_steps: Some(after_steps),
        write_back: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_ordered_and_optional() {
        let mut j = JointComponent::default();
        assert_eq!(j.limits(), None);
        j.limits_enabled = true;
        j.limit_min = 30.0;
        j.limit_max = -10.0;
        assert_eq!(j.limits(), Some((-10.0, 30.0)));
    }

    #[test]
    fn degenerate_axis_falls_back_to_x() {
        let mut j = JointComponent::default();
        j.axis = [0.0; 3];
        assert_eq!(j.unit_axis(), [1.0, 0.0, 0.0]);
        j.axis = [0.0, 3.0, 4.0];
        assert_eq!(j.unit_axis(), [0.0, 0.6, 0.8]);
    }
}
