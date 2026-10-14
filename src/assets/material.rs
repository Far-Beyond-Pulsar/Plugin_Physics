//! `.physmat`: a physics material.
//!
//! Friction, restitution and density, how each combines with the surface it
//! touches, and authoring metadata (a surface type and notes). The numbers
//! mirror `pulsar_physics`'s `MaterialPhysicsProps`, whose defaults are
//! friction 0.4 and restitution 0.2.
//!
//! The model also owns the physics the editor previews: Rapier's combine
//! rules, the height of each bounce, and when a block starts to slide. They
//! are closed-form, so the editor can show them live without running a
//! simulation.

use serde_json::{json, Map, Value};

use super::{Asset, Issue, Severity};

pub const SCHEMA_VERSION: u32 = 1;

/// Standard gravity, m/s².
pub const GRAVITY: f32 = 9.806_65;

/// Density of water in the units densities are authored in (kg/m³).
pub const WATER_DENSITY: f32 = 1000.0;

// ── Combine rules ──────────────────────────────────────────────────────────

/// How two touching materials merge a coefficient into the one the contact
/// uses. The order is Rapier's priority order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum CombineRule {
    #[default]
    Average,
    Min,
    Multiply,
    Max,
}

impl CombineRule {
    pub const ALL: [CombineRule; 4] = [
        CombineRule::Average,
        CombineRule::Min,
        CombineRule::Multiply,
        CombineRule::Max,
    ];

    /// The name stored in the file.
    pub fn id(self) -> &'static str {
        match self {
            CombineRule::Average => "average",
            CombineRule::Min => "min",
            CombineRule::Multiply => "multiply",
            CombineRule::Max => "max",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|rule| rule.id() == text)
    }

    pub fn label(self) -> &'static str {
        match self {
            CombineRule::Average => "Average",
            CombineRule::Min => "Minimum",
            CombineRule::Multiply => "Multiply",
            CombineRule::Max => "Maximum",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            CombineRule::Average => "Half of the two values added: the neutral choice",
            CombineRule::Min => "The lower of the two: the slipperier / deader surface wins",
            CombineRule::Multiply => "The two multiplied: either one at zero zeroes the contact",
            CombineRule::Max => "The higher of the two: the grippier / bouncier surface wins",
        }
    }

    /// When the two materials ask for different rules, the one with the
    /// higher priority wins (Rapier's behavior).
    pub fn priority(self) -> u8 {
        self as u8
    }

    /// The rule a contact uses when its two materials ask for `a` and `b`.
    pub fn resolve(a: CombineRule, b: CombineRule) -> CombineRule {
        if a.priority() >= b.priority() {
            a
        } else {
            b
        }
    }

    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            CombineRule::Average => (a + b) * 0.5,
            CombineRule::Min => a.min(b),
            CombineRule::Multiply => a * b,
            CombineRule::Max => a.max(b),
        }
    }
}

// ── Surface types ──────────────────────────────────────────────────────────

/// What a material is *made of*, for the systems that react to it (footsteps,
/// impact sounds and effects) and for telling materials apart at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SurfaceType {
    #[default]
    Default,
    Concrete,
    Stone,
    Dirt,
    Sand,
    Grass,
    Wood,
    Metal,
    Glass,
    Plastic,
    Rubber,
    Ice,
    Cloth,
    Flesh,
}

