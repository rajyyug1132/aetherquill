//! Direct port of service/vendor/wha/src/compiler/spellBuilder.js — the final
//! compiler stage: GlyphAst -> SpellIr.
//!
//! ponytail: JS stamps `activatedAt` with `performance.now()` (a browser
//! wall-clock read) when a spell activates. That's a device-runtime concern
//! (when did MY clock see this), not pipeline logic — dropped here; the
//! device crate stamps its own timestamp when it observes `active` flip true.

use crate::config::{CompilerConfig, EffectSizeConfig};
use crate::drawing_classifier::GlyphAst;
use crate::geometry::clamp;
use crate::glyph_warnings::GlyphWarning;
use crate::semantic_rules::{aggregate_manifestations, aggregate_semantic_deltas, combine_sign_direction, sign_influence, Manifestation, SemanticDeltas, SurfaceDirection};
use crate::spell_direction::{direction_from_surface_vector, SpellDirection};
use crate::spell_quality::{calculate_spell_quality, calculate_spell_stability};
use crate::symbol_recognizer::{Recognition, SemanticFields};
use std::collections::HashMap;

const PRIMARY_SIGIL_AMBIGUITY_GAP: f64 = 0.05;
const SUPPORTED_ELEMENTS: [&str; 5] = ["fire", "water", "wind", "earth", "light"];

struct SpellParameterTuning;
impl SpellParameterTuning {
    const FOCUS_BASE: f64 = 0.46;
    const FOCUS_QUALITY: f64 = 0.2;
    const SPREAD_BASE: f64 = 0.32;
    const SPREAD_INVERSE_FOCUS: f64 = 0.28;
    const FORCE_BASE: f64 = 0.34;
    const FORCE_SIGN_POWER: f64 = 0.34;
    const FORCE_QUALITY: f64 = 0.18;
    const RANGE_BASE: f64 = 0.42;
    const RANGE_SIGN_POWER: f64 = 0.18;
    const DURATION_MIN_SECONDS: f64 = 0.65;
    const DURATION_MAX_SECONDS: f64 = 8.5;
    const DURATION_SECONDS_SCALE: f64 = 6.4;
    const DURATION_QUALITY_WEIGHT: f64 = 0.35;
    const DURATION_NEATNESS_WEIGHT: f64 = 0.65;
    const DURATION_CURVE: f64 = 1.45;
}

const LEVITATION_GRAVITY_SCALE: f64 = 0.42;

fn same_kind_alternate_confidence(recognition: &Recognition) -> f64 {
    let self_id = recognition.id.as_deref().unwrap_or("");
    recognition
        .diagnostics
        .top_matches
        .iter()
        .find(|t| t.kind == recognition.kind && t.id != self_id)
        .map(|t| t.confidence)
        .unwrap_or(0.0)
}

#[derive(Debug, Clone)]
pub struct SpellIr {
    pub active: bool,
    pub prepared: bool,
    pub valid: bool,
    pub status: String,
    pub element: Option<String>,
    pub element_confidence: f64,
    pub primary_size_norm: f64,
    pub effect_scale: f64,
    pub primary_manifestation: String,
    pub manifestations: HashMap<String, Manifestation>,
    pub direction: SpellDirection,
    pub direction_coherence: f64,
    pub gravity: f64,
    pub force: f64,
    pub spread: f64,
    pub focus: f64,
    pub range: f64,
    pub duration: f64,
    pub stability: f64,
    pub quality: f64,
    pub neatness: f64,
    pub warnings: Vec<GlyphWarning>,
    pub signature: String,
}

fn dedup_warnings(base: &[GlyphWarning], extra: &[GlyphWarning]) -> Vec<GlyphWarning> {
    let mut seen = std::collections::HashSet::new();
    base.iter().chain(extra.iter()).copied().filter(|w| seen.insert(*w)).collect()
}

