//! `.physlayers`: named collision layers and which of them collide.
//!
//! Each layer lists the layers it collides with. A pair collides only when
//! **both** lists name each other, the same test Rapier applies to collision
//! groups (`a.filter & b.membership && b.filter & a.membership`). The editor
//! therefore treats the matrix as symmetric and reports a one-sided entry as
//! a problem, since it silently does nothing.
//!
//! A layer's position is its bit in the 32-bit group masks, so reordering
//! layers renumbers them: there are at most [`MAX_LAYERS`].

use serde_json::{json, Map, Value};

use super::{auto_color, hex_color, parse_hex_color, Asset, Issue, Severity};

pub const SCHEMA_VERSION: u32 = 1;

/// Rapier collision groups are 32-bit masks.
pub const MAX_LAYERS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    /// Names of the layers this one collides with (it may list itself).
    pub collides_with: Vec<String>,
    /// `#rrggbb`, for the editor and debug draw. Optional.
    pub color: Option<String>,
    pub description: String,
    pub extra: Map<String, Value>,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            collides_with: Vec::new(),
            color: None,
            description: String::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CollisionLayers {
    pub version: u32,
    pub layers: Vec<Layer>,
    pub extra: Map<String, Value>,
}

impl Default for CollisionLayers {
    fn default() -> Self {
        let mut default = Layer::new("Default");
        default.collides_with.push("Default".to_string());
        Self { version: SCHEMA_VERSION, layers: vec![default], extra: Map::new() }
    }
}

/// What to do to every pairing of one layer at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowOp {
    CollideWithAll,
    CollideWithNone,
    Invert,
}

/// How to resolve one-sided entries across the whole matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symmetry {
    /// A pair collides if either side listed the other.
    Union,
    /// A pair collides only if both did (what the engine does today).
    Intersection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameError {
    Empty,
    Duplicate,
}

impl RenameError {
    pub fn message(self) -> &'static str {
        match self {
            RenameError::Empty => "A layer needs a name",
            RenameError::Duplicate => "Another layer already has that name",
        }
    }
}

/// Repairs the validator can make with one click. They name layers rather
/// than index them, since an earlier repair may have shifted the indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerFix {
    /// Make `a` and `b` agree: both collide, or neither does.
    SymmetrizePair { a: String, b: String, collide: bool },
    /// Remove a reference to a layer that doesn't exist.
    RemoveDanglingReference { layer: String, reference: String },
    /// Give a layer a fresh unique name (empty or duplicate names).
    Rename { index: usize },
    /// Drop repeated entries from a layer's list.
    Deduplicate { layer: String },
    /// Add the `Default` layer new objects start on.
    AddDefaultLayer,
    /// Remove layers past the 32nd, which can't be represented.
    TrimToLimit,
}

impl LayerFix {
    pub fn label(&self) -> String {
        match self {
            LayerFix::SymmetrizePair { collide: true, .. } => "Make both collide".to_string(),
            LayerFix::SymmetrizePair { collide: false, .. } => "Make neither collide".to_string(),
            LayerFix::RemoveDanglingReference { .. } => "Remove reference".to_string(),
            LayerFix::Rename { .. } => "Rename".to_string(),
            LayerFix::Deduplicate { .. } => "Remove repeats".to_string(),
            LayerFix::AddDefaultLayer => "Add Default".to_string(),
            LayerFix::TrimToLimit => format!("Keep the first {MAX_LAYERS}"),
        }
    }
}

pub type LayerIssue = Issue<LayerFix>;

