//! Direct port of service/vendor/wha/src/parser/symbolRecognizer.js.
//!
//! Recognition scores each grouped symbol candidate against every dictionary
//! sigil and sign:
//! 1. Extract candidate geometry (aspect ratio, elongation, stroke count,
//!    stroke-length profile, neatness).
//! 2. For signs, rotate the candidate into the bottom-of-ring canonical frame
//!    so template matching can compare shape while preserving the original
//!    ring-relative orientation as spell meaning.
//! 3. Rasterize the candidate and dictionary template, test the allowed
//!    rotations, and keep the best ink overlap/coverage measurements.
//! 4. Blend ink score with structural compatibility, layer fit, size fit, and
//!    neatness, then cap obvious incomplete or contaminated matches.
//! 5. Sort all dictionary matches, decide accepted/ambiguous/contaminated/
//!    messy-valid/unknown, and keep the top 3 matches in diagnostics.
//!
//! ponytail: the JS caches per-template features in a WeakMap because the
//! browser UI rescores every animation frame; this pipeline scores once per
//! pen-up against a small (~8 entry) dictionary, so the cache is skipped —
//! same call as template_matcher.rs.

use crate::geometry::{angular_difference, bounds_for_points, clamp, dominant_axis_orientation_deg, normalize_angle_deg, path_length, Point};
use crate::layer_mapper::Layer;
use crate::sign_rotation::{recognition_plan_for_symbol, RecognitionEntry};
use crate::stroke_grouper::{Candidate, RadialFacing};
use crate::template_matcher::{score_stroke_template, TemplateScore};

const RECOGNITION_AMBIGUITY_GAP: f64 = 0.065;
const SIMPLE_SIGN_STROKE_LIMIT: usize = 6;
const SIMPLE_SIGN_MIN_TEMPLATE_COVERAGE: f64 = 0.78;

// --- dictionary shape (real JSON wiring lands in the `dictionaries` task) ---

/// A dictionary entry's `semantic` block. `manifestation`/`direction_mode` are
/// string-valued (read by compiler/semanticRules.js); the rest are numeric
/// deltas defaulted to 0 wherever they're read (JS: `semantic[target] ?? 0`).
#[derive(Debug, Clone, Default)]
pub struct SemanticFields {
    pub manifestation: Option<String>,
    pub direction_mode: Option<String>,
    pub force: Option<f64>,
    pub focus: Option<f64>,
    pub spread: Option<f64>,
    pub range: Option<f64>,
    pub lifetime_bias: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct StrokeTemplate {
    pub strokes: Vec<Vec<Point>>,
}

#[derive(Debug, Clone, Default)]
pub struct DictionaryEntry {
    pub id: String,
    pub display_name: Option<String>,
    pub element: Option<String>,
    pub semantic: Option<SemanticFields>,
    pub allowed_layers: Option<Vec<Layer>>,
    pub stroke_template: Option<StrokeTemplate>,
    pub recognition_rotation_invariant: Option<bool>,
    pub allowed_rotations_deg: Option<Vec<f64>>,
}

pub struct Dictionary {
    pub sigils: Vec<DictionaryEntry>,
    pub signs: Vec<DictionaryEntry>,
}

pub struct RecognitionConfig {
    pub min_confidence: f64,
}

// --- scoring helpers ---

fn allowed_layer_score(entry: &DictionaryEntry, candidate: &Candidate) -> f64 {
    // ponytail: JS also special-cases `candidate.layer === "any"`, but
    // layer_mapper::Layer never produces that value (map_radius_to_layer only
    // returns Center/Middle/Outer/RingBoundary/Outside) — that branch is
    // provably dead code for a real candidate, so it's omitted here.
    match &entry.allowed_layers {
        None => 0.75,
        Some(layers) if layers.is_empty() => 0.75,
        Some(layers) if layers.contains(&candidate.layer) => 1.0,
        _ if candidate.near_boundary => 0.72,
        _ => 0.34,
    }
}

fn range_score(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        clamp(value / (0.001_f64).max(min), 0.0, 1.0)
    } else if value > max {
        clamp(1.0 - (value - max) / (0.001_f64).max(max), 0.0, 1.0)
    } else {
        1.0
    }
}

