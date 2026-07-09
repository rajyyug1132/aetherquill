//! Direct port of service/vendor/wha/src/compiler/spellQuality.js.

use crate::config::CompilerConfig;
use crate::drawing_classifier::GlyphAst;
use crate::geometry::{clamp, mean};
use crate::glyph_warnings::GlyphWarning;
use crate::symbol_recognizer::Recognition;

struct QualityTuning;
impl QualityTuning {
    const RING_QUALITY: f64 = 0.25;
    const PRIMARY_CONFIDENCE: f64 = 0.25;
    const SIGN_CONFIDENCE: f64 = 0.2;
    const SIGN_FALLBACK_PRIMARY_CONFIDENCE: f64 = 0.7;
    const GLOBAL_NEATNESS: f64 = 0.15;
    const RADIAL_SYMMETRY: f64 = 0.1;
    const INSIDE_SCORE: f64 = 0.05;
    const UNKNOWN_SOFT_LIMIT: f64 = 7.0;
}

// ponytail: JS's radialSymmetryFallback/instabilityFallback guard
// `glyphAST.globalMetrics?.radialSymmetry`/`?.instability` being undefined —
// GlyphAst.global_metrics is a required, always-populated field in Rust, so
// those fallbacks are unreachable here. Omitted, same reasoning as the
// `candidate.layer === "any"` branch dropped in symbol_recognizer.rs.
struct StabilityTuning;
impl StabilityTuning {
    const RING_NEATNESS: f64 = 0.36;
    const SYMBOL_NEATNESS: f64 = 0.34;
    const SYMBOL_NEATNESS_FALLBACK: f64 = 0.35;
    const RADIAL_SYMMETRY: f64 = 0.12;
    const INVERSE_INSTABILITY: f64 = 0.18;
    const UNKNOWN_PENALTY_MAX: f64 = 0.34;
    const UNKNOWN_PENALTY_SCALE: f64 = 0.24;
    const AMBIGUITY_GRACE: f64 = 0.14;
    const BOUNDARY_PENALTY: f64 = 0.08;
    const CENTER_PENALTY: f64 = 0.16;
}

fn top_match_competitor_confidence(recognition: Option<&Recognition>) -> f64 {
    let Some(recognition) = recognition else { return 0.0 };
    let self_id = recognition.id.as_deref().unwrap_or("");
    recognition
        .diagnostics
        .top_matches
        .iter()
        .find(|t| t.kind != recognition.kind || t.id != self_id)
        .map(|t| t.confidence)
        .unwrap_or(0.0)
}

pub fn calculate_spell_quality(glyph_ast: &GlyphAst) -> f64 {
    let ring_quality = glyph_ast.ring.neatness;
    let primary_confidence = glyph_ast.primary_sigil.as_ref().map(|s| s.confidence).unwrap_or(0.0);
    let sign_confidence = mean(&glyph_ast.signs.iter().map(|s| s.confidence).collect::<Vec<_>>());
    let global_neatness = glyph_ast.global_metrics.neatness;
    let symmetry = glyph_ast.global_metrics.radial_symmetry;
    let inside_score = 1.0 - (1.0_f64).min(glyph_ast.unknowns.len() as f64 / QualityTuning::UNKNOWN_SOFT_LIMIT);

    let sign_term = if sign_confidence != 0.0 { sign_confidence } else { primary_confidence * QualityTuning::SIGN_FALLBACK_PRIMARY_CONFIDENCE };

    clamp(
        ring_quality * QualityTuning::RING_QUALITY
            + primary_confidence * QualityTuning::PRIMARY_CONFIDENCE
            + sign_term * QualityTuning::SIGN_CONFIDENCE
            + global_neatness * QualityTuning::GLOBAL_NEATNESS
            + symmetry * QualityTuning::RADIAL_SYMMETRY
            + inside_score * QualityTuning::INSIDE_SCORE,
        0.0,
        1.0,
    )
}

