//! Direct port of service/vendor/wha/src/parser/drawingClassifier.js — the
//! pipeline entry point tying every other module together.
//!
//! ponytail: the JS's `roundedDeep`/`stripCandidate`/`stripRecognitionDiagnostics`
//! exist to trim payloads for the browser's JSON wire format. There's no
//! serialization boundary in this crate yet (that's the device crate's job,
//! later), so GlyphAst below keeps full Candidate/Recognition values —
//! trimming for whatever wire format the device actually needs is deferred
//! to that task instead of guessing a shape now.

use crate::config::{InputConfig, LayersConfig, RingConfig};
use crate::coordinate_normalizer::{classify_strokes_against_ring, Ring, StrokeClassification};
use crate::geometry::{clamp, mean, vector_from_angle_deg};
use crate::glyph_warnings::GlyphWarning;
use crate::layer_mapper::Layer;
use crate::ring_detector::detect_ring;
use crate::stroke_cleaner::{clean_strokes, CleanedStroke, RawStroke};
use crate::stroke_grouper::{build_symbol_candidates, Candidate};
use crate::symbol_recognizer::{recognize_candidates, BestGuess, Dictionary, Recognition, RecognitionConfig};

// Assuming a sigil has to be in the center of the ring, so reward the score a little bit.
fn primary_sigil_score(sigil: &Recognition) -> f64 {
    let layer_bonus = if sigil.layer == Layer::Center {
        0.12
    } else if sigil.radius_norm <= 0.45 {
        0.06
    } else {
        0.0
    };
    sigil.confidence + layer_bonus
}

fn recognized_sigils(recognitions: &[Recognition]) -> Vec<&Recognition> {
    let mut sigils: Vec<&Recognition> = recognitions.iter().filter(|r| r.recognized && r.kind == "sigil").collect();
    sigils.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    sigils
}

fn select_primary_sigil<'a>(sigils: &[&'a Recognition]) -> Option<&'a Recognition> {
    sigils.iter().copied().max_by(|a, b| primary_sigil_score(a).partial_cmp(&primary_sigil_score(b)).unwrap())
}

#[derive(Debug, Clone)]
pub struct UnknownSummary {
    pub candidate_id: String,
    pub stroke_ids: Vec<String>,
    pub layer: Layer,
    pub radius_norm: f64,
    pub angle_deg: f64,
    pub reason: String,
    pub best_guess: Option<BestGuess>,
}

fn summarize_unknowns(candidates: &[Candidate], recognitions: &[Recognition]) -> Vec<UnknownSummary> {
    let by_candidate: std::collections::HashMap<&str, &Recognition> =
        recognitions.iter().map(|r| (r.candidate_id.as_str(), r)).collect();

    candidates
        .iter()
        .filter(|c| !by_candidate.get(c.candidate_id.as_str()).map(|r| r.recognized).unwrap_or(false))
        .map(|candidate| {
            let recognition = by_candidate.get(candidate.candidate_id.as_str()).copied();
            UnknownSummary {
                candidate_id: candidate.candidate_id.clone(),
                stroke_ids: candidate.stroke_ids.clone(),
                layer: candidate.layer,
                radius_norm: candidate.radius_norm,
                angle_deg: candidate.angle_deg,
                reason: recognition.map(|r| r.recognition_status.as_str().to_string()).unwrap_or_else(|| "no_confident_match".to_string()),
                best_guess: recognition.and_then(|r| r.diagnostics.best_guess.clone()),
            }
        })
        .collect()
}

fn calculate_directional_bias(signs: &[&Recognition]) -> (f64, f64) {
    if signs.is_empty() {
        return (0.0, 0.0);
    }

    let (mut vx, mut vy) = (0.0, 0.0);
    for sign in signs {
        let direction = vector_from_angle_deg(sign.angle_deg);
        let weight = sign.confidence * sign.neatness * (0.3_f64).max(sign.size_norm + sign.length_norm);
        vx += direction.x * weight;
        vy += direction.y * weight;
    }

    let magnitude = vx.hypot(vy);
    if magnitude < 0.001 {
        (0.0, 0.0)
    } else {
        (vx / magnitude, vy / magnitude)
    }
}

#[derive(Debug, Clone)]
pub struct GlobalMetrics {
    pub neatness: f64,
    pub radial_symmetry: f64,
    pub instability: f64,
}