fn aspect_ratio(width: f64, height: f64) -> f64 {
    (0.001_f64).max(width) / (0.001_f64).max(height)
}

fn rotated_aspect_ratio(ratio: f64, rotation_deg: f64) -> f64 {
    let normalized = ((normalize_angle_deg(rotation_deg) % 180.0) - 90.0).abs();
    let blend = 1.0 - normalized / 90.0;
    let log_ratio = (0.001_f64).max(ratio).ln();
    (log_ratio * (1.0 - blend * 2.0)).exp()
}

fn aspect_compatibility(candidate_ratio: f64, template_ratio: f64, rotation_deg: f64) -> f64 {
    let adjusted = rotated_aspect_ratio(candidate_ratio, rotation_deg);
    let distance = (adjusted / (0.001_f64).max(template_ratio)).ln().abs();
    clamp(1.0 - distance / 1.1, 0.0, 1.0)
}

fn undirected_angular_difference(a: f64, b: f64) -> f64 {
    let difference = angular_difference(a, b);
    difference.min((180.0 - difference).abs())
}

fn stroke_length_profile(point_lists: &[Vec<Point>]) -> Vec<f64> {
    let mut lengths: Vec<f64> = point_lists.iter().map(|pts| path_length(pts)).filter(|&l| l > 0.0001).collect();
    lengths.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let total: f64 = lengths.iter().sum();
    if total == 0.0 {
        return vec![];
    }
    lengths.into_iter().map(|l| l / total).collect()
}

fn profile_compatibility(candidate_profile: &[f64], template_profile: &[f64]) -> f64 {
    let count = candidate_profile.len().max(template_profile.len());
    if count == 0 {
        return 1.0;
    }
    let mut distance = 0.0;
    for index in 0..count {
        let c = candidate_profile.get(index).copied().unwrap_or(0.0);
        let t = template_profile.get(index).copied().unwrap_or(0.0);
        distance += (c - t).abs();
    }
    clamp(1.0 - distance / 1.4, 0.0, 1.0)
}

fn stroke_count_compatibility(candidate_count: usize, template_count: usize) -> f64 {
    if candidate_count == 0 || template_count == 0 {
        return 0.0;
    }
    clamp(1.0 - (candidate_count as f64 - template_count as f64).abs() / (candidate_count.max(template_count) as f64), 0.0, 1.0)
}

struct TemplateFeatures {
    aspect_ratio: f64,
    stroke_count: usize,
    orientation_deg: f64,
    stroke_profile: Vec<f64>,
}

fn template_features(stroke_template: &StrokeTemplate) -> TemplateFeatures {
    let points: Vec<Point> = stroke_template.strokes.iter().flatten().copied().collect();
    let bounds = bounds_for_points(&points);
    let width = (0.001_f64).max(bounds.width);
    let height = (0.001_f64).max(bounds.height);
    TemplateFeatures {
        aspect_ratio: aspect_ratio(width, height),
        stroke_count: stroke_template.strokes.len(),
        orientation_deg: dominant_axis_orientation_deg(&points),
        stroke_profile: stroke_length_profile(&stroke_template.strokes),
    }
}

#[derive(Clone)]
pub struct CandidateFeatures {
    pub aspect_ratio: f64,
    pub elongation: f64,
    pub elongation_norm: f64,
    pub stroke_count: usize,
    pub stroke_length_imbalance: f64,
    pub axis_dominance: f64,
    stroke_profile: Vec<f64>,
}