impl CollisionLayers {
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }

    /// Whether layer `a`'s list names layer `b`.
    pub fn lists(&self, a: usize, b: usize) -> bool {
        match (self.layers.get(a), self.layers.get(b)) {
            (Some(la), Some(lb)) => la.collides_with.contains(&lb.name),
            _ => false,
        }
    }

    /// Whether `a` and `b` actually collide: both name each other.
    pub fn collides(&self, a: usize, b: usize) -> bool {
        self.lists(a, b) && self.lists(b, a)
    }

    /// One layer names the other and the other doesn't: the entry does nothing.
    pub fn is_one_sided(&self, a: usize, b: usize) -> bool {
        a != b && self.lists(a, b) != self.lists(b, a)
    }

    fn set_directed(&mut self, a: usize, b: usize, on: bool) {
        let Some(target) = self.layers.get(b).map(|l| l.name.clone()) else { return };
        let Some(layer) = self.layers.get_mut(a) else { return };
        let present = layer.collides_with.contains(&target);
        match (on, present) {
            (true, false) => layer.collides_with.push(target),
            (false, true) => layer.collides_with.retain(|n| *n != target),
            _ => {}
        }
    }

    /// Make `a` and `b` collide (or not), on both sides.
    pub fn set_pair(&mut self, a: usize, b: usize, on: bool) {
        self.set_directed(a, b, on);
        if a != b {
            self.set_directed(b, a, on);
        }
    }

    /// Flip a pair. A one-sided pair becomes a colliding one.
    pub fn toggle_pair(&mut self, a: usize, b: usize) {
        let now = self.collides(a, b);
        self.set_pair(a, b, !now);
    }

    /// Apply `op` to every pairing of layer `i`, keeping the matrix symmetric.
    pub fn apply_row(&mut self, i: usize, op: RowOp) {
        for j in 0..self.layers.len() {
            let on = match op {
                RowOp::CollideWithAll => true,
                RowOp::CollideWithNone => false,
                RowOp::Invert => !self.collides(i, j),
            };
            self.set_pair(i, j, on);
        }
    }

    pub fn collide_all(&mut self) {
        for i in 0..self.layers.len() {
            for j in i..self.layers.len() {
                self.set_pair(i, j, true);
            }
        }
    }

    pub fn collide_none(&mut self) {
        for layer in &mut self.layers {
            layer.collides_with.clear();
        }
    }

    /// Resolve every one-sided pair by `strategy`.
    pub fn make_symmetric(&mut self, strategy: Symmetry) {
        for i in 0..self.layers.len() {
            for j in (i + 1)..self.layers.len() {
                if self.is_one_sided(i, j) {
                    self.set_pair(i, j, strategy == Symmetry::Union);
                }
            }
        }
    }

    /// `base`, or `base 2`, `base 3`… whichever isn't taken.
    pub fn unique_name(&self, base: &str) -> String {
        let base = if base.trim().is_empty() { "Layer" } else { base.trim() };
        if self.index_of(base).is_none() {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|candidate| self.index_of(candidate).is_none())
            .unwrap()
    }

    /// Add a layer that collides with everything, itself included. `None`
    /// when the limit is reached.
    pub fn add(&mut self, name: Option<&str>) -> Option<usize> {
        if self.layers.len() >= MAX_LAYERS {
            return None;
        }
        let name = self.unique_name(name.unwrap_or("Layer"));
        let mut layer = Layer::new(name);
        layer.color = Some(hex_color(auto_color(self.layers.len())));
        self.layers.push(layer);
        let new = self.layers.len() - 1;
        self.apply_row(new, RowOp::CollideWithAll);
        Some(new)
    }

    /// Rename layer `i`, updating every list that names it.
    pub fn rename(&mut self, i: usize, new_name: &str) -> Result<(), RenameError> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(RenameError::Empty);
        }
        let Some(old) = self.layers.get(i).map(|l| l.name.clone()) else { return Ok(()) };
        if old == new_name {
            return Ok(());
        }
        if self
            .layers
            .iter()
            .enumerate()
            .any(|(j, l)| j != i && l.name.eq_ignore_ascii_case(new_name))
        {
            return Err(RenameError::Duplicate);
        }
        for layer in &mut self.layers {
            for entry in &mut layer.collides_with {
                if *entry == old {
                    *entry = new_name.to_string();
                }
            }
        }
        self.layers[i].name = new_name.to_string();
        Ok(())
    }

    /// Remove layer `i` and every reference to it.
    pub fn remove(&mut self, i: usize) {
        if i >= self.layers.len() {
            return;
        }
        let removed = self.layers.remove(i).name;
        for layer in &mut self.layers {
            layer.collides_with.retain(|n| *n != removed);
        }
    }

    /// Copy layer `i` right after itself, colliding with what it collides with.
    pub fn duplicate(&mut self, i: usize) -> Option<usize> {
        if i >= self.layers.len() || self.layers.len() >= MAX_LAYERS {
            return None;
        }
        let source = self.layers[i].clone();
        let name = self.unique_name(&format!("{} Copy", source.name));
        let mut copy = source.clone();
        copy.name = name;
        copy.color = Some(hex_color(auto_color(self.layers.len())));
        self.layers.insert(i + 1, copy);
        let new = i + 1;
        let original = i;
        for j in 0..self.layers.len() {
            if j == new {
                continue;
            }
            let on = self.collides(original, j);
            self.set_pair(new, j, on);
        }
        // Whether the copy meets itself and the original follows the original.
        let self_on = self.collides(original, original);
        self.set_pair(new, new, self_on);
        let twin = self.collides(original, original);
        self.set_pair(new, original, twin);
        Some(new)
    }

    /// Move layer `from` to position `to` (renumbering the layers between).
    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layers.len() || to >= self.layers.len() || from == to {
            return;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
    }

    /// Number of layers `i` actually collides with (itself included).
    pub fn collision_count(&self, i: usize) -> usize {
        (0..self.layers.len()).filter(|&j| self.collides(i, j)).count()
    }

    /// The group bit of layer `i`.
    pub fn membership_bit(&self, i: usize) -> u32 {
        if i < MAX_LAYERS {
            1 << i
        } else {
            0
        }
    }

    /// The groups layer `i` collides with, as the engine's filter mask.
    pub fn filter_mask(&self, i: usize) -> u32 {
        (0..self.layers.len().min(MAX_LAYERS))
            .filter(|&j| self.collides(i, j))
            .fold(0, |mask, j| mask | (1 << j))
    }

    /// `Default Layer` as `DEFAULT_LAYER`: a Rust identifier for `name`.
    fn const_name(name: &str) -> String {
        let mut out = String::new();
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_uppercase());
            } else if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
        }
        let out = out.trim_end_matches('_').to_string();
        if out.is_empty() {
            "LAYER".to_string()
        } else if out.starts_with(|c: char| c.is_ascii_digit()) {
            format!("_{out}")
        } else {
            out
        }
    }

    /// Rust constants for every layer's group bit and collision mask, to
    /// paste where gameplay code builds collision filters.
    pub fn to_rust_consts(&self) -> String {
        let mut used: Vec<String> = Vec::new();
        let names: Vec<String> = self
            .layers
            .iter()
            .map(|layer| {
                let base = Self::const_name(&layer.name);
                let mut name = base.clone();
                let mut n = 2;
                while used.contains(&name) {
                    name = format!("{base}_{n}");
                    n += 1;
                }
                used.push(name.clone());
                name
            })
            .collect();

        let mut out = String::from("// Collision layers: a layer's bit is its position in the list.\n");
        for (i, name) in names.iter().enumerate().take(MAX_LAYERS) {
            out.push_str(&format!("pub const LAYER_{name}: u32 = 1 << {i};\n"));
        }
        out.push_str("\n// What each layer collides with.\n");
        for (i, name) in names.iter().enumerate().take(MAX_LAYERS) {
            out.push_str(&format!("pub const MASK_{name}: u32 = 0x{:08X};\n", self.filter_mask(i)));
        }
        out
    }

    /// The layers, their bits and masks as an aligned text table.
    pub fn masks_table(&self) -> String {
        let width = self.layers.iter().map(|l| l.name.chars().count()).max().unwrap_or(5).max(5);
        let mut out = format!("{:<4} {:<width$}  {:<10}  {}\n", "Bit", "Layer", "Group", "Collides with");
        for (i, layer) in self.layers.iter().enumerate().take(MAX_LAYERS) {
            out.push_str(&format!(
                "{:<4} {:<width$}  0x{:08X}  0x{:08X}\n",
                i,
                layer.name,
                self.membership_bit(i),
                self.filter_mask(i),
            ));
        }
        out
    }

    /// The color to draw layer `i` with: its own, or one derived from its index.
    pub fn color_of(&self, i: usize) -> [u8; 3] {
        self.layers
            .get(i)
            .and_then(|l| l.color.as_deref())
            .and_then(parse_hex_color)
            .unwrap_or_else(|| auto_color(i))
    }

    pub fn validate(&self) -> Vec<LayerIssue> {
        let mut out: Vec<LayerIssue> = Vec::new();

        if self.layers.len() > MAX_LAYERS {
            out.push(
                Issue::new(
                    Severity::Error,
                    "layers",
                    format!(
                        "{} layers, but the engine's collision groups hold {MAX_LAYERS}: the rest are ignored",
                        self.layers.len()
                    ),
                )
                .with_fix(LayerFix::TrimToLimit),
            );
        }

        for (i, layer) in self.layers.iter().enumerate() {
            let subject = if layer.name.is_empty() { format!("layer {}", i + 1) } else { layer.name.clone() };

            if layer.name.trim().is_empty() {
                out.push(
                    Issue::new(Severity::Error, subject.clone(), "A layer has no name")
                        .with_fix(LayerFix::Rename { index: i }),
                );
            } else if self.layers[..i].iter().any(|l| l.name.eq_ignore_ascii_case(&layer.name)) {
                out.push(
                    Issue::new(
                        Severity::Error,
                        subject.clone(),
                        format!("The name \"{}\" is used by another layer", layer.name),
                    )
                    .with_fix(LayerFix::Rename { index: i }),
                );
            }

            let mut seen: Vec<&str> = Vec::new();
            let mut repeated = false;
            for entry in &layer.collides_with {
                if seen.contains(&entry.as_str()) {
                    repeated = true;
                } else {
                    seen.push(entry);
                }
                if self.index_of(entry).is_none() {
                    out.push(
                        Issue::new(
                            Severity::Warning,
                            subject.clone(),
                            format!("Collides with \"{entry}\", which doesn't exist"),
                        )
                        .with_fix(LayerFix::RemoveDanglingReference {
                            layer: layer.name.clone(),
                            reference: entry.clone(),
                        }),
                    );
                }
            }
            if repeated {
                out.push(
                    Issue::new(Severity::Info, subject.clone(), "Lists the same layer more than once")
                        .with_fix(LayerFix::Deduplicate { layer: layer.name.clone() }),
                );
            }

            if i < MAX_LAYERS && self.collision_count(i) == 0 {
                out.push(Issue::new(
                    Severity::Info,
                    subject,
                    "Collides with nothing: objects on this layer pass through everything",
                ));
            }
        }

        for i in 0..self.layers.len() {
            for j in (i + 1)..self.layers.len() {
                if self.is_one_sided(i, j) {
                    let (a, b) = (self.layers[i].name.clone(), self.layers[j].name.clone());
                    let (lister, other) = if self.lists(i, j) { (&a, &b) } else { (&b, &a) };
                    out.push(
                        Issue::new(
                            Severity::Warning,
                            a.clone(),
                            format!(
                                "\"{lister}\" lists \"{other}\" but \"{other}\" doesn't list it back, so they don't collide"
                            ),
                        )
                        .with_fix(LayerFix::SymmetrizePair { a, b, collide: false }),
                    );
                }
            }
        }

        if self.index_of("Default").is_none() {
            out.push(
                Issue::new(Severity::Warning, "Default", "No \"Default\" layer: objects without a layer have nowhere to go")
                    .with_fix(LayerFix::AddDefaultLayer),
            );
        }
        out
    }

    /// Apply a repair from [`Self::validate`].
    pub fn apply_fix(&mut self, fix: &LayerFix) {
        match fix {
            LayerFix::SymmetrizePair { a, b, collide } => {
                if let (Some(i), Some(j)) = (self.index_of(a), self.index_of(b)) {
                    self.set_pair(i, j, *collide);
                }
            }
            LayerFix::RemoveDanglingReference { layer, reference } => {
                if let Some(i) = self.index_of(layer) {
                    self.layers[i].collides_with.retain(|n| n != reference);
                }
            }
            LayerFix::Rename { index } => {
                if *index < self.layers.len() {
                    self.layers[*index].name.clear();
                    let name = self.unique_name("Layer");
                    // The old name is the empty string or a duplicate; nothing
                    // refers to it unambiguously, so a plain assignment is right.
                    self.layers[*index].name = name;
                }
            }
            LayerFix::Deduplicate { layer } => {
                if let Some(i) = self.index_of(layer) {
                    let mut seen = Vec::new();
                    self.layers[i].collides_with.retain(|n| {
                        if seen.contains(n) {
                            false
                        } else {
                            seen.push(n.clone());
                            true
                        }
                    });
                }
            }
            LayerFix::AddDefaultLayer => {
                if self.layers.len() < MAX_LAYERS {
                    self.add(Some("Default"));
                }
            }
            LayerFix::TrimToLimit => {
                while self.layers.len() > MAX_LAYERS {
                    self.remove(self.layers.len() - 1);
                }
            }
        }
    }
}