impl SurfaceType {
    pub const ALL: [SurfaceType; 14] = [
        SurfaceType::Default,
        SurfaceType::Concrete,
        SurfaceType::Stone,
        SurfaceType::Dirt,
        SurfaceType::Sand,
        SurfaceType::Grass,
        SurfaceType::Wood,
        SurfaceType::Metal,
        SurfaceType::Glass,
        SurfaceType::Plastic,
        SurfaceType::Rubber,
        SurfaceType::Ice,
        SurfaceType::Cloth,
        SurfaceType::Flesh,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SurfaceType::Default => "default",
            SurfaceType::Concrete => "concrete",
            SurfaceType::Stone => "stone",
            SurfaceType::Dirt => "dirt",
            SurfaceType::Sand => "sand",
            SurfaceType::Grass => "grass",
            SurfaceType::Wood => "wood",
            SurfaceType::Metal => "metal",
            SurfaceType::Glass => "glass",
            SurfaceType::Plastic => "plastic",
            SurfaceType::Rubber => "rubber",
            SurfaceType::Ice => "ice",
            SurfaceType::Cloth => "cloth",
            SurfaceType::Flesh => "flesh",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|s| s.id() == text)
    }

    pub fn label(self) -> &'static str {
        match self {
            SurfaceType::Default => "Default",
            SurfaceType::Concrete => "Concrete",
            SurfaceType::Stone => "Stone",
            SurfaceType::Dirt => "Dirt",
            SurfaceType::Sand => "Sand",
            SurfaceType::Grass => "Grass",
            SurfaceType::Wood => "Wood",
            SurfaceType::Metal => "Metal",
            SurfaceType::Glass => "Glass",
            SurfaceType::Plastic => "Plastic",
            SurfaceType::Rubber => "Rubber",
            SurfaceType::Ice => "Ice",
            SurfaceType::Cloth => "Cloth",
            SurfaceType::Flesh => "Flesh",
        }
    }

    /// A swatch color for the surface.
    pub fn tint(self) -> [u8; 3] {
        match self {
            SurfaceType::Default => [0x8a, 0x93, 0xa6],
            SurfaceType::Concrete => [0x9a, 0x9d, 0xa3],
            SurfaceType::Stone => [0x76, 0x7b, 0x84],
            SurfaceType::Dirt => [0x8b, 0x65, 0x3f],
            SurfaceType::Sand => [0xd8, 0xc0, 0x8a],
            SurfaceType::Grass => [0x5f, 0xa8, 0x4f],
            SurfaceType::Wood => [0xb0, 0x7a, 0x45],
            SurfaceType::Metal => [0xa9, 0xb7, 0xc6],
            SurfaceType::Glass => [0x7f, 0xd4, 0xe6],
            SurfaceType::Plastic => [0xe0, 0x6c, 0x8d],
            SurfaceType::Rubber => [0x4a, 0x4f, 0x5a],
            SurfaceType::Ice => [0xb6, 0xe8, 0xf7],
            SurfaceType::Cloth => [0xc9, 0x8f, 0xd6],
            SurfaceType::Flesh => [0xe4, 0x9b, 0x8c],
        }
    }
}

// ── The material ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsMaterial {
    pub version: u32,
    /// Coulomb friction coefficient (0 = ice, ~1 = rubber on asphalt).
    pub friction: f32,
    /// Bounciness: the fraction of speed kept after an impact (0..=1).
    pub restitution: f32,
    pub friction_combine: CombineRule,
    pub restitution_combine: CombineRule,
    /// kg/m³, used to derive a body's mass when its own mass is 0.
    pub density: f32,
    pub surface: SurfaceType,
    pub description: String,
    /// Fields this version doesn't know, kept so a save doesn't drop them.
    pub extra: Map<String, Value>,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            friction: 0.4,
            restitution: 0.2,
            friction_combine: CombineRule::Average,
            restitution_combine: CombineRule::Average,
            density: 1.0,
            surface: SurfaceType::Default,
            description: String::new(),
            extra: Map::new(),
        }
    }
}

/// Repairs the validator can make with one click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialFix {
    ClampFriction,
    ClampRestitution,
    ResetDensity,
}

impl MaterialFix {
    pub fn label(self) -> &'static str {
        match self {
            MaterialFix::ClampFriction => "Clamp friction",
            MaterialFix::ClampRestitution => "Clamp restitution",
            MaterialFix::ResetDensity => "Reset density",
        }
    }

    pub fn apply(self, m: &mut PhysicsMaterial) {
        match self {
            MaterialFix::ClampFriction => {
                m.friction = if m.friction.is_finite() {
                    m.friction.max(0.0)
                } else {
                    0.4
                };
            }
            MaterialFix::ClampRestitution => {
                m.restitution = if m.restitution.is_finite() {
                    m.restitution.clamp(0.0, 1.0)
                } else {
                    0.2
                };
            }
            MaterialFix::ResetDensity => m.density = 1.0,
        }
    }
}

