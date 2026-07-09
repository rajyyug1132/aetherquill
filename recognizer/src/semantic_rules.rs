//! Direct port of service/vendor/wha/src/compiler/semanticRules.js.
//!
//! Operates on recognized "sign" recognitions (the `signs` list drawing_classifier
//! produces) — reads their geometry/confidence to derive spell-parameter deltas
//! and manifestation groupings.

use crate::geometry::{clamp, clamp_signed, vector_from_angle_deg, Point};
use crate::symbol_recognizer::Recognition;
use std::collections::HashMap;

const INWARD_DIRECTION_OFFSET_DEG: f64 = 180.0;

struct SignShapeTuning;
impl SignShapeTuning {
    const FORCE_IMBALANCE_OFFSET: f64 = 0.08;
    const FORCE_IMBALANCE_SCALE: f64 = 0.34;
    const FORCE_MAX: f64 = 0.18;
    const FOCUS_ELONGATION_OFFSET: f64 = 0.12;
    const FOCUS_ELONGATION_SCALE: f64 = 0.2;
    const FOCUS_MAX: f64 = 0.12;
    const DIRECTION_AXIS_OFFSET: f64 = 0.1;
    const DIRECTION_AXIS_SCALE: f64 = 0.95;
    const DIRECTION_MAX: f64 = 0.58;
}

struct SignInfluenceTuning;
impl SignInfluenceTuning {
    const SIZE_BASE: f64 = 0.68;
    const SIZE_SCALE: f64 = 2.4;
    const SIZE_MIN: f64 = 0.45;
    const SIZE_MAX: f64 = 1.25;
    const LENGTH_BASE: f64 = 0.72;
    const LENGTH_SCALE: f64 = 1.8;
    const LENGTH_MIN: f64 = 0.45;
    const LENGTH_MAX: f64 = 1.22;
    const LAYER_OUTER: f64 = 1.0;
    const LAYER_MIDDLE: f64 = 0.88;
    const LAYER_OTHER: f64 = 0.62;
    const DISTANCE_BASE: f64 = 0.76;
    const DISTANCE_SCALE: f64 = 0.34;
    const DISTANCE_MIN: f64 = 0.58;
    const DISTANCE_MAX: f64 = 1.14;
    const FEATURE_BOOST_BASE: f64 = 1.0;
    const FEATURE_BOOST_MIN: f64 = 0.35;
    const FEATURE_BOOST_MAX: f64 = 1.85;
    const MINIMUM_DIRECTION_MAGNITUDE: f64 = 0.001;
}

struct ConvergenceTuning;
impl ConvergenceTuning {
    const POINT_SCALE: f64 = 0.42;
    const POINT_LIMIT: f64 = 0.5;
    const RADIUS_BASE: f64 = 0.3;
    const RADIUS_STRENGTH_SCALE: f64 = 0.16;
    const RADIUS_SIZE_SCALE: f64 = 0.42;
    const RADIUS_INNER_BIAS_SCALE: f64 = 0.04;
    const RADIUS_MIN: f64 = 0.06;
    const RADIUS_MAX: f64 = 0.3;
    const RIGIDITY_BASE: f64 = 0.58;
    const RIGIDITY_SIZE_SCALE: f64 = 2.1;
    const RIGIDITY_RADIUS_SCALE: f64 = 0.18;
}

fn manifestation_id(sign: &Recognition) -> String {
    sign.semantic
        .as_ref()
        .and_then(|s| s.manifestation.clone())
        .unwrap_or_else(|| sign.id.clone().unwrap_or_default())
}