pub fn candidate_features(candidate: &Candidate) -> CandidateFeatures {
    let bounds = candidate.bounds;
    let width = (1.0_f64).max(bounds.width);
    let height = (1.0_f64).max(bounds.height);
    let elongation = width.max(height) / (1.0_f64).max(width.min(height));
    let mut lengths: Vec<f64> = candidate.strokes.iter().map(|s| path_length(&s.points)).collect();
    lengths.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let total_stroke_length: f64 = lengths.iter().sum();
    let dominant = lengths.first().copied().unwrap_or(0.0);
    let secondary = lengths.get(1).copied().unwrap_or(0.0);
    let stroke_length_imbalance =
        if lengths.len() > 1 { (dominant - secondary) / (0.001_f64).max(total_stroke_length) } else { 0.0 };
    let elongation_norm = clamp((elongation - 1.0) / 3.0, 0.0, 1.0);
    let axis_dominance = clamp(stroke_length_imbalance * 1.35 + elongation_norm * 0.35, 0.0, 1.0);
    let point_lists: Vec<Vec<Point>> = candidate.strokes.iter().map(|s| s.points.clone()).collect();

    CandidateFeatures {
        aspect_ratio: aspect_ratio(width, height),
        elongation,
        elongation_norm,
        stroke_count: candidate.strokes.len(),
        stroke_length_imbalance,
        axis_dominance,
        stroke_profile: stroke_length_profile(&point_lists),
    }
}

pub struct StructuralMatch {
    pub score: f64,
    pub aspect_score: f64,
    pub stroke_count_score: f64,
    pub stroke_profile_score: f64,
    pub axis_score: f64,
    pub candidate_aspect_ratio: f64,
    pub template_aspect_ratio: f64,
    pub candidate_stroke_count: usize,
    pub template_stroke_count: usize,
}

fn structural_compatibility(
    kind: &str,
    entry: &DictionaryEntry,
    candidate: &Candidate,
    features: &CandidateFeatures,
    template_match: &TemplateScore,
) -> StructuralMatch {
    let template = template_features(entry.stroke_template.as_ref().unwrap());
    let aspect_score = aspect_compatibility(features.aspect_ratio, template.aspect_ratio, template_match.rotation_deg);
    let overdraw_compatible = template_match.candidate_explained_ratio >= 0.9
        && template_match.template_covered_ratio >= 0.82
        && template_match.unexplained_ink_ratio <= 0.16;
    let raw_count_score = stroke_count_compatibility(features.stroke_count, template.stroke_count);
    let raw_profile_score = profile_compatibility(&features.stroke_profile, &template.stroke_profile);
    let count_score = if overdraw_compatible && features.stroke_count > template.stroke_count {
        raw_count_score.max(0.86)
    } else {
        raw_count_score
    };
    let profile_score = if overdraw_compatible && features.stroke_count > template.stroke_count {
        raw_profile_score.max(0.82)
    } else {
        raw_profile_score
    };
    let rotated_candidate_axis = normalize_angle_deg(candidate.orientation_deg + template_match.rotation_deg);
    let axis_score = clamp(1.0 - undirected_angular_difference(rotated_candidate_axis, template.orientation_deg) / 90.0, 0.0, 1.0);
    let small_sign = kind == "sign" && template.stroke_count <= SIMPLE_SIGN_STROKE_LIMIT;
    let stroke_structure_score =
        if small_sign { count_score * 0.58 + profile_score * 0.42 } else { count_score * 0.24 + profile_score * 0.76 };
    let score = if kind == "sign" {
        stroke_structure_score * 0.68 + aspect_score * 0.2 + axis_score * 0.12
    } else {
        aspect_score * 0.54 + profile_score * 0.28 + count_score * 0.18
    };

    StructuralMatch {
        score: clamp(score, 0.0, 1.0),
        aspect_score,
        stroke_count_score: count_score,
        stroke_profile_score: profile_score,
        axis_score,
        candidate_aspect_ratio: features.aspect_ratio,
        template_aspect_ratio: template.aspect_ratio,
        candidate_stroke_count: features.stroke_count,
        template_stroke_count: template.stroke_count,
    }
}