pub type MaterialIssue = Issue<MaterialFix>;

impl PhysicsMaterial {
    pub fn validate(&self) -> Vec<MaterialIssue> {
        let mut out = Vec::new();
        let mut push = |issue: MaterialIssue| out.push(issue);

        if !self.friction.is_finite() || self.friction < 0.0 {
            push(
                Issue::new(
                    Severity::Error,
                    "friction",
                    "Friction must be a number that is 0 or more",
                )
                .with_fix(MaterialFix::ClampFriction),
            );
        } else if self.friction > 2.0 {
            push(Issue::new(
                Severity::Warning,
                "friction",
                "Unusually high: real surface pairs rarely exceed 1.5, and bodies will stick to it",
            ));
        }

        if !self.restitution.is_finite() || self.restitution < 0.0 {
            push(
                Issue::new(
                    Severity::Error,
                    "restitution",
                    "Restitution must be a number that is 0 or more",
                )
                .with_fix(MaterialFix::ClampRestitution),
            );
        } else if self.restitution > 1.0 {
            push(
                Issue::new(
                    Severity::Warning,
                    "restitution",
                    "Above 1 adds energy on every bounce: bodies will fly higher each time",
                )
                .with_fix(MaterialFix::ClampRestitution),
            );
        } else if self.restitution >= 0.99 {
            push(Issue::new(
                Severity::Info,
                "restitution",
                "Almost perfectly elastic: a body bounces for a very long time",
            ));
        }

        if !self.density.is_finite() || self.density <= 0.0 {
            push(
                Issue::new(Severity::Error, "density", "Density must be greater than 0")
                    .with_fix(MaterialFix::ResetDensity),
            );
        } else if self.density > 25_000.0 {
            push(Issue::new(
                Severity::Warning,
                "density",
                "Denser than any real material (osmium is about 22,600 kg/m³)",
            ));
        }

        if self.friction_combine == CombineRule::Multiply && self.friction == 0.0 {
            push(Issue::new(
                Severity::Info,
                "friction_combine",
                "Multiply with zero friction: every contact with this material is frictionless",
            ));
        }
        if self.restitution_combine == CombineRule::Multiply && self.restitution == 0.0 {
            push(Issue::new(
                Severity::Info,
                "restitution_combine",
                "Multiply with zero restitution: nothing bounces off this material",
            ));
        }
        if self.version > SCHEMA_VERSION {
            push(Issue::new(
                Severity::Warning,
                "version",
                format!(
                    "Written by a newer version of the plugin (format {}); unknown fields are kept",
                    self.version
                ),
            ));
        }
        out
    }

    /// The coefficients a contact between `self` and `other` uses.
    pub fn contact_with(&self, other: &PhysicsMaterial) -> ContactResponse {
        let friction_rule = CombineRule::resolve(self.friction_combine, other.friction_combine);
        let restitution_rule =
            CombineRule::resolve(self.restitution_combine, other.restitution_combine);
        ContactResponse {
            friction: friction_rule.apply(self.friction, other.friction),
            restitution: restitution_rule.apply(self.restitution, other.restitution),
            friction_rule,
            restitution_rule,
        }
    }

    /// Mass of a solid box of `size` (metres) made of this material.
    pub fn box_mass(&self, size: [f32; 3]) -> f32 {
        self.density * size[0] * size[1] * size[2]
    }

    /// Mass of a solid sphere of `radius` metres.
    pub fn sphere_mass(&self, radius: f32) -> f32 {
        self.density * (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3)
    }

    /// Whether this material would float in water.
    pub fn floats(&self) -> bool {
        self.density < WATER_DENSITY
    }
}

/// The result of two materials touching.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactResponse {
    pub friction: f32,
    pub restitution: f32,
    pub friction_rule: CombineRule,
    pub restitution_rule: CombineRule,
}

// ── File format ────────────────────────────────────────────────────────────

fn number(map: &Map<String, Value>, key: &str, default: f32, notes: &mut Vec<String>) -> f32 {
    match map.get(key) {
        None => default,
        Some(Value::Number(n)) => n.as_f64().map_or(default, |v| v as f32),
        Some(other) => {
            notes.push(format!(
                "`{key}` should be a number, found {other}; using {default}"
            ));
            default
        }
    }
}