pub fn calculate_spell_stability(glyph_ast: &GlyphAst, compiler_config: &CompilerConfig) -> f64 {
    let ring_neatness = glyph_ast.ring.neatness;
    let symbol_neatness_values: Vec<f64> = std::iter::once(glyph_ast.primary_sigil.as_ref().map(|s| s.neatness).unwrap_or(0.0))
        .chain(glyph_ast.signs.iter().map(|s| s.neatness))
        .filter(|&v| v != 0.0)
        .collect();
    let symbol_neatness = mean(&symbol_neatness_values);
    let unknown_penalty = StabilityTuning::UNKNOWN_PENALTY_MAX
        .min((glyph_ast.unknowns.len() as f64 / compiler_config.max_unknowns_before_instability) * StabilityTuning::UNKNOWN_PENALTY_SCALE);
    let ambiguity_penalty = (0.0_f64).max(
        top_match_competitor_confidence(glyph_ast.primary_sigil.as_ref()) - glyph_ast.primary_sigil.as_ref().map(|s| s.confidence).unwrap_or(0.0)
            + StabilityTuning::AMBIGUITY_GRACE,
    );
    let boundary_penalty = if glyph_ast.warnings.contains(&GlyphWarning::SymbolNearLayerBoundary) { StabilityTuning::BOUNDARY_PENALTY } else { 0.0 };
    let center_penalty = if glyph_ast.warnings.contains(&GlyphWarning::CenterUnknownContamination) { StabilityTuning::CENTER_PENALTY } else { 0.0 };
    let inverse_instability = 1.0 - glyph_ast.global_metrics.instability;

    clamp(
        ring_neatness * StabilityTuning::RING_NEATNESS
            + (if symbol_neatness != 0.0 { symbol_neatness } else { StabilityTuning::SYMBOL_NEATNESS_FALLBACK }) * StabilityTuning::SYMBOL_NEATNESS
            + glyph_ast.global_metrics.radial_symmetry * StabilityTuning::RADIAL_SYMMETRY
            + inverse_instability * StabilityTuning::INVERSE_INSTABILITY
            - unknown_penalty
            - ambiguity_penalty
            - boundary_penalty
            - center_penalty,
        0.0,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::COMPILER;
    use crate::drawing_classifier::GlobalMetrics;
    use crate::ring_detector::Ring;

    fn base_glyph_ast() -> GlyphAst {
        GlyphAst {
            version: "test".into(),
            ring: Ring { found: true, neatness: 0.8, ..Default::default() },
            candidates: vec![],
            primary_sigil: None,
            unsupported_multiple_sigils: vec![],
            signs: vec![],
            unknowns: vec![],
            global_metrics: GlobalMetrics { neatness: 0.7, radial_symmetry: 0.9, instability: 0.2 },
            warnings: vec![],
        }
    }

    #[test]
    fn quality_with_no_sigil_or_signs_is_low_but_not_zero() {
        let glyph_ast = base_glyph_ast();
        let quality = calculate_spell_quality(&glyph_ast);
        // Still gets ring/global/symmetry contribution even with no sigil.
        assert!(quality > 0.0 && quality < 0.5, "got {quality}");
    }

    #[test]
    fn stability_with_no_warnings_is_higher_than_with_boundary_warning() {
        let clean = base_glyph_ast();
        let mut warned = base_glyph_ast();
        warned.warnings.push(GlyphWarning::SymbolNearLayerBoundary);

        let clean_stability = calculate_spell_stability(&clean, &COMPILER);
        let warned_stability = calculate_spell_stability(&warned, &COMPILER);
        assert!(warned_stability < clean_stability, "boundary warning should reduce stability: {warned_stability} vs {clean_stability}");
    }

    #[test]
    fn more_unknowns_reduce_stability() {
        let mut few = base_glyph_ast();
        few.unknowns.push(crate::drawing_classifier::UnknownSummary {
            candidate_id: "c1".into(),
            stroke_ids: vec![],
            layer: crate::layer_mapper::Layer::Outer,
            radius_norm: 0.5,
            angle_deg: 0.0,
            reason: "unknown".into(),
            best_guess: None,
        });
        let mut many = base_glyph_ast();
        for i in 0..8 {
            many.unknowns.push(crate::drawing_classifier::UnknownSummary {
                candidate_id: format!("c{i}"),
                stroke_ids: vec![],
                layer: crate::layer_mapper::Layer::Outer,
                radius_norm: 0.5,
                angle_deg: 0.0,
                reason: "unknown".into(),
                best_guess: None,
            });
        }

        assert!(calculate_spell_stability(&many, &COMPILER) < calculate_spell_stability(&few, &COMPILER));
    }
}