fn is_contaminated_match(candidate: &Candidate, best: &ScoredEntry) -> bool {
    let Some(tm) = &best.template_match else { return false };
    let high_risk_extra_ink = tm.contamination_risk >= 0.62 && tm.unexplained_ink_ratio >= 0.34;
    let oversized_weak_match = candidate.size_norm >= 0.42 && tm.unexplained_ink_ratio >= 0.26 && best.confidence < 0.7;
    let wrong_region_ink = tm.forbidden_cell_ink_ratio >= 0.42 && tm.required_cell_coverage <= 0.82 && best.confidence < 0.72;
    high_risk_extra_ink || oversized_weak_match || wrong_region_ink
}

fn is_messy_match(candidate: &Candidate, best: &ScoredEntry) -> bool {
    let Some(tm) = &best.template_match else { return false };
    candidate.overdraw_amount >= 0.24
        || candidate.neatness <= 0.74
        || (tm.candidate_explained_ratio >= 0.9 && tm.soft_dice_score < 0.74)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionStatus {
    Unknown,
    Contaminated,
    Ambiguous,
    ValidMessy,
    Valid,
}

impl RecognitionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RecognitionStatus::Unknown => "unknown",
            RecognitionStatus::Contaminated => "contaminated",
            RecognitionStatus::Ambiguous => "ambiguous",
            RecognitionStatus::ValidMessy => "valid_messy",
            RecognitionStatus::Valid => "valid",
        }
    }
}

fn recognition_status(
    candidate: &Candidate,
    best: Option<&ScoredEntry>,
    second_confidence: f64,
    second_same_kind_confidence: f64,
    accepted_by_confidence: bool,
    ambiguity_gap: f64,
) -> RecognitionStatus {
    let Some(best) = best else { return RecognitionStatus::Unknown };
    if is_contaminated_match(candidate, best) {
        return RecognitionStatus::Contaminated;
    }
    if best.structural_match.as_ref().map(|s| s.score).unwrap_or(0.0) < 0.42 && best.confidence < 0.7 {
        return RecognitionStatus::Ambiguous;
    }
    if !accepted_by_confidence {
        return RecognitionStatus::Unknown;
    }

    let competitor = second_confidence.max(second_same_kind_confidence);
    let clear_ink_identity = best
        .template_match
        .as_ref()
        .map(|tm| tm.ink_score >= 0.92 && tm.candidate_explained_ratio >= 0.98 && tm.template_covered_ratio >= 0.98)
        .unwrap_or(false);
    if !clear_ink_identity && best.confidence - competitor < ambiguity_gap {
        return RecognitionStatus::Ambiguous;
    }
    if is_messy_match(candidate, best) {
        return RecognitionStatus::ValidMessy;
    }
    RecognitionStatus::Valid
}

struct ScoredEntry<'a> {
    kind: &'static str,
    entry: &'a DictionaryEntry,
    confidence: f64,
    template_match: Option<TemplateScore>,
    structural_match: Option<StructuralMatch>,
}

