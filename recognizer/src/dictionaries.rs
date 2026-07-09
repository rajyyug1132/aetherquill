//! Loads the real sigil/sign dictionaries — the JSON is vendored unmodified
//! from service/vendor/wha/src/dictionary/ and embedded at compile time so
//! the recognizer crate has no filesystem dependency on-device.

use crate::geometry::Point;
use crate::layer_mapper::Layer;
use crate::symbol_recognizer::{Dictionary, DictionaryEntry, SemanticFields, StrokeTemplate};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawPoint {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct RawStrokeTemplate {
    // sourceAspectRatio isn't read by any ported logic — recognition works
    // entirely off the resampled/normalized points (templateNormalizer.rs).
    strokes: Vec<Vec<RawPoint>>,
}

#[derive(Deserialize, Default)]
struct RawSemantic {
    manifestation: Option<String>,
    #[serde(rename = "directionMode")]
    direction_mode: Option<String>,
    force: Option<f64>,
    focus: Option<f64>,
    spread: Option<f64>,
    range: Option<f64>,
    #[serde(rename = "lifetimeBias")]
    lifetime_bias: Option<f64>,
}

#[derive(Deserialize)]
struct RawEntry {
    id: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    element: Option<String>,
    semantic: Option<RawSemantic>,
    #[serde(rename = "allowedLayers")]
    allowed_layers: Option<Vec<String>>,
    #[serde(rename = "strokeTemplate")]
    stroke_template: Option<RawStrokeTemplate>,
    #[serde(rename = "recognitionRotationInvariant")]
    recognition_rotation_invariant: Option<bool>,
    #[serde(rename = "allowedRotationsDeg")]
    allowed_rotations_deg: Option<Vec<f64>>,
}

impl From<RawEntry> for DictionaryEntry {
    fn from(raw: RawEntry) -> Self {
        DictionaryEntry {
            id: raw.id,
            display_name: raw.display_name,
            element: raw.element,
            semantic: raw.semantic.map(|s| SemanticFields {
                manifestation: s.manifestation,
                direction_mode: s.direction_mode,
                force: s.force,
                focus: s.focus,
                spread: s.spread,
                range: s.range,
                lifetime_bias: s.lifetime_bias,
            }),
            allowed_layers: raw.allowed_layers.map(|layers| layers.iter().filter_map(|l| Layer::from_str(l)).collect()),
            stroke_template: raw.stroke_template.map(|t| StrokeTemplate {
                strokes: t.strokes.into_iter().map(|stroke| stroke.into_iter().map(|p| Point { x: p.x, y: p.y }).collect()).collect(),
            }),
            recognition_rotation_invariant: raw.recognition_rotation_invariant,
            allowed_rotations_deg: raw.allowed_rotations_deg,
        }
    }
}

const SIGILS_JSON: &str = include_str!("../../service/vendor/wha/src/dictionary/sigils.json");
const SIGNS_JSON: &str = include_str!("../../service/vendor/wha/src/dictionary/signs.json");

pub fn load_dictionary() -> Dictionary {
    let sigils: Vec<RawEntry> = serde_json::from_str(SIGILS_JSON).expect("sigils.json is valid — vendored, checked into the repo");
    let signs: Vec<RawEntry> = serde_json::from_str(SIGNS_JSON).expect("signs.json is valid — vendored, checked into the repo");
    Dictionary { sigils: sigils.into_iter().map(DictionaryEntry::from).collect(), signs: signs.into_iter().map(DictionaryEntry::from).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_five_sigils_and_three_signs() {
        let dictionary = load_dictionary();
        assert_eq!(dictionary.sigils.len(), 5);
        assert_eq!(dictionary.signs.len(), 3);
    }

    #[test]
    fn fire_sigil_has_expected_fields() {
        let dictionary = load_dictionary();
        let fire = dictionary.sigils.iter().find(|e| e.id == "fire").expect("fire sigil present");
        assert_eq!(fire.element.as_deref(), Some("fire"));
        assert_eq!(fire.recognition_rotation_invariant, Some(false));
        assert!(fire.stroke_template.as_ref().unwrap().strokes[0].len() > 10, "fire template should have real stroke points");
        assert_eq!(fire.allowed_layers.as_ref().unwrap(), &[Layer::Center, Layer::Middle, Layer::Outer]);
    }

    #[test]
    fn column_sign_has_manifestation_and_direction_mode() {
        let dictionary = load_dictionary();
        let column = dictionary.signs.iter().find(|e| e.id == "column").expect("column sign present");
        let semantic = column.semantic.as_ref().expect("column has semantic block");
        assert_eq!(semantic.manifestation.as_deref(), Some("column"));
        assert_eq!(semantic.direction_mode.as_deref(), Some("inward"));
        assert_eq!(semantic.force, Some(0.3));
    }
}