fn rule(map: &Map<String, Value>, key: &str, notes: &mut Vec<String>) -> CombineRule {
    match map.get(key) {
        None => CombineRule::Average,
        Some(Value::String(text)) => CombineRule::parse(text).unwrap_or_else(|| {
            notes.push(format!(
                "`{key}` is `{text}`, which isn't a combine mode; using average"
            ));
            CombineRule::Average
        }),
        Some(other) => {
            notes.push(format!(
                "`{key}` should be a string, found {other}; using average"
            ));
            CombineRule::Average
        }
    }
}

const KNOWN_KEYS: [&str; 8] = [
    "version",
    "friction",
    "restitution",
    "friction_combine",
    "restitution_combine",
    "density",
    "surface",
    "description",
];

impl Asset for PhysicsMaterial {
    const KIND: &'static str = "physics material";

    fn new_default() -> Self {
        Self::default()
    }

    fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        // An empty file is a fresh material, not an error.
        if text.trim().is_empty() {
            return Ok((Self::default(), Vec::new()));
        }
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
        let Value::Object(map) = value else {
            return Err("expected a JSON object".to_string());
        };

        let defaults = Self::default();
        let mut notes = Vec::new();
        let surface = match map.get("surface") {
            None => SurfaceType::Default,
            Some(Value::String(s)) => SurfaceType::parse(s).unwrap_or_else(|| {
                notes.push(format!(
                    "`surface` is `{s}`, which isn't a known surface; using default"
                ));
                SurfaceType::Default
            }),
            Some(other) => {
                notes.push(format!(
                    "`surface` should be a string, found {other}; using default"
                ));
                SurfaceType::Default
            }
        };
        let material = Self {
            version: map
                .get("version")
                .and_then(Value::as_u64)
                .map_or(SCHEMA_VERSION, |v| v as u32),
            friction: number(&map, "friction", defaults.friction, &mut notes),
            restitution: number(&map, "restitution", defaults.restitution, &mut notes),
            friction_combine: rule(&map, "friction_combine", &mut notes),
            restitution_combine: rule(&map, "restitution_combine", &mut notes),
            density: number(&map, "density", defaults.density, &mut notes),
            surface,
            description: map
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            extra: map
                .iter()
                .filter(|(k, _)| !KNOWN_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        Ok((material, notes))
    }

    fn to_text(&self) -> String {
        let mut map = Map::new();
        map.insert("version".into(), json!(self.version));
        map.insert("friction".into(), json!(self.friction));
        map.insert("restitution".into(), json!(self.restitution));
        map.insert("friction_combine".into(), json!(self.friction_combine.id()));
        map.insert(
            "restitution_combine".into(),
            json!(self.restitution_combine.id()),
        );
        map.insert("density".into(), json!(self.density));
        if self.surface != SurfaceType::Default {
            map.insert("surface".into(), json!(self.surface.id()));
        }
        if !self.description.is_empty() {
            map.insert("description".into(), json!(self.description));
        }
        for (key, value) in &self.extra {
            map.entry(key.clone()).or_insert_with(|| value.clone());
        }
        let mut text = serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default();
        text.push('\n');
        text
    }
}

// ── Closed-form physics for the previews ───────────────────────────────────

/// One leg of a bouncing ball's path: it rises from the ground at `start`,
/// peaks, and lands at `end` (the first leg is the drop, which only falls).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BounceArc {
    pub start: f32,
    pub end: f32,
    pub peak: f32,
    /// Whether this leg begins at the apex (the initial drop) rather than the ground.
    pub drop: bool,
}

impl BounceArc {
    /// Height above the ground at time `t` (clamped to this arc).
    pub fn height_at(&self, t: f32, gravity: f32) -> f32 {
        let t = t.clamp(self.start, self.end);
        if self.drop {
            let fall = t - self.start;
            (self.peak - 0.5 * gravity * fall * fall).max(0.0)
        } else {
            let mid = 0.5 * (self.start + self.end);
            let dt = t - mid;
            (self.peak - 0.5 * gravity * dt * dt).max(0.0)
        }
    }
}