fn score_by_stroke_template<'a>(
    kind: &'static str,
    entry: &'a DictionaryEntry,
    candidate: &Candidate,
    features: &CandidateFeatures,
) -> ScoredEntry<'a> {
    let layer_score = allowed_layer_score(entry, candidate);
    let Some(stroke_template) = &entry.stroke_template else {
        return ScoredEntry { kind, entry, confidence: 0.0, template_match: None, structural_match: None };
    };
    if stroke_template.strokes.is_empty() {
        return ScoredEntry { kind, entry, confidence: 0.0, template_match: None, structural_match: None };
    }

    let recognition_entry = RecognitionEntry {
        recognition_rotation_invariant: entry.recognition_rotation_invariant,
        allowed_rotations_deg: entry.allowed_rotations_deg.clone(),
    };
    let plan = recognition_plan_for_symbol(kind, &recognition_entry, candidate);
    let match_features = if kind == "sign" { candidate_features(&plan.candidate) } else { features.clone() };
    let plan_strokes: Vec<Vec<Point>> = plan.candidate.strokes.iter().map(|s| s.points.clone()).collect();
    let raw_template_match = score_stroke_template(&plan_strokes, &stroke_template.strokes, &plan.options);
    let template_match = TemplateScore {
        rotation_deg: normalize_angle_deg(plan.base_rotation_deg + raw_template_match.rotation_deg),
        recognition_rotation_deg: normalize_angle_deg(plan.base_rotation_deg + raw_template_match.recognition_rotation_deg),
        ..raw_template_match
    };
    let structural_match = structural_compatibility(kind, entry, &plan.candidate, &match_features, &raw_template_match);

    let size_score = range_score(candidate.size_norm, 0.045, 0.46);
    let simple_sign_structure_multiplier = if kind == "sign" && structural_match.template_stroke_count <= SIMPLE_SIGN_STROKE_LIMIT {
        0.42 + structural_match.stroke_count_score * 0.58
    } else {
        1.0
    };
    let simple_sign_incomplete_cap = if kind == "sign"
        && structural_match.template_stroke_count <= SIMPLE_SIGN_STROKE_LIMIT
        && template_match.template_covered_ratio < SIMPLE_SIGN_MIN_TEMPLATE_COVERAGE
    {
        0.44
    } else {
        1.0
    };
    let gross_structure_mismatch_cap =
        if structural_match.score < 0.18 && template_match.template_covered_ratio < 0.5 { 0.44 } else { 1.0 };
    let contextual_score = template_match.confidence * 0.68
        + structural_match.score * 0.13
        + layer_score * 0.1
        + size_score * 0.04
        + candidate.neatness * 0.05;
    let context_lift_cap = template_match.confidence + 0.035;
    let confidence = clamp(contextual_score.min(context_lift_cap), 0.0, 1.0) * simple_sign_structure_multiplier;
    let confidence = confidence.min(simple_sign_incomplete_cap).min(gross_structure_mismatch_cap);

    ScoredEntry { kind, entry, confidence, template_match: Some(template_match), structural_match: Some(structural_match) }
}