// ── File format ────────────────────────────────────────────────────────────

impl Asset for CollisionLayers {
    const KIND: &'static str = "collision layers";

    fn new_default() -> Self {
        Self::default()
    }

    fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        if text.trim().is_empty() {
            return Ok((Self::default(), Vec::new()));
        }
        let value: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
        let Value::Object(map) = value else {
            return Err("expected a JSON object".to_string());
        };

        let mut notes = Vec::new();
        let layers_value = map.get("layers");
        let mut layers = Vec::new();
        match layers_value {
            None => notes.push("no `layers` list; starting with the Default layer".to_string()),
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let Value::Object(entry) = item else {
                        notes.push(format!("layer {} isn't an object; skipped", i + 1));
                        continue;
                    };
                    let name = match entry.get("name") {
                        Some(Value::String(name)) => name.clone(),
                        _ => {
                            notes.push(format!("layer {} has no name", i + 1));
                            String::new()
                        }
                    };
                    let mut collides_with = Vec::new();
                    match entry.get("collides_with") {
                        None => {}
                        Some(Value::Array(list)) => {
                            for entry in list {
                                match entry {
                                    Value::String(s) => collides_with.push(s.clone()),
                                    other => notes.push(format!(
                                        "`collides_with` of \"{name}\" has {other}, which isn't a layer name; skipped"
                                    )),
                                }
                            }
                        }
                        Some(other) => notes.push(format!(
                            "`collides_with` of \"{name}\" should be a list, found {other}"
                        )),
                    }
                    layers.push(Layer {
                        name,
                        collides_with,
                        color: entry.get("color").and_then(Value::as_str).map(str::to_string),
                        description: entry
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        extra: entry
                            .iter()
                            .filter(|(k, _)| !["name", "collides_with", "color", "description"].contains(&k.as_str()))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    });
                }
            }
            Some(other) => return Err(format!("`layers` should be a list, found {other}")),
        }
        if layers.is_empty() && layers_value.is_none() {
            layers = Self::default().layers;
        }

        let parsed = Self {
            version: map.get("version").and_then(Value::as_u64).map_or(SCHEMA_VERSION, |v| v as u32),
            layers,
            extra: map
                .iter()
                .filter(|(k, _)| !["version", "layers"].contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        Ok((parsed, notes))
    }

    fn to_text(&self) -> String {
        let layers: Vec<Value> = self
            .layers
            .iter()
            .map(|layer| {
                let mut entry = Map::new();
                entry.insert("name".into(), json!(layer.name));
                entry.insert("collides_with".into(), json!(layer.collides_with));
                if let Some(color) = &layer.color {
                    entry.insert("color".into(), json!(color));
                }
                if !layer.description.is_empty() {
                    entry.insert("description".into(), json!(layer.description));
                }
                for (k, v) in &layer.extra {
                    entry.entry(k.clone()).or_insert_with(|| v.clone());
                }
                Value::Object(entry)
            })
            .collect();
        let mut map = Map::new();
        map.insert("version".into(), json!(self.version));
        map.insert("layers".into(), Value::Array(layers));
        for (k, v) in &self.extra {
            map.entry(k.clone()).or_insert_with(|| v.clone());
        }
        let mut text = serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default();
        text.push('\n');
        text
    }
}