fn invalid_spell(status: &str, glyph_ast: &GlyphAst, extra_warnings: &[GlyphWarning]) -> SpellIr {
    let ring_complete = glyph_ast.ring.complete;
    SpellIr {
        active: false,
        prepared: false,
        valid: false,
        status: status.to_string(),
        element: None,
        element_confidence: 0.0,
        primary_size_norm: 0.0,
        effect_scale: 1.0,
        primary_manifestation: "none".to_string(),
        manifestations: HashMap::new(),
        direction: SpellDirection { x: 0.0, y: 0.0, z: 1.0, x_tilt_deg: 0.0, y_tilt_deg: 0.0, tilt_from_z_deg: 0.0 },
        direction_coherence: 0.0,
        gravity: 1.0,
        force: 0.0,
        spread: 0.0,
        focus: 0.0,
        range: 0.0,
        duration: 0.0,
        stability: 0.0,
        quality: 0.0,
        neatness: glyph_ast.global_metrics.neatness,
        warnings: dedup_warnings(&glyph_ast.warnings, extra_warnings),
        signature: format!("invalid:{status}:{ring_complete}:{}", glyph_ast.ring.completeness),
    }
}

fn calculate_spell_gravity(manifestation_influence: &HashMap<String, f64>) -> f64 {
    clamp(1.0 - manifestation_influence.get("levitation").copied().unwrap_or(0.0) * LEVITATION_GRAVITY_SCALE, 0.0, 1.0)
}

fn manifestation_signature(manifestations: &HashMap<String, Manifestation>) -> String {
    let mut parts: Vec<String> = manifestations
        .iter()
        .map(|(id, m)| {
            let point_part = m.convergence.map(|c| format!(".p{}.{}", (c.point.x * 100.0).round(), (c.point.y * 100.0).round())).unwrap_or_default();
            let radius_part = m.convergence.map(|c| format!(".r{}", (c.radius * 100.0).round())).unwrap_or_default();
            format!("{id}.{}{point_part}{radius_part}", (m.strength * 100.0).round())
        })
        .collect();
    parts.sort();
    parts.join(",")
}

struct DurationInput<'a> {
    primary_semantic: &'a SemanticFields,
    deltas: &'a SemanticDeltas,
    quality: f64,
    neatness: f64,
}

fn calculate_spell_duration(input: DurationInput) -> f64 {
    let duration_score = clamp(
        input.quality * SpellParameterTuning::DURATION_QUALITY_WEIGHT
            + input.neatness * SpellParameterTuning::DURATION_NEATNESS_WEIGHT
            + input.primary_semantic.lifetime_bias.unwrap_or(0.0)
            + input.deltas.lifetime_bias,
        0.0,
        1.0,
    );

    clamp(
        SpellParameterTuning::DURATION_MIN_SECONDS + duration_score.powf(SpellParameterTuning::DURATION_CURVE) * SpellParameterTuning::DURATION_SECONDS_SCALE,
        SpellParameterTuning::DURATION_MIN_SECONDS,
        SpellParameterTuning::DURATION_MAX_SECONDS,
    )
}