fn calculate_global_metrics(ring: &Ring, recognitions: &[Recognition], unknowns: &[UnknownSummary]) -> GlobalMetrics {
    let recognized: Vec<&Recognition> = recognitions.iter().filter(|r| r.recognized).collect();
    let neatness_values: Vec<f64> = std::iter::once(ring.neatness)
        .chain(recognized.iter().map(|r| if r.neatness > 0.0 { r.neatness } else { 0.6 }))
        .filter(|&v| v > 0.0)
        .collect();
    let neatness_average = mean(&neatness_values);
    let signs: Vec<&Recognition> = recognized.iter().filter(|r| r.kind == "sign").copied().collect();
    let (bias_x, bias_y) = calculate_directional_bias(&signs);
    let unknown_penalty = clamp(unknowns.len() as f64 / 6.0, 0.0, 1.0);
    let contaminated_penalty = clamp(recognitions.iter().filter(|r| r.recognition_status.as_str() == "contaminated").count() as f64 / 4.0, 0.0, 1.0);
    let ambiguous_penalty = clamp(recognitions.iter().filter(|r| r.recognition_status.as_str() == "ambiguous").count() as f64 / 5.0, 0.0, 1.0);
    let messy_penalty = clamp(recognitions.iter().filter(|r| r.recognition_status.as_str() == "valid_messy").count() as f64 / 8.0, 0.0, 1.0);

    GlobalMetrics {
        neatness: clamp(neatness_average, 0.0, 1.0),
        radial_symmetry: clamp(1.0 - bias_x.hypot(bias_y) * 0.35, 0.0, 1.0),
        instability: clamp(
            0.22 + unknown_penalty * 0.34 + contaminated_penalty * 0.22 + ambiguous_penalty * 0.12 + messy_penalty * 0.08
                + (1.0 - ring.neatness) * 0.36,
            0.0,
            1.0,
        ),
    }
}

fn warning_list(
    ring: &Ring,
    primary_sigil: Option<&Recognition>,
    unsupported_multiple_sigils: &[Recognition],
    unknowns: &[UnknownSummary],
    recognitions: &[Recognition],
) -> Vec<GlyphWarning> {
    let mut warnings = vec![];
    if !ring.found {
        warnings.push(GlyphWarning::NoRingDetected);
    } else if !ring.complete {
        warnings.push(GlyphWarning::RingIncomplete);
    }
    if !ring.unsupported_nested_rings.is_empty() {
        warnings.push(GlyphWarning::UnsupportedNestedRing);
    }
    if !ring.unsupported_multiple_rings.is_empty() {
        warnings.push(GlyphWarning::UnsupportedMultipleRings);
    }
    if !unsupported_multiple_sigils.is_empty() {
        warnings.push(GlyphWarning::UnsupportedMultipleSigils);
    }
    if primary_sigil.is_none() {
        warnings.push(GlyphWarning::MissingPrimarySigil);
    }
    if unknowns.iter().any(|u| u.radius_norm <= 0.36) {
        warnings.push(GlyphWarning::CenterUnknownContamination);
    }
    if recognitions.iter().any(|r| r.recognized && r.near_boundary) {
        warnings.push(GlyphWarning::SymbolNearLayerBoundary);
    }
    if recognitions.iter().any(|r| r.recognition_status.as_str() == "contaminated") {
        warnings.push(GlyphWarning::SymbolContaminated);
    }
    if recognitions.iter().any(|r| r.recognition_status.as_str() == "ambiguous") {
        warnings.push(GlyphWarning::SymbolAmbiguous);
    }
    if recognitions.iter().any(|r| r.recognition_status.as_str() == "valid_messy") {
        warnings.push(GlyphWarning::SymbolMessy);
    }
    warnings
}

#[derive(Debug, Clone)]
pub struct GlyphAst {
    pub version: String,
    pub ring: Ring,
    pub candidates: Vec<Candidate>,
    pub primary_sigil: Option<Recognition>,
    pub unsupported_multiple_sigils: Vec<Recognition>,
    pub signs: Vec<Recognition>,
    pub unknowns: Vec<UnknownSummary>,
    pub global_metrics: GlobalMetrics,
    pub warnings: Vec<GlyphWarning>,
}

pub struct ClassifyResult {
    pub cleaned_strokes: Vec<CleanedStroke>,
    pub ring: Ring,
    pub classifications: Vec<StrokeClassification>,
    pub candidates: Vec<Candidate>,
    pub recognitions: Vec<Recognition>,
    pub glyph_ast: GlyphAst,
}