// ── Presets ────────────────────────────────────────────────────────────────

/// A ready-made layer setup for a common kind of game.
pub struct LayerPreset {
    pub name: &'static str,
    pub description: &'static str,
    /// `(layer, the layers it collides with)`. Every pairing is applied on
    /// both sides, so the result is symmetric by construction.
    pub rows: &'static [(&'static str, &'static [&'static str])],
}

impl LayerPreset {
    pub fn build(&self) -> CollisionLayers {
        let mut set = CollisionLayers { version: SCHEMA_VERSION, layers: Vec::new(), extra: Map::new() };
        for (i, (name, _)) in self.rows.iter().enumerate() {
            let mut layer = Layer::new(*name);
            layer.color = Some(hex_color(auto_color(i)));
            set.layers.push(layer);
        }
        for (name, partners) in self.rows {
            let Some(a) = set.index_of(name) else { continue };
            for partner in *partners {
                if let Some(b) = set.index_of(partner) {
                    set.set_pair(a, b, true);
                }
            }
        }
        set
    }
}

pub const PRESETS: &[LayerPreset] = &[
    LayerPreset {
        name: "Single layer",
        description: "Everything on Default collides with everything on Default.",
        rows: &[("Default", &["Default"])],
    },
    LayerPreset {
        name: "Shooter",
        description: "Player and enemy projectiles that ignore their own side, pickups and triggers that only the player can touch.",
        rows: &[
            ("Default", &["Default", "Environment", "Player", "Enemy"]),
            ("Environment", &["Player", "Enemy", "PlayerProjectile", "EnemyProjectile", "Pickup", "Debris"]),
            ("Player", &["Enemy", "EnemyProjectile", "Pickup", "Trigger"]),
            ("Enemy", &["Enemy", "PlayerProjectile", "Trigger"]),
            ("PlayerProjectile", &[]),
            ("EnemyProjectile", &[]),
            ("Pickup", &[]),
            ("Trigger", &[]),
            ("Debris", &["Debris"]),
        ],
    },
    LayerPreset {
        name: "Platformer",
        description: "Characters stand on platforms, hazards and collectibles are sensors.",
        rows: &[
            ("Default", &["Default", "Platform", "Player", "Enemy"]),
            ("Platform", &["Player", "Enemy", "Collectible"]),
            ("Player", &["Enemy", "Hazard", "Collectible", "Trigger"]),
            ("Enemy", &["Hazard", "Enemy"]),
            ("Hazard", &[]),
            ("Collectible", &[]),
            ("Trigger", &[]),
        ],
    },
    LayerPreset {
        name: "Vehicles",
        description: "Wheels touch only the terrain, so a car's body and wheels never fight each other.",
        rows: &[
            ("Default", &["Default", "Terrain", "Props", "Vehicle"]),
            ("Terrain", &["Wheel", "Vehicle", "Props", "Pedestrian"]),
            ("Vehicle", &["Vehicle", "Props", "Pedestrian"]),
            ("Wheel", &[]),
            ("Props", &["Props", "Pedestrian"]),
            ("Pedestrian", &["Pedestrian"]),
            ("Trigger", &["Vehicle", "Pedestrian"]),
        ],
    },
    LayerPreset {
        name: "Top-down RPG",
        description: "Walls block everyone, NPCs can overlap each other, interaction volumes only react to the player.",
        rows: &[
            ("Default", &["Default", "Wall", "Player", "NPC"]),
            ("Wall", &["Player", "NPC", "Projectile"]),
            ("Player", &["NPC", "Interactable"]),
            ("NPC", &["Projectile"]),
            ("Projectile", &[]),
            ("Interactable", &[]),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> CollisionLayers {
        let mut set = CollisionLayers::default();
        set.add(Some("Player"));
        set.add(Some("Enemy"));
        set.collide_none();
        set
    }

    #[test]
    fn pairs_collide_only_when_both_sides_agree() {
        let mut set = three();
        let (d, p) = (0, 1);
        assert!(!set.collides(d, p));
        set.set_directed(d, p, true);
        assert!(set.lists(d, p));
        assert!(!set.collides(d, p), "one side isn't enough");
        assert!(set.is_one_sided(d, p));
        set.set_pair(d, p, true);
        assert!(set.collides(d, p) && set.collides(p, d));
        assert!(!set.is_one_sided(d, p));
        set.toggle_pair(d, p);
        assert!(!set.lists(d, p) && !set.lists(p, d));
        set.set_pair(p, p, true);
        assert!(set.collides(p, p), "a layer can collide with itself");
    }

    #[test]
    fn adding_a_layer_collides_with_everything_and_names_stay_unique() {
        let mut set = CollisionLayers::default();
        let a = set.add(Some("Props")).unwrap();
        let b = set.add(Some("Props")).unwrap();
        assert_ne!(set.layers[a].name, set.layers[b].name);
        assert_eq!(set.layers[b].name, "Props 2");
        for i in 0..set.len() {
            assert_eq!(set.collision_count(i), set.len(), "layer {i} should meet everything");
        }
    }

    #[test]
    fn the_limit_is_thirty_two() {
        let mut set = CollisionLayers::default();
        while set.add(None).is_some() {}
        assert_eq!(set.len(), MAX_LAYERS);
        assert!(set.add(Some("One too many")).is_none());
        assert!(set.duplicate(0).is_none());
        assert!(set.validate().iter().all(|i| i.severity != Severity::Error));
        set.layers.push(Layer::new("Overflow"));
        assert!(set.validate().iter().any(|i| i.fix == Some(LayerFix::TrimToLimit)));
        set.apply_fix(&LayerFix::TrimToLimit);
        assert_eq!(set.len(), MAX_LAYERS);
    }

    #[test]
    fn rename_updates_references_and_rejects_clashes() {
        let mut set = three();
        set.set_pair(1, 2, true);
        assert_eq!(set.rename(1, "Hero"), Ok(()));
        assert!(set.collides(set.index_of("Hero").unwrap(), set.index_of("Enemy").unwrap()));
        assert_eq!(set.rename(2, "hero"), Err(RenameError::Duplicate), "names clash ignoring case");
        assert_eq!(set.rename(2, "   "), Err(RenameError::Empty));
        assert_eq!(set.rename(2, "Enemy"), Ok(()), "renaming to itself is fine");
    }

    #[test]
    fn removing_a_layer_cleans_every_reference() {
        let mut set = three();
        set.set_pair(0, 2, true);
        set.remove(2);
        assert_eq!(set.len(), 2);
        assert!(set.layers.iter().all(|l| !l.collides_with.iter().any(|n| n == "Enemy")));
        assert!(set.validate().iter().all(|i| !i.message.contains("doesn't exist")));
    }

    #[test]
    fn duplicating_copies_the_row() {
        let mut set = three();
        set.set_pair(1, 2, true);
        set.set_pair(1, 1, true);
        let copy = set.duplicate(1).unwrap();
        assert_eq!(copy, 2);
        assert_eq!(set.layers[copy].name, "Player Copy");
        let enemy = set.index_of("Enemy").unwrap();
        assert!(set.collides(copy, enemy), "the copy meets what the original meets");
        assert!(set.collides(copy, copy));
        assert!(set.collides(copy, 1), "and the original");
        assert!(set.validate().iter().all(|i| i.severity != Severity::Warning), "{:?}", set.validate());
    }

    #[test]
    fn reordering_renumbers_but_keeps_relationships() {
        let mut set = three();
        set.set_pair(1, 2, true);
        set.move_layer(2, 0);
        assert_eq!(set.layers[0].name, "Enemy");
        let (p, e) = (set.index_of("Player").unwrap(), set.index_of("Enemy").unwrap());
        assert!(set.collides(p, e));
        assert_eq!(set.membership_bit(e), 1);
        assert_eq!(set.filter_mask(e), 1 << p);
    }

    #[test]
    fn masks_report_what_the_engine_will_see() {
        let mut set = three();
        set.set_pair(0, 0, true);
        set.set_pair(0, 1, true);
        assert_eq!(set.membership_bit(1), 0b10);
        assert_eq!(set.filter_mask(0), 0b011);
        assert_eq!(set.filter_mask(1), 0b001);
        assert_eq!(set.filter_mask(2), 0);
    }

    #[test]
    fn row_operations_stay_symmetric() {
        let mut set = three();
        set.apply_row(1, RowOp::CollideWithAll);
        assert_eq!(set.collision_count(1), 3);
        assert!(set.collides(0, 1) && set.collides(2, 1));
        set.apply_row(1, RowOp::Invert);
        assert_eq!(set.collision_count(1), 0);
        set.apply_row(1, RowOp::CollideWithNone);
        assert!(set.validate().iter().all(|i| i.severity != Severity::Warning));
    }

    #[test]
    fn one_sided_entries_are_reported_and_resolved_either_way() {
        let mut set = three();
        set.set_directed(1, 2, true);
        let issues = set.validate();
        let issue = issues.iter().find(|i| i.message.contains("doesn't list it back")).unwrap();
        assert_eq!(issue.severity, Severity::Warning);

        let mut union = set.clone();
        union.make_symmetric(Symmetry::Union);
        assert!(union.collides(1, 2));
        let mut inter = set.clone();
        inter.make_symmetric(Symmetry::Intersection);
        assert!(!inter.lists(1, 2));

        set.apply_fix(issue.fix.as_ref().unwrap());
        assert!(set.validate().iter().all(|i| !i.message.contains("doesn't list it back")));
    }

    #[test]
    fn validation_catches_names_and_references_and_the_fixes_work() {
        let text = r#"{ "layers": [
            { "name": "Default", "collides_with": ["Default", "Ghost", "Default"] },
            { "name": "", "collides_with": [] },
            { "name": "default", "collides_with": [] }
        ] }"#;
        let (mut set, _) = CollisionLayers::parse(text).unwrap();
        let issues = set.validate();
        assert!(issues.iter().any(|i| i.message.contains("Ghost")));
        assert!(issues.iter().any(|i| i.message.contains("no name")));
        assert!(issues.iter().any(|i| i.message.contains("used by another")));
        assert!(issues.iter().any(|i| i.message.contains("more than once")));
        for _ in 0..4 {
            for issue in set.validate() {
                if let Some(fix) = &issue.fix {
                    set.apply_fix(fix);
                }
            }
        }
        let left: Vec<_> = set.validate().into_iter().filter(|i| i.severity != Severity::Info).collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_missing_default_layer_can_be_added() {
        let mut set = CollisionLayers { version: 1, layers: vec![Layer::new("Only")], extra: Map::new() };
        let issue = set.validate().into_iter().find(|i| i.fix == Some(LayerFix::AddDefaultLayer)).unwrap();
        set.apply_fix(issue.fix.as_ref().unwrap());
        assert!(set.index_of("Default").is_some());
    }

    #[test]
    fn the_template_file_parses_to_the_default_asset() {
        let template = &crate::file_types::file_types()[1].default_content;
        let (set, notes) = CollisionLayers::parse(&template.to_string()).unwrap();
        assert!(notes.is_empty());
        assert_eq!(set, CollisionLayers::default());
        let (again, _) = CollisionLayers::parse(&set.to_text()).unwrap();
        assert_eq!(again, set);
    }

    #[test]
    fn unknown_fields_and_bad_entries_are_handled() {
        let text = r##"{ "version": 2, "layers": [
            { "name": "A", "collides_with": ["A", 7], "color": "#ff0000", "tag": "keep" }, 5 ],
          "future": true }"##;
        let (set, notes) = CollisionLayers::parse(text).unwrap();
        assert_eq!(set.len(), 1);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert_eq!(set.version, 2);
        let saved: Value = serde_json::from_str(&set.to_text()).unwrap();
        assert_eq!(saved["future"], true);
        assert_eq!(saved["layers"][0]["tag"], "keep");
        assert_eq!(saved["layers"][0]["color"], "#ff0000");
        assert!(CollisionLayers::parse("{ \"layers\": 3 }").is_err());
        assert!(CollisionLayers::parse("nope").is_err());
        assert_eq!(CollisionLayers::parse("").unwrap().0, CollisionLayers::default());
    }

    #[test]
    fn every_preset_builds_a_clean_symmetric_setup() {
        for preset in PRESETS {
            let set = preset.build();
            assert!(set.len() <= MAX_LAYERS, "{}", preset.name);
            assert!(set.index_of("Default").is_some(), "{} lacks Default", preset.name);
            let issues = set.validate();
            assert!(
                issues.iter().all(|i| i.severity != Severity::Error && i.severity != Severity::Warning),
                "{}: {issues:?}",
                preset.name
            );
            for (name, partners) in preset.rows {
                let a = set.index_of(name).unwrap();
                for partner in *partners {
                    let b = set.index_of(partner).unwrap_or_else(|| panic!("{}: unknown layer {partner}", preset.name));
                    assert!(set.collides(a, b), "{}: {name} should meet {partner}", preset.name);
                }
            }
        }
    }

    #[test]
    fn rust_constants_name_each_layer_and_its_mask() {
        let mut set = CollisionLayers::default();
        set.add(Some("Player Projectile"));
        set.add(Some("player-projectile"));
        set.add(Some("9 lives"));
        set.collide_none();
        set.set_pair(0, 1, true);
        let text = set.to_rust_consts();
        assert!(text.contains("pub const LAYER_DEFAULT: u32 = 1 << 0;"), "{text}");
        assert!(text.contains("pub const LAYER_PLAYER_PROJECTILE: u32 = 1 << 1;"));
        assert!(text.contains("LAYER_PLAYER_PROJECTILE_2"), "clashing names are numbered");
        assert!(text.contains("LAYER__9_LIVES") || text.contains("LAYER_9_LIVES") || text.contains("_9_LIVES"));
        assert!(text.contains("pub const MASK_DEFAULT: u32 = 0x00000002;"), "{text}");
        let table = set.masks_table();
        assert!(table.contains("0x00000001") && table.lines().count() == 5, "{table}");
    }

    #[test]
    fn shooter_projectiles_ignore_their_own_side() {
        let set = PRESETS.iter().find(|p| p.name == "Shooter").unwrap().build();
        let ix = |n: &str| set.index_of(n).unwrap();
        assert!(!set.collides(ix("PlayerProjectile"), ix("Player")));
        assert!(set.collides(ix("PlayerProjectile"), ix("Enemy")));
        assert!(set.collides(ix("EnemyProjectile"), ix("Player")));
        assert!(!set.collides(ix("Pickup"), ix("Enemy")));
    }
}