/// The path of a ball dropped from `drop_height` metres onto a surface with
/// effective restitution `e`, up to `max_bounces` legs or until a bounce
/// peaks below `rest_height`.
pub fn bounce_arcs(
    drop_height: f32,
    e: f32,
    gravity: f32,
    rest_height: f32,
    max_bounces: usize,
) -> Vec<BounceArc> {
    let drop_height = drop_height.max(0.0);
    let gravity = gravity.max(1e-3);
    let e = if e.is_finite() { e.max(0.0) } else { 0.0 };
    let mut arcs = Vec::new();
    if drop_height <= 0.0 {
        return arcs;
    }
    let fall_time = (2.0 * drop_height / gravity).sqrt();
    arcs.push(BounceArc {
        start: 0.0,
        end: fall_time,
        peak: drop_height,
        drop: true,
    });

    let mut time = fall_time;
    let mut speed = (2.0 * gravity * drop_height).sqrt();
    for _ in 0..max_bounces {
        speed *= e;
        let peak = speed * speed / (2.0 * gravity);
        if peak < rest_height {
            break;
        }
        let duration = 2.0 * speed / gravity;
        arcs.push(BounceArc {
            start: time,
            end: time + duration,
            peak,
            drop: false,
        });
        time += duration;
    }
    arcs
}

/// Time until the ball stops bouncing, in the limit (`None` when it never does).
pub fn settle_time(drop_height: f32, e: f32, gravity: f32) -> Option<f32> {
    if drop_height <= 0.0 {
        return Some(0.0);
    }
    if e >= 1.0 {
        return None;
    }
    let fall = (2.0 * drop_height / gravity.max(1e-3)).sqrt();
    Some(fall * (1.0 + e.max(0.0)) / (1.0 - e.max(0.0)))
}

/// A block on an incline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlopeState {
    pub angle_deg: f32,
    /// The steepest angle the block holds on: `atan(μ)`.
    pub critical_deg: f32,
    pub slipping: bool,
    /// Acceleration down the slope, m/s² (0 while it holds).
    pub acceleration: f32,
    /// Gravity's pull along the slope, m/s² per unit mass.
    pub driving: f32,
    /// Friction's pull back up the slope, m/s² per unit mass.
    pub resisting: f32,
    /// Support from the slope, m/s² per unit mass.
    pub normal: f32,
}

pub fn slope_state(mu: f32, angle_deg: f32, gravity: f32) -> SlopeState {
    let mu = mu.max(0.0);
    let angle = angle_deg.clamp(0.0, 90.0).to_radians();
    let driving = gravity * angle.sin();
    let normal = gravity * angle.cos();
    let max_friction = mu * normal;
    let slipping = driving > max_friction + 1e-6;
    let resisting = if slipping { max_friction } else { driving };
    SlopeState {
        angle_deg: angle_deg.clamp(0.0, 90.0),
        critical_deg: mu.atan().to_degrees(),
        slipping,
        acceleration: if slipping {
            driving - max_friction
        } else {
            0.0
        },
        driving,
        resisting,
        normal,
    }
}

/// Distance a block slides in `seconds` from rest on a slope.
pub fn slide_distance(state: &SlopeState, seconds: f32) -> f32 {
    0.5 * state.acceleration * seconds * seconds
}

// ── Presets ────────────────────────────────────────────────────────────────

/// A named starting point: real-world-ish values for a common surface.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub category: &'static str,
    pub friction: f32,
    pub restitution: f32,
    /// kg/m³.
    pub density: f32,
    pub surface: SurfaceType,
    pub note: &'static str,
}

impl Preset {
    pub fn to_material(&self) -> PhysicsMaterial {
        PhysicsMaterial {
            friction: self.friction,
            restitution: self.restitution,
            density: self.density,
            surface: self.surface,
            description: self.note.to_string(),
            ..PhysicsMaterial::default()
        }
    }