pub fn classify_drawing(
    strokes: &[RawStroke],
    previous_ring: Option<&Ring>,
    dictionary: &Dictionary,
    app_version: &str,
    input_config: &InputConfig,
    ring_config: &RingConfig,
    layers_config: &LayersConfig,
    recognition_config: &RecognitionConfig,
) -> ClassifyResult {
    let cleaned_strokes = clean_strokes(strokes, input_config);
    let ring = detect_ring(&cleaned_strokes, previous_ring, ring_config, layers_config);

    if !ring.found {
        let glyph_ast = GlyphAst {
            version: app_version.to_string(),
            ring: ring.clone(),
            candidates: vec![],
            primary_sigil: None,
            unsupported_multiple_sigils: vec![],
            signs: vec![],
            unknowns: vec![],
            global_metrics: GlobalMetrics { neatness: 0.0, radial_symmetry: 0.0, instability: 1.0 },
            warnings: vec![GlyphWarning::NoRingDetected],
        };
        return ClassifyResult { cleaned_strokes, ring, classifications: vec![], candidates: vec![], recognitions: vec![], glyph_ast };
    }

    let classifications = classify_strokes_against_ring(&cleaned_strokes, &ring, layers_config);
    let candidates = build_symbol_candidates(&cleaned_strokes, &classifications, &ring, layers_config);
    let recognitions = recognize_candidates(&candidates, dictionary, recognition_config);
    let sigils = recognized_sigils(&recognitions);
    let primary_sigil = select_primary_sigil(&sigils);
    let primary_candidate_id = primary_sigil.map(|s| s.candidate_id.clone());
    let unsupported_multiple_sigils: Vec<Recognition> =
        sigils.iter().filter(|s| Some(&s.candidate_id) != primary_candidate_id.as_ref()).map(|s| (*s).clone()).collect();
    let signs: Vec<Recognition> = recognitions.iter().filter(|r| r.recognized && r.kind == "sign").cloned().collect();
    let unknowns = summarize_unknowns(&candidates, &recognitions);
    let global_metrics = calculate_global_metrics(&ring, &recognitions, &unknowns);
    let warnings = warning_list(&ring, primary_sigil, &unsupported_multiple_sigils, &unknowns, &recognitions);

    let glyph_ast = GlyphAst {
        version: app_version.to_string(),
        ring: ring.clone(),
        candidates: candidates.clone(),
        primary_sigil: primary_sigil.cloned(),
        unsupported_multiple_sigils,
        signs,
        unknowns,
        global_metrics,
        warnings,
    };

    ClassifyResult { cleaned_strokes, ring, classifications, candidates, recognitions, glyph_ast }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{INPUT, LAYERS, RECOGNITION, RING};
    use crate::symbol_recognizer::{Dictionary, DictionaryEntry, StrokeTemplate};

    fn ring_points(cx: f64, cy: f64, radius: f64) -> Vec<crate::geometry::Point> {
        (0..=130)
            .map(|i| {
                let angle = (i as f64 / 128.0) * std::f64::consts::TAU;
                crate::geometry::Point { x: cx + angle.cos() * radius, y: cy + angle.sin() * radius }
            })
            .collect()
    }

    #[test]
    fn no_strokes_yields_no_ring_glyph_ast() {
        let dictionary = Dictionary { sigils: vec![], signs: vec![] };
        let result = classify_drawing(&[], None, &dictionary, "0.1.0-test", &INPUT, &RING, &LAYERS, &RECOGNITION);
        assert!(!result.ring.found);
        assert_eq!(result.glyph_ast.warnings.len(), 1);
        assert_eq!(result.glyph_ast.warnings[0], GlyphWarning::NoRingDetected);
        assert!(result.glyph_ast.primary_sigil.is_none());
        assert_eq!(result.glyph_ast.global_metrics.instability, 1.0);
    }

    #[test]
    fn ring_without_sigil_warns_missing_primary_sigil() {
        let raw = vec![RawStroke { id: "ring".into(), points: ring_points(350.0, 450.0, 260.0) }];
        let dictionary = Dictionary { sigils: vec![], signs: vec![] };
        let result = classify_drawing(&raw, None, &dictionary, "0.1.0-test", &INPUT, &RING, &LAYERS, &RECOGNITION);
        assert!(result.ring.found);
        assert!(result.glyph_ast.warnings.contains(&GlyphWarning::MissingPrimarySigil));
        assert!(result.glyph_ast.primary_sigil.is_none());
    }

    #[test]
    fn ring_with_matching_sigil_selects_primary() {
        let square: Vec<crate::geometry::Point> = {
            let n = 20;
            (0..=n)
                .map(|i| {
                    let t = i as f64 / n as f64;
                    let half = 10.0;
                    if t < 0.25 {
                        crate::geometry::Point { x: -half + t * 4.0 * (2.0 * half), y: -half }
                    } else if t < 0.5 {
                        crate::geometry::Point { x: half, y: -half + (t - 0.25) * 4.0 * (2.0 * half) }
                    } else if t < 0.75 {
                        crate::geometry::Point { x: half - (t - 0.5) * 4.0 * (2.0 * half), y: half }
                    } else {
                        crate::geometry::Point { x: -half, y: half - (t - 0.75) * 4.0 * (2.0 * half) }
                    }
                })
                .collect()
        };
        let raw = vec![
            RawStroke { id: "ring".into(), points: ring_points(0.0, 0.0, 100.0) },
            RawStroke { id: "sigil".into(), points: square.clone() },
        ];
        let entry = DictionaryEntry {
            id: "test-square".into(),
            element: Some("fire".into()),
            stroke_template: Some(StrokeTemplate { strokes: vec![square] }),
            recognition_rotation_invariant: Some(true),
            ..Default::default()
        };
        let dictionary = Dictionary { sigils: vec![entry], signs: vec![] };
        let result = classify_drawing(&raw, None, &dictionary, "0.1.0-test", &INPUT, &RING, &LAYERS, &RECOGNITION);
        assert!(result.ring.found);
        assert!(result.glyph_ast.primary_sigil.is_some(), "warnings: {:?}", result.glyph_ast.warnings);
        assert_eq!(result.glyph_ast.primary_sigil.unwrap().id.as_deref(), Some("test-square"));
        assert!(!result.glyph_ast.warnings.contains(&GlyphWarning::MissingPrimarySigil));
    }
}