pub fn compile_spell(glyph_ast: &GlyphAst, compiler_config: &CompilerConfig, effect_size_config: &EffectSizeConfig) -> SpellIr {
    if !glyph_ast.ring.found {
        return invalid_spell("No ring detected", glyph_ast, &[]);
    }

    if !glyph_ast.ring.unsupported_multiple_rings.is_empty() {
        return invalid_spell("Multiple rings detected", glyph_ast, &[GlyphWarning::UnsupportedMultipleRings]);
    }

    if !glyph_ast.unsupported_multiple_sigils.is_empty() {
        return invalid_spell("Multiple sigils detected", glyph_ast, &[GlyphWarning::UnsupportedMultipleSigils]);
    }

    let Some(primary) = glyph_ast.primary_sigil.as_ref() else {
        return invalid_spell("Invalid spell", glyph_ast, &[GlyphWarning::MissingPrimarySigil]);
    };

    if primary.confidence < compiler_config.minimum_primary_sigil_confidence {
        return invalid_spell("Invalid spell", glyph_ast, &[GlyphWarning::PrimarySigilConfidenceLow]);
    }

    let confidence_gap = primary.confidence - same_kind_alternate_confidence(primary);
    if confidence_gap < PRIMARY_SIGIL_AMBIGUITY_GAP {
        return invalid_spell("Ambiguous sigil", glyph_ast, &[GlyphWarning::PrimarySigilAmbiguous]);
    }

    let Some(element) = primary.element.as_ref() else {
        return invalid_spell("Unsupported element", glyph_ast, &[GlyphWarning::PrimaryElementMissing]);
    };

    if !SUPPORTED_ELEMENTS.contains(&element.as_str()) {
        return invalid_spell("Unsupported element", glyph_ast, &[GlyphWarning::PrimaryElementUnsupported]);
    }

    let signs = &glyph_ast.signs;
    let quality = calculate_spell_quality(glyph_ast);
    let stability = calculate_spell_stability(glyph_ast, compiler_config);
    let neatness = glyph_ast.global_metrics.neatness;
    let manifestation_aggregate = aggregate_manifestations(signs);
    let deltas = aggregate_semantic_deltas(signs);
    let surface_direction = if !signs.is_empty() { combine_sign_direction(signs) } else { SurfaceDirection { x: 0.0, y: 0.0, strength: 0.0 } };
    let direction_coherence = surface_direction.strength;
    let sign_power: f64 = signs.iter().map(sign_influence).sum();
    let active = glyph_ast.ring.complete;
    let prepared = !active;
    let default_semantic = SemanticFields::default();
    let primary_semantic = primary.semantic.as_ref().unwrap_or(&default_semantic);
    let effect_scale = clamp(
        effect_size_config.base_scale + primary.size_norm * effect_size_config.sigil_size_influence,
        effect_size_config.min_scale,
        effect_size_config.max_scale,
    );

    let focus = clamp(SpellParameterTuning::FOCUS_BASE + primary_semantic.focus.unwrap_or(0.0) + deltas.focus + quality * SpellParameterTuning::FOCUS_QUALITY, 0.0, 1.0);
    let spread = clamp(
        SpellParameterTuning::SPREAD_BASE + primary_semantic.spread.unwrap_or(0.0) + deltas.spread + (1.0 - focus) * SpellParameterTuning::SPREAD_INVERSE_FOCUS,
        0.0,
        1.0,
    );
    let force = clamp(
        SpellParameterTuning::FORCE_BASE + primary_semantic.force.unwrap_or(0.0) + sign_power * SpellParameterTuning::FORCE_SIGN_POWER + deltas.force + quality * SpellParameterTuning::FORCE_QUALITY,
        0.0,
        1.0,
    );
    let range = clamp(SpellParameterTuning::RANGE_BASE + primary_semantic.range.unwrap_or(0.0) + deltas.range + sign_power * SpellParameterTuning::RANGE_SIGN_POWER, 0.0, 1.0);
    let duration = calculate_spell_duration(DurationInput { primary_semantic, deltas: &deltas, quality, neatness });
    let direction = direction_from_surface_vector((surface_direction.x, surface_direction.y), force);
    let gravity = calculate_spell_gravity(&manifestation_aggregate.manifestation_influence);

    let signature = format!(
        "{}:{}:{active}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        primary.id.as_deref().unwrap_or(""),
        manifestation_signature(&manifestation_aggregate.manifestations),
        (effect_scale * 100.0).round(),
        (force * 100.0).round(),
        (spread * 100.0).round(),
        (duration * 100.0).round(),
        direction.x_tilt_deg.round(),
        direction.y_tilt_deg.round(),
        (direction_coherence * 100.0).round(),
        (gravity * 100.0).round(),
        (quality * 100.0).round(),
        (stability * 100.0).round(),
    );

    SpellIr {
        active,
        prepared,
        valid: true,
        status: if active { "Active spell".to_string() } else { "Prepared spell".to_string() },
        element: Some(element.clone()),
        element_confidence: primary.confidence,
        primary_size_norm: primary.size_norm,
        effect_scale,
        primary_manifestation: manifestation_aggregate.primary_manifestation,
        manifestations: manifestation_aggregate.manifestations,
        direction,
        direction_coherence,
        gravity,
        force,
        spread,
        focus,
        range,
        duration,
        stability,
        quality,
        neatness,
        warnings: glyph_ast.warnings.clone(),
        signature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{COMPILER, EFFECT_SIZE};
    use crate::drawing_classifier::GlobalMetrics;
    use crate::layer_mapper::Layer;
    use crate::ring_detector::Ring;
    use crate::stroke_grouper::RadialFacing;
    use crate::symbol_recognizer::{RecognitionDiagnostics, RecognitionShape, RecognitionStatus};

    fn valid_primary_sigil() -> Recognition {
        Recognition {
            candidate_id: "c1".into(),
            layer: Layer::Center,
            near_boundary: false,
            radius_norm: 0.1,
            angle_deg: 0.0,
            size_norm: 0.2,
            length_norm: 0.2,
            orientation_deg: 0.0,
            directed_orientation_deg: 0.0,
            radial_facing: RadialFacing::Outward,
            neatness: 0.85,
            recognized: true,
            recognition_status: RecognitionStatus::Valid,
            kind: "sigil".into(),
            id: Some("fire".into()),
            display_name: Some("Fire".into()),
            element: Some("fire".into()),
            semantic: None,
            confidence: 0.95,
            shape: RecognitionShape { stroke_count: 1, aspect_ratio: 1.0, elongation: 1.0, elongation_norm: 0.0, stroke_length_imbalance: 0.0, axis_dominance: 0.0 },
            diagnostics: RecognitionDiagnostics { best_guess: None, recognition_rotation_deg: 0.0, top_matches: vec![] },
        }
    }

    fn glyph_ast_with(ring_complete: bool, primary_sigil: Option<Recognition>) -> GlyphAst {
        GlyphAst {
            version: "test".into(),
            ring: Ring { found: true, complete: ring_complete, completeness: if ring_complete { 1.0 } else { 0.8 }, neatness: 0.8, ..Default::default() },
            candidates: vec![],
            primary_sigil,
            unsupported_multiple_sigils: vec![],
            signs: vec![],
            unknowns: vec![],
            global_metrics: GlobalMetrics { neatness: 0.8, radial_symmetry: 0.9, instability: 0.1 },
            warnings: vec![],
        }
    }

    #[test]
    fn no_ring_is_invalid() {
        let glyph_ast = GlyphAst {
            version: "test".into(),
            ring: Ring::default(),
            candidates: vec![],
            primary_sigil: None,
            unsupported_multiple_sigils: vec![],
            signs: vec![],
            unknowns: vec![],
            global_metrics: GlobalMetrics { neatness: 0.0, radial_symmetry: 0.0, instability: 1.0 },
            warnings: vec![GlyphWarning::NoRingDetected],
        };
        let spell = compile_spell(&glyph_ast, &COMPILER, &EFFECT_SIZE);
        assert!(!spell.valid);
        assert_eq!(spell.status, "No ring detected");
    }

    #[test]
    fn missing_primary_sigil_is_invalid() {
        let glyph_ast = glyph_ast_with(false, None);
        let spell = compile_spell(&glyph_ast, &COMPILER, &EFFECT_SIZE);
        assert!(!spell.valid);
        assert!(spell.warnings.contains(&GlyphWarning::MissingPrimarySigil));
    }

    #[test]
    fn low_confidence_primary_is_invalid() {
        let mut sigil = valid_primary_sigil();
        sigil.confidence = 0.1; // below minimum_primary_sigil_confidence (0.62)
        let glyph_ast = glyph_ast_with(false, Some(sigil));
        let spell = compile_spell(&glyph_ast, &COMPILER, &EFFECT_SIZE);
        assert!(!spell.valid);
        assert!(spell.warnings.contains(&GlyphWarning::PrimarySigilConfidenceLow));
    }

    #[test]
    fn valid_sigil_with_open_ring_is_prepared_not_active() {
        let glyph_ast = glyph_ast_with(false, Some(valid_primary_sigil()));
        let spell = compile_spell(&glyph_ast, &COMPILER, &EFFECT_SIZE);
        assert!(spell.valid, "warnings: {:?}", spell.warnings);
        assert!(spell.prepared);
        assert!(!spell.active);
        assert_eq!(spell.status, "Prepared spell");
        assert_eq!(spell.element.as_deref(), Some("fire"));
    }

    #[test]
    fn valid_sigil_with_closed_ring_is_active() {
        let glyph_ast = glyph_ast_with(true, Some(valid_primary_sigil()));
        let spell = compile_spell(&glyph_ast, &COMPILER, &EFFECT_SIZE);
        assert!(spell.valid);
        assert!(spell.active);
        assert!(!spell.prepared);
        assert_eq!(spell.status, "Active spell");
        // No signs -> aura manifestation, no levitation -> full gravity.
        assert_eq!(spell.primary_manifestation, "aura");
        assert_eq!(spell.gravity, 1.0);
    }

    #[test]
    fn unsupported_element_is_invalid() {
        let mut sigil = valid_primary_sigil();
        sigil.element = Some("void".into()); // not in SUPPORTED_ELEMENTS
        let glyph_ast = glyph_ast_with(true, Some(sigil));
        let spell = compile_spell(&glyph_ast, &COMPILER, &EFFECT_SIZE);
        assert!(!spell.valid);
        assert!(spell.warnings.contains(&GlyphWarning::PrimaryElementUnsupported));
    }
}