    /// Whether `material` has this preset's numbers.
    pub fn matches(&self, material: &PhysicsMaterial) -> bool {
        (material.friction - self.friction).abs() < 1e-4
            && (material.restitution - self.restitution).abs() < 1e-4
            && (material.density - self.density).abs() < 1e-3
    }
}

pub const PRESET_CATEGORIES: [&str; 4] = ["Engine", "Natural", "Man-made", "Special"];

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Engine default",
        category: "Engine",
        friction: 0.4,
        restitution: 0.2,
        density: 1.0,
        surface: SurfaceType::Default,
        note: "What an unauthored component uses.",
    },
    Preset {
        name: "Dirt",
        category: "Natural",
        friction: 0.6,
        restitution: 0.05,
        density: 1500.0,
        surface: SurfaceType::Dirt,
        note: "Packed earth: grippy and dead.",
    },
    Preset {
        name: "Sand",
        category: "Natural",
        friction: 0.5,
        restitution: 0.02,
        density: 1600.0,
        surface: SurfaceType::Sand,
        note: "Loose and absorbent.",
    },
    Preset {
        name: "Grass",
        category: "Natural",
        friction: 0.55,
        restitution: 0.05,
        density: 1100.0,
        surface: SurfaceType::Grass,
        note: "Turf over soil.",
    },
    Preset {
        name: "Stone",
        category: "Natural",
        friction: 0.65,
        restitution: 0.15,
        density: 2600.0,
        surface: SurfaceType::Stone,
        note: "Cut rock.",
    },
    Preset {
        name: "Ice",
        category: "Natural",
        friction: 0.03,
        restitution: 0.05,
        density: 920.0,
        surface: SurfaceType::Ice,
        note: "Almost frictionless.",
    },
    Preset {
        name: "Wood",
        category: "Natural",
        friction: 0.5,
        restitution: 0.25,
        density: 700.0,
        surface: SurfaceType::Wood,
        note: "Seasoned timber.",
    },
    Preset {
        name: "Flesh",
        category: "Natural",
        friction: 0.6,
        restitution: 0.05,
        density: 1000.0,
        surface: SurfaceType::Flesh,
        note: "Soft tissue.",
    },
    Preset {
        name: "Concrete",
        category: "Man-made",
        friction: 0.7,
        restitution: 0.1,
        density: 2400.0,
        surface: SurfaceType::Concrete,
        note: "Poured and cured.",
    },
    Preset {
        name: "Asphalt",
        category: "Man-made",
        friction: 0.8,
        restitution: 0.05,
        density: 2300.0,
        surface: SurfaceType::Concrete,
        note: "Road surface.",
    },
    Preset {
        name: "Steel",
        category: "Man-made",
        friction: 0.45,
        restitution: 0.2,
        density: 7850.0,
        surface: SurfaceType::Metal,
        note: "Structural steel.",
    },
    Preset {
        name: "Aluminium",
        category: "Man-made",
        friction: 0.4,
        restitution: 0.25,
        density: 2700.0,
        surface: SurfaceType::Metal,
        note: "Light alloy.",
    },
    Preset {
        name: "Glass",
        category: "Man-made",
        friction: 0.3,
        restitution: 0.5,
        density: 2500.0,
        surface: SurfaceType::Glass,
        note: "Smooth and brittle.",
    },
    Preset {
        name: "Plastic",
        category: "Man-made",
        friction: 0.35,
        restitution: 0.4,
        density: 1200.0,
        surface: SurfaceType::Plastic,
        note: "Hard polymer.",
    },
    Preset {
        name: "Rubber",
        category: "Man-made",
        friction: 1.0,
        restitution: 0.8,
        density: 1100.0,
        surface: SurfaceType::Rubber,
        note: "Grips and rebounds.",
    },
    Preset {
        name: "Cloth",
        category: "Man-made",
        friction: 0.7,
        restitution: 0.02,
        density: 400.0,
        surface: SurfaceType::Cloth,
        note: "Fabric and padding.",
    },
    Preset {
        name: "Bouncy ball",
        category: "Special",
        friction: 0.8,
        restitution: 0.95,
        density: 1000.0,
        surface: SurfaceType::Rubber,
        note: "A superball.",
    },
    Preset {
        name: "Frictionless",
        category: "Special",
        friction: 0.0,
        restitution: 0.0,
        density: 1000.0,
        surface: SurfaceType::Default,
        note: "Slides forever, never bounces. Useful for debugging.",
    },
    Preset {
        name: "Sticky",
        category: "Special",
        friction: 2.0,
        restitution: 0.0,
        density: 1000.0,
        surface: SurfaceType::Default,
        note: "Things stay where they land.",
    },
];

