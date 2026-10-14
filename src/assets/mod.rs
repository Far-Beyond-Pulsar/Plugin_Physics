//! The physics asset formats as typed models.
//!
//! These are plain data: parsing, validation, editing operations and the
//! physics the editors preview. They know nothing about GPUI, so everything
//! here is unit-tested. The editors (`crate::editors`) are thin views over
//! them.
//!
//! Both formats keep any field they don't know: a file written by a newer
//! version of the plugin survives a round trip through this one.

pub mod layers;
pub mod material;

use serde::{Deserialize, Serialize};

/// How serious a validation finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Worth knowing, nothing is wrong.
    Info,
    /// Legal, but probably not what was meant.
    Warning,
    /// The engine will reject or misread this.
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
        }
    }
}

/// One validation finding.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue<F> {
    pub severity: Severity,
    /// Which part of the asset it concerns, for grouping and highlighting.
    pub subject: String,
    pub message: String,
    /// A one-click repair, when there is an obvious one.
    pub fix: Option<F>,
}

impl<F> Issue<F> {
    pub fn new(severity: Severity, subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            subject: subject.into(),
            message: message.into(),
            fix: None,
        }
    }

    pub fn with_fix(mut self, fix: F) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// An asset the document layer can load, save and diff.
pub trait Asset: Clone + PartialEq + Send + 'static {
    /// What the file type is called in messages.
    const KIND: &'static str;

    /// A fresh asset, as the "new file" template creates one.
    fn new_default() -> Self;

    /// Parse the file's text, with notes about anything repaired on the way
    /// (an unknown combine mode becoming the default, a field of the wrong
    /// type). `Err` only when the text isn't usable at all.
    fn parse(text: &str) -> Result<(Self, Vec<String>), String>;

    /// The file's text.
    fn to_text(&self) -> String;
}

/// `#rrggbb` as `[r, g, b]` bytes.
pub fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

pub fn hex_color(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// A color for the `index`-th item of a set, spread around the hue wheel by
/// the golden angle so neighbours stay distinct however many there are.
pub fn auto_color(index: usize) -> [u8; 3] {
    let hue = (index as f32 * 0.618_034).fract();
    let (s, l) = (0.62_f32, 0.58_f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue * 6.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_round_trip() {
        assert_eq!(parse_hex_color("#4ecdc4"), Some([0x4e, 0xcd, 0xc4]));
        assert_eq!(hex_color([0x4e, 0xcd, 0xc4]), "#4ecdc4");
        assert_eq!(parse_hex_color("4ecdc4"), None);
        assert_eq!(parse_hex_color("#4ecd"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
    }

    #[test]
    fn auto_colors_are_distinct() {
        let colors: Vec<_> = (0..16).map(auto_color).collect();
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