#[derive(Debug, Clone)]
pub struct BestGuess {
    pub kind: String,
    pub id: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct TopMatch {
    pub kind: String,
    pub id: String,
    pub confidence: f64,
    pub template_confidence: f64,
    pub ink_score: f64,
    pub structural_score: f64,
    pub rotation_deg: f64,
}

#[derive(Debug, Clone)]
pub struct RecognitionDiagnostics {
    pub best_guess: Option<BestGuess>,
    pub recognition_rotation_deg: f64,
    pub top_matches: Vec<TopMatch>,
}

#[derive(Debug, Clone)]
pub struct RecognitionShape {
    pub stroke_count: usize,
    pub aspect_ratio: f64,
    pub elongation: f64,
    pub elongation_norm: f64,
    pub stroke_length_imbalance: f64,
    pub axis_dominance: f64,
}

#[derive(Debug, Clone)]
pub struct Recognition {
    pub candidate_id: String,
    pub layer: Layer,
    pub near_boundary: bool,
    pub radius_norm: f64,
    pub angle_deg: f64,
    pub size_norm: f64,
    pub length_norm: f64,
    pub orientation_deg: f64,
    pub directed_orientation_deg: f64,
    pub radial_facing: RadialFacing,
    pub neatness: f64,
    pub recognized: bool,
    pub recognition_status: RecognitionStatus,
    pub kind: String,
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub element: Option<String>,
    pub semantic: Option<SemanticFields>,
    pub confidence: f64,
    pub shape: RecognitionShape,
    pub diagnostics: RecognitionDiagnostics,
}

pub fn recognize_candidates(candidates: &[Candidate], dictionary: &Dictionary, config: &RecognitionConfig) -> Vec<Recognition> {
    candidates
        .iter()
        .map(|candidate| {
            let features = candidate_features(candidate);
            let mut scored: Vec<ScoredEntry> = dictionary
                .sigils
                .iter()
                .map(|entry| score_by_stroke_template("sigil", entry, candidate, &features))
                .chain(dictionary.signs.iter().map(|entry| score_by_stroke_template("sign", entry, candidate, &features)))
                .collect();
            scored.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

            let best_index = if scored.is_empty() { None } else { Some(0usize) };
            let second_confidence = scored.get(1).map(|s| s.confidence).unwrap_or(0.0);
            let second_same_kind_confidence = best_index
                .and_then(|bi| {
                    let best_kind = scored[bi].kind;
                    let best_id = scored[bi].entry.id.as_str();
                    scored.iter().find(|s| s.kind == best_kind && s.entry.id != best_id)
                })
                .map(|s| s.confidence)
                .unwrap_or(0.0);

            let accepted_by_confidence = best_index.map(|bi| scored[bi].confidence >= config.min_confidence).unwrap_or(false);
            let status = recognition_status(
                candidate,
                best_index.map(|bi| &scored[bi]),
                second_confidence,
                second_same_kind_confidence,
                accepted_by_confidence,
                RECOGNITION_AMBIGUITY_GAP,
            );
            let accepted = accepted_by_confidence && matches!(status, RecognitionStatus::Valid | RecognitionStatus::ValidMessy);

            let top_matches: Vec<TopMatch> = scored
                .iter()
                .take(3)
                .map(|s| TopMatch {
                    kind: s.kind.to_string(),
                    id: s.entry.id.clone(),
                    confidence: s.confidence,
                    template_confidence: s.template_match.as_ref().map(|t| t.confidence).unwrap_or(0.0),
                    ink_score: s.template_match.as_ref().map(|t| t.ink_score).unwrap_or(0.0),
                    structural_score: s.structural_match.as_ref().map(|m| m.score).unwrap_or(0.0),
                    rotation_deg: s.template_match.as_ref().map(|t| t.rotation_deg).unwrap_or(0.0),
                })
                .collect();

            let best = best_index.map(|bi| &scored[bi]);
            let best_guess = best.map(|b| BestGuess { kind: b.kind.to_string(), id: b.entry.id.clone(), confidence: b.confidence });
            let best_template_match = best.and_then(|b| b.template_match.as_ref());

            Recognition {
                candidate_id: candidate.candidate_id.clone(),
                layer: candidate.layer,
                near_boundary: candidate.near_boundary,
                radius_norm: candidate.radius_norm,
                angle_deg: candidate.angle_deg,
                size_norm: candidate.size_norm,
                length_norm: candidate.length_norm,
                orientation_deg: candidate.orientation_deg,
                directed_orientation_deg: candidate.directed_orientation_deg,
                radial_facing: candidate.radial_facing,
                neatness: candidate.neatness,
                recognized: accepted,
                recognition_status: status,
                kind: if accepted { best.unwrap().kind.to_string() } else { "unknown".to_string() },
                id: if accepted { Some(best.unwrap().entry.id.clone()) } else { None },
                display_name: if accepted { best.unwrap().entry.display_name.clone() } else { None },
                element: if accepted { best.unwrap().entry.element.clone() } else { None },
                semantic: if accepted { best.unwrap().entry.semantic.clone() } else { None },
                confidence: if accepted { best.unwrap().confidence } else { 0.0 },
                shape: RecognitionShape {
                    stroke_count: features.stroke_count,
                    aspect_ratio: features.aspect_ratio,
                    elongation: features.elongation,
                    elongation_norm: features.elongation_norm,
                    stroke_length_imbalance: features.stroke_length_imbalance,
                    axis_dominance: features.axis_dominance,
                },
                diagnostics: RecognitionDiagnostics {
                    best_guess: if accepted { None } else { best_guess },
                    recognition_rotation_deg: best_template_match.map(|t| t.recognition_rotation_deg).unwrap_or(0.0),
                    top_matches,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{INPUT, LAYERS};
    use crate::coordinate_normalizer::{classify_strokes_against_ring, Ring};
    use crate::stroke_cleaner::clean_strokes;
    use crate::stroke_grouper::build_symbol_candidates;

    fn square_points(cx: f64, cy: f64, half: f64) -> Vec<Point> {
        let n = 20;
        (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                // Trace a square outline as one continuous stroke.
                if t < 0.25 {
                    Point { x: cx - half + t * 4.0 * (2.0 * half), y: cy - half }
                } else if t < 0.5 {
                    Point { x: cx + half, y: cy - half + (t - 0.25) * 4.0 * (2.0 * half) }
                } else if t < 0.75 {
                    Point { x: cx + half - (t - 0.5) * 4.0 * (2.0 * half), y: cy + half }
                } else {
                    Point { x: cx - half, y: cy + half - (t - 0.75) * 4.0 * (2.0 * half) }
                }
            })
            .collect()
    }

    fn square_candidate(cx: f64, cy: f64, half: f64, ring: &Ring) -> Candidate {
        let raw = vec![crate::stroke_cleaner::RawStroke { id: "s1".into(), points: square_points(cx, cy, half) }];
        let strokes = clean_strokes(&raw, &INPUT);
        let classifications = classify_strokes_against_ring(&strokes, ring, &LAYERS);
        let mut candidates = build_symbol_candidates(&strokes, &classifications, ring, &LAYERS);
        candidates.remove(0)
    }

    fn square_entry(id: &str) -> DictionaryEntry {
        DictionaryEntry {
            id: id.to_string(),
            display_name: Some(id.to_string()),
            element: Some(id.to_string()),
            stroke_template: Some(StrokeTemplate { strokes: vec![square_points(0.0, 0.0, 10.0)] }),
            recognition_rotation_invariant: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn matching_square_is_recognized_valid() {
        let ring = Ring { found: true, center: Point { x: 0.0, y: 0.0 }, radius: 100.0, stroke_ids: vec![], ..Default::default() };
        let candidate = square_candidate(0.0, 0.0, 10.0, &ring);
        let dictionary = Dictionary { sigils: vec![square_entry("square-sigil")], signs: vec![] };
        let config = RecognitionConfig { min_confidence: 0.48 };

        let recognitions = recognize_candidates(&[candidate], &dictionary, &config);
        assert_eq!(recognitions.len(), 1);
        assert!(recognitions[0].recognized, "identical shape should recognize, status={:?}", recognitions[0].recognition_status);
        assert_eq!(recognitions[0].recognition_status, RecognitionStatus::Valid);
        assert_eq!(recognitions[0].id.as_deref(), Some("square-sigil"));
        assert_eq!(recognitions[0].kind, "sigil");
    }

    #[test]
    fn empty_dictionary_yields_unknown() {
        let ring = Ring { found: true, center: Point { x: 0.0, y: 0.0 }, radius: 100.0, stroke_ids: vec![], ..Default::default() };
        let candidate = square_candidate(0.0, 0.0, 10.0, &ring);
        let dictionary = Dictionary { sigils: vec![], signs: vec![] };
        let config = RecognitionConfig { min_confidence: 0.48 };

        let recognitions = recognize_candidates(&[candidate], &dictionary, &config);
        assert!(!recognitions[0].recognized);
        assert_eq!(recognitions[0].recognition_status, RecognitionStatus::Unknown);
        assert_eq!(recognitions[0].kind, "unknown");
        assert!(recognitions[0].id.is_none());
    }

    #[test]
    fn unrelated_template_scores_below_threshold() {
        let ring = Ring { found: true, center: Point { x: 0.0, y: 0.0 }, radius: 100.0, stroke_ids: vec![], ..Default::default() };
        let candidate = square_candidate(0.0, 0.0, 10.0, &ring);
        // A single-dot template is nothing like a square outline.
        let dot_entry = DictionaryEntry {
            id: "dot".into(),
            stroke_template: Some(StrokeTemplate { strokes: vec![vec![Point { x: 0.0, y: 0.0 }]] }),
            ..Default::default()
        };
        let dictionary = Dictionary { sigils: vec![dot_entry], signs: vec![] };
        let config = RecognitionConfig { min_confidence: 0.48 };

        let recognitions = recognize_candidates(&[candidate], &dictionary, &config);
        assert!(!recognitions[0].recognized);
    }
}