/// The preset whose numbers `material` has, if any.
pub fn matching_preset(material: &PhysicsMaterial) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.matches(material))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_modes_match_the_file_type_constants() {
        let ids: Vec<_> = CombineRule::ALL.iter().map(|r| r.id()).collect();
        assert_eq!(ids, crate::COMBINE_MODES);
    }

    #[test]
    fn combine_rules_follow_rapier_priority() {
        use CombineRule::*;
        assert_eq!(CombineRule::resolve(Average, Max), Max);
        assert_eq!(CombineRule::resolve(Min, Multiply), Multiply);
        assert_eq!(CombineRule::resolve(Max, Average), Max);
        assert_eq!(CombineRule::resolve(Min, Min), Min);
        assert_eq!(Average.apply(0.2, 0.6), 0.4);
        assert_eq!(Min.apply(0.2, 0.6), 0.2);
        assert!((Multiply.apply(0.5, 0.5) - 0.25).abs() < 1e-6);
        assert_eq!(Max.apply(0.2, 0.6), 0.6);
    }

    #[test]
    fn contact_uses_the_dominant_rule_per_coefficient() {
        let ice = PRESETS
            .iter()
            .find(|p| p.name == "Ice")
            .unwrap()
            .to_material();
        let mut rubber = PRESETS
            .iter()
            .find(|p| p.name == "Rubber")
            .unwrap()
            .to_material();
        rubber.friction_combine = CombineRule::Max;
        let hit = ice.contact_with(&rubber);
        assert_eq!(hit.friction_rule, CombineRule::Max);
        assert_eq!(hit.friction, 1.0, "the grippier surface wins under max");
        assert_eq!(hit.restitution_rule, CombineRule::Average);
        assert!((hit.restitution - 0.425).abs() < 1e-6);
    }

    #[test]
    fn default_text_matches_the_file_type_template() {
        let (m, notes) = PhysicsMaterial::parse(&PhysicsMaterial::default().to_text()).unwrap();
        assert!(notes.is_empty());
        assert_eq!(m, PhysicsMaterial::default());
        let template = &crate::file_types::file_types()[0].default_content;
        let from_template = PhysicsMaterial::parse(&template.to_string()).unwrap().0;
        assert_eq!(from_template, PhysicsMaterial::default());
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let text = r#"{ "version": 1, "friction": 0.9, "future_feature": { "a": [1, 2, 3] } }"#;
        let (m, _) = PhysicsMaterial::parse(text).unwrap();
        assert_eq!(m.friction, 0.9);
        let saved = m.to_text();
        let value: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["future_feature"]["a"][2], 3);
    }

    #[test]
    fn bad_values_are_repaired_and_reported() {
        let text = r#"{ "friction": "slippery", "friction_combine": "sum", "surface": "lava", "density": 3 }"#;
        let (m, notes) = PhysicsMaterial::parse(text).unwrap();
        assert_eq!(m.friction, 0.4);
        assert_eq!(m.friction_combine, CombineRule::Average);
        assert_eq!(m.surface, SurfaceType::Default);
        assert_eq!(m.density, 3.0);
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(PhysicsMaterial::parse("[1, 2]").is_err());
        assert!(PhysicsMaterial::parse("{ not json").is_err());
        assert!(PhysicsMaterial::parse("   ").unwrap().1.is_empty());
    }

    #[test]
    fn validation_finds_problems_and_the_fixes_clear_them() {
        let mut m = PhysicsMaterial {
            friction: -1.0,
            restitution: 1.5,
            density: 0.0,
            ..Default::default()
        };
        let issues = m.validate();
        assert_eq!(
            issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count(),
            2
        );
        assert!(issues.iter().any(|i| i.severity == Severity::Warning));
        for issue in &issues {
            if let Some(fix) = issue.fix {
                fix.apply(&mut m);
            }
        }
        assert!(
            m.validate().iter().all(|i| i.severity < Severity::Warning),
            "{:?}",
            m.validate()
        );
        assert!(PhysicsMaterial::default().validate().is_empty());
    }

    #[test]
    fn every_preset_is_valid_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for preset in PRESETS {
            assert!(seen.insert(preset.name), "duplicate preset {}", preset.name);
            assert!(
                PRESET_CATEGORIES.contains(&preset.category),
                "{}",
                preset.name
            );
            let issues = preset.to_material().validate();
            assert!(
                issues.iter().all(|i| i.severity < Severity::Warning),
                "{}: {issues:?}",
                preset.name
            );
        }
        let rubber = PRESETS.iter().find(|p| p.name == "Rubber").unwrap();
        assert!(rubber.matches(&rubber.to_material()));
        assert_eq!(
            matching_preset(&rubber.to_material()).unwrap().name,
            "Rubber"
        );
    }

    #[test]
    fn bounce_heights_fall_off_by_restitution_squared() {
        let arcs = bounce_arcs(2.0, 0.5, GRAVITY, 0.001, 8);
        assert!(arcs[0].drop);
        assert!((arcs[0].peak - 2.0).abs() < 1e-6);
        assert!((arcs[1].peak - 2.0 * 0.25).abs() < 1e-4);
        assert!((arcs[2].peak - 2.0 * 0.0625).abs() < 1e-4);
        for pair in arcs.windows(2) {
            assert!(
                (pair[0].end - pair[1].start).abs() < 1e-5,
                "arcs are contiguous"
            );
        }
        // The path is continuous: it touches the ground at each landing.
        for arc in &arcs {
            assert!(arc.height_at(arc.end, GRAVITY) < 1e-3);
        }
        assert!(
            arcs[1].height_at(0.5 * (arcs[1].start + arcs[1].end), GRAVITY) > arcs[1].peak - 1e-4
        );
    }

    #[test]
    fn dead_surfaces_do_not_bounce_and_elastic_ones_never_settle() {
        assert_eq!(bounce_arcs(1.0, 0.0, GRAVITY, 0.001, 8).len(), 1);
        assert_eq!(settle_time(1.0, 1.0, GRAVITY), None);
        let finite = settle_time(1.0, 0.5, GRAVITY).unwrap();
        let summed: f32 = bounce_arcs(1.0, 0.5, GRAVITY, 1e-7, 64).last().unwrap().end;
        assert!(
            (finite - summed).abs() / finite < 0.01,
            "{finite} vs {summed}"
        );
    }

    #[test]
    fn a_block_slips_exactly_past_atan_mu() {
        let state = slope_state(0.5, 20.0, GRAVITY);
        assert!(!state.slipping);
        assert_eq!(state.acceleration, 0.0);
        assert!((state.critical_deg - 26.565).abs() < 0.01);
        let steep = slope_state(0.5, 40.0, GRAVITY);
        assert!(steep.slipping);
        assert!(steep.acceleration > 0.0);
        assert!(steep.resisting < steep.driving);
        // Frictionless: slides at g·sin θ.
        let ice = slope_state(0.0, 30.0, GRAVITY);
        assert!((ice.acceleration - GRAVITY * 0.5).abs() < 1e-3);
        assert!(slide_distance(&ice, 2.0) > 0.0);
    }

    #[test]
    fn masses_follow_density() {
        let steel = PRESETS
            .iter()
            .find(|p| p.name == "Steel")
            .unwrap()
            .to_material();
        assert_eq!(steel.box_mass([1.0, 1.0, 1.0]), 7850.0);
        assert!(!steel.floats());
        assert!(PRESETS
            .iter()
            .find(|p| p.name == "Wood")
            .unwrap()
            .to_material()
            .floats());
        let ball = PhysicsMaterial {
            density: 1000.0,
            ..Default::default()
        };
        assert!((ball.sphere_mass(0.5) - 523.6).abs() < 0.1);
    }
}