fn sign_direction(sign: &Recognition) -> Point {
    match sign.semantic.as_ref().and_then(|s| s.direction_mode.as_deref()) {
        Some("orientation") => vector_from_angle_deg(sign.directed_orientation_deg),
        Some("inward") => vector_from_angle_deg(sign.angle_deg + INWARD_DIRECTION_OFFSET_DEG),
        _ => vector_from_angle_deg(sign.angle_deg),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SemanticDeltas {
    pub force: f64,
    pub focus: f64,
    pub spread: f64,
    pub range: f64,
    pub lifetime_bias: f64,
}

struct ShapeDeltas {
    deltas: SemanticDeltas,
    direction_weight: f64,
}

fn sign_shape_deltas(sign: &Recognition) -> ShapeDeltas {
    let axis_dominance = sign.shape.axis_dominance;
    let stroke_length_imbalance = sign.shape.stroke_length_imbalance;
    let elongation_norm = sign.shape.elongation_norm;

    ShapeDeltas {
        deltas: SemanticDeltas {
            force: clamp((stroke_length_imbalance - SignShapeTuning::FORCE_IMBALANCE_OFFSET) * SignShapeTuning::FORCE_IMBALANCE_SCALE, 0.0, SignShapeTuning::FORCE_MAX),
            focus: clamp((elongation_norm - SignShapeTuning::FOCUS_ELONGATION_OFFSET) * SignShapeTuning::FOCUS_ELONGATION_SCALE, 0.0, SignShapeTuning::FOCUS_MAX),
            spread: 0.0,
            range: 0.0,
            lifetime_bias: 0.0,
        },
        direction_weight: clamp((axis_dominance - SignShapeTuning::DIRECTION_AXIS_OFFSET) * SignShapeTuning::DIRECTION_AXIS_SCALE, 0.0, SignShapeTuning::DIRECTION_MAX),
    }
}

pub fn sign_influence(sign: &Recognition) -> f64 {
    let size_weight = clamp(SignInfluenceTuning::SIZE_BASE + sign.size_norm * SignInfluenceTuning::SIZE_SCALE, SignInfluenceTuning::SIZE_MIN, SignInfluenceTuning::SIZE_MAX);
    let length_weight = clamp(SignInfluenceTuning::LENGTH_BASE + sign.length_norm * SignInfluenceTuning::LENGTH_SCALE, SignInfluenceTuning::LENGTH_MIN, SignInfluenceTuning::LENGTH_MAX);
    let layer_weight = match sign.layer.as_str() {
        "outer" => SignInfluenceTuning::LAYER_OUTER,
        "middle" => SignInfluenceTuning::LAYER_MIDDLE,
        _ => SignInfluenceTuning::LAYER_OTHER,
    };
    let distance_weight = clamp(SignInfluenceTuning::DISTANCE_BASE + sign.radius_norm * SignInfluenceTuning::DISTANCE_SCALE, SignInfluenceTuning::DISTANCE_MIN, SignInfluenceTuning::DISTANCE_MAX);
    clamp(sign.confidence * sign.neatness * size_weight * length_weight * layer_weight * distance_weight, 0.0, 1.0)
}

struct WeightedSign<'a> {
    sign: &'a Recognition,
    influence: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ConvergenceProfile {
    pub point: Point,
    pub radius: f64,
    pub rigidity: f64,
}

fn convergence_profile(weighted_signs: &[WeightedSign], strength: f64) -> ConvergenceProfile {
    let total_influence: f64 = weighted_signs.iter().map(|w| w.influence).sum();
    if total_influence == 0.0 {
        return ConvergenceProfile { point: Point { x: 0.0, y: 0.0 }, radius: ConvergenceTuning::RADIUS_MAX, rigidity: 0.0 };
    }

    let (mut sx, mut sy, mut placement_weight, mut size_sum, mut radius_sum) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for w in weighted_signs {
        let radial = clamp(w.sign.radius_norm, 0.0, 1.0);
        let direction = vector_from_angle_deg(w.sign.angle_deg);
        let size = w.sign.size_norm;
        let weight = w.influence * (0.7 + radial * 0.45);

        sx += direction.x * radial * weight;
        sy += direction.y * radial * weight;
        placement_weight += weight;
        size_sum += size * w.influence;
        radius_sum += radial * w.influence;
    }

    let average_size = size_sum / total_influence;
    let average_radius = radius_sum / total_influence;
    let x = if placement_weight > 0.0 { (sx / placement_weight) * ConvergenceTuning::POINT_SCALE } else { 0.0 };
    let y = if placement_weight > 0.0 { (sy / placement_weight) * ConvergenceTuning::POINT_SCALE } else { 0.0 };

    ConvergenceProfile {
        point: Point { x: clamp_signed(x, ConvergenceTuning::POINT_LIMIT), y: clamp_signed(y, ConvergenceTuning::POINT_LIMIT) },
        radius: clamp(
            ConvergenceTuning::RADIUS_BASE - strength * ConvergenceTuning::RADIUS_STRENGTH_SCALE - average_size * ConvergenceTuning::RADIUS_SIZE_SCALE
                + (1.0 - average_radius) * ConvergenceTuning::RADIUS_INNER_BIAS_SCALE,
            ConvergenceTuning::RADIUS_MIN,
            ConvergenceTuning::RADIUS_MAX,
        ),
        rigidity: clamp(strength * (ConvergenceTuning::RIGIDITY_BASE + average_size * ConvergenceTuning::RIGIDITY_SIZE_SCALE + average_radius * ConvergenceTuning::RIGIDITY_RADIUS_SCALE), 0.0, 1.0),
    }
}

fn sign_direction_weight(sign: &Recognition) -> f64 {
    let feature_deltas = sign_shape_deltas(sign);
    sign_influence(sign)
        * clamp(
            SignInfluenceTuning::FEATURE_BOOST_BASE + feature_deltas.direction_weight,
            SignInfluenceTuning::FEATURE_BOOST_MIN,
            SignInfluenceTuning::FEATURE_BOOST_MAX,
        )
}

#[derive(Debug, Clone)]
pub struct Manifestation {
    pub strength: f64,
    pub convergence: Option<ConvergenceProfile>,
}

#[derive(Debug, Clone)]
pub struct ManifestationAggregate {
    pub primary_manifestation: String,
    pub manifestations: HashMap<String, Manifestation>,
    pub manifestation_influence: HashMap<String, f64>,
}

pub fn aggregate_manifestations(signs: &[Recognition]) -> ManifestationAggregate {
    if signs.is_empty() {
        let mut manifestations = HashMap::new();
        manifestations.insert("aura".to_string(), Manifestation { strength: 1.0, convergence: None });
        let mut influence = HashMap::new();
        influence.insert("aura".to_string(), 0.0);
        return ManifestationAggregate { primary_manifestation: "aura".to_string(), manifestations, manifestation_influence: influence };
    }

    struct Group<'a> {
        id: String,
        total_influence: f64,
        signs: Vec<WeightedSign<'a>>,
    }

    let mut groups: HashMap<String, Group> = HashMap::new();
    for sign in signs {
        let id = manifestation_id(sign);
        let influence = sign_influence(sign);
        let group = groups.entry(id.clone()).or_insert_with(|| Group { id: id.clone(), total_influence: 0.0, signs: vec![] });
        group.total_influence += influence;
        group.signs.push(WeightedSign { sign, influence });
    }

    let mut sorted_groups: Vec<Group> = groups.into_values().collect();
    sorted_groups.sort_by(|a, b| b.total_influence.partial_cmp(&a.total_influence).unwrap());

    let mut manifestations = HashMap::new();
    let mut manifestation_influence = HashMap::new();
    for group in &sorted_groups {
        let strength = clamp(group.total_influence, 0.0, 1.0);
        let convergence = if group.id == "convergence" { Some(convergence_profile(&group.signs, strength)) } else { None };
        manifestations.insert(group.id.clone(), Manifestation { strength, convergence });
        manifestation_influence.insert(group.id.clone(), group.total_influence);
    }

    ManifestationAggregate {
        primary_manifestation: sorted_groups.first().map(|g| g.id.clone()).unwrap_or_else(|| "aura".to_string()),
        manifestations,
        manifestation_influence,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceDirection {
    pub x: f64,
    pub y: f64,
    pub strength: f64,
}

pub fn combine_sign_direction(signs: &[Recognition]) -> SurfaceDirection {
    let (mut vx, mut vy, mut weight) = (0.0, 0.0, 0.0);
    for sign in signs {
        let influence = sign_direction_weight(sign);
        let direction = sign_direction(sign);
        vx += direction.x * influence;
        vy += direction.y * influence;
        weight += influence;
    }

    let magnitude = vx.hypot(vy);
    if magnitude < SignInfluenceTuning::MINIMUM_DIRECTION_MAGNITUDE {
        return SurfaceDirection { x: 0.0, y: 0.0, strength: 0.0 };
    }

    let strength = clamp(magnitude / SignInfluenceTuning::MINIMUM_DIRECTION_MAGNITUDE.max(weight), 0.0, 1.0);
    SurfaceDirection { x: vx / magnitude, y: vy / magnitude, strength }
}

pub fn aggregate_semantic_deltas(signs: &[Recognition]) -> SemanticDeltas {
    let mut sum = SemanticDeltas::default();
    for sign in signs {
        let influence = sign_influence(sign);
        let semantic = sign.semantic.as_ref();
        let feature_deltas = sign_shape_deltas(sign).deltas;
        sum.force += (semantic.and_then(|s| s.force).unwrap_or(0.0) + feature_deltas.force) * influence;
        sum.focus += (semantic.and_then(|s| s.focus).unwrap_or(0.0) + feature_deltas.focus) * influence;
        sum.spread += (semantic.and_then(|s| s.spread).unwrap_or(0.0) + feature_deltas.spread) * influence;
        sum.range += (semantic.and_then(|s| s.range).unwrap_or(0.0) + feature_deltas.range) * influence;
        sum.lifetime_bias += (semantic.and_then(|s| s.lifetime_bias).unwrap_or(0.0) + feature_deltas.lifetime_bias) * influence;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_mapper::Layer;
    use crate::stroke_grouper::RadialFacing;
    use crate::symbol_recognizer::{RecognitionShape, RecognitionStatus};

    fn dummy_sign(id: &str, angle_deg: f64, semantic: Option<crate::symbol_recognizer::SemanticFields>) -> Recognition {
        Recognition {
            candidate_id: "c1".into(),
            layer: Layer::Outer,
            near_boundary: false,
            radius_norm: 0.7,
            angle_deg,
            size_norm: 0.1,
            length_norm: 0.1,
            orientation_deg: 0.0,
            directed_orientation_deg: 0.0,
            radial_facing: RadialFacing::Outward,
            neatness: 0.9,
            recognized: true,
            recognition_status: RecognitionStatus::Valid,
            kind: "sign".into(),
            id: Some(id.to_string()),
            display_name: None,
            element: None,
            semantic,
            confidence: 0.9,
            shape: RecognitionShape { stroke_count: 1, aspect_ratio: 1.0, elongation: 1.0, elongation_norm: 0.0, stroke_length_imbalance: 0.0, axis_dominance: 0.0 },
            diagnostics: crate::symbol_recognizer::RecognitionDiagnostics { best_guess: None, recognition_rotation_deg: 0.0, top_matches: vec![] },
        }
    }

    #[test]
    fn no_signs_yields_aura_manifestation() {
        let result = aggregate_manifestations(&[]);
        assert_eq!(result.primary_manifestation, "aura");
        assert_eq!(result.manifestations["aura"].strength, 1.0);
    }

    #[test]
    fn manifestation_id_falls_back_to_sign_id() {
        let sign = dummy_sign("levitation", 90.0, None);
        assert_eq!(manifestation_id(&sign), "levitation");
    }

    #[test]
    fn manifestation_id_prefers_semantic_manifestation() {
        let semantic = crate::symbol_recognizer::SemanticFields { manifestation: Some("custom-effect".into()), ..Default::default() };
        let sign = dummy_sign("some-sign", 90.0, Some(semantic));
        assert_eq!(manifestation_id(&sign), "custom-effect");
    }

    #[test]
    fn convergence_group_gets_a_profile() {
        let semantic = crate::symbol_recognizer::SemanticFields { manifestation: Some("convergence".into()), ..Default::default() };
        let sign = dummy_sign("convergence-sign", 0.0, Some(semantic));
        let result = aggregate_manifestations(&[sign]);
        assert_eq!(result.primary_manifestation, "convergence");
        assert!(result.manifestations["convergence"].convergence.is_some());
    }

    #[test]
    fn combine_sign_direction_of_no_signs_is_zero_strength() {
        let result = combine_sign_direction(&[]);
        assert_eq!(result.strength, 0.0);
    }

    #[test]
    fn combine_sign_direction_points_toward_position_by_default() {
        // A single sign at angle 0 with default (position) direction mode should
        // produce a rightward-pointing (positive x) surface direction.
        let sign = dummy_sign("directional-sign", 0.0, None);
        let result = combine_sign_direction(&[sign]);
        assert!(result.x > 0.9, "expected strong rightward direction, got x={}", result.x);
        assert!(result.strength > 0.0);
    }

    #[test]
    fn aggregate_semantic_deltas_of_no_signs_is_zero() {
        let deltas = aggregate_semantic_deltas(&[]);
        assert_eq!(deltas.force, 0.0);
        assert_eq!(deltas.focus, 0.0);
    }
}
