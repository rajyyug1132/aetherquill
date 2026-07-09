//! Port of the `input` section of service/vendor/wha/src/config.js.
//! Extend with more sections (ring/layers/recognition/compiler) as later
//! modules get ported — ponytail: only what's used today.

pub struct InputConfig {
    pub smoothing_passes: u32,
    pub min_stroke_length: f64,
}

pub const INPUT: InputConfig = InputConfig { smoothing_passes: 1, min_stroke_length: 7.0 };

/// `layers` section of config.js — normalized-radius rings of the spell paper.
pub struct LayersConfig {
    pub center_max: f64,
    pub middle_max: f64,
    pub outer_max: f64,
    pub boundary_max: f64,
    pub boundary_tolerance: f64,
}

/// `ring` section of config.js.
pub struct RingConfig {
    pub min_radius: f64,
}

pub const RING: RingConfig = RingConfig { min_radius: 70.0 };

pub const LAYERS: LayersConfig = LayersConfig {
    center_max: 0.32,
    middle_max: 0.66,
    outer_max: 0.94,
    boundary_max: 1.06,
    boundary_tolerance: 0.055,
};

/// `recognition` section of config.js.
pub const RECOGNITION: crate::symbol_recognizer::RecognitionConfig =
    crate::symbol_recognizer::RecognitionConfig { min_confidence: 0.48 };

/// `compiler` section of config.js.
pub struct CompilerConfig {
    pub minimum_primary_sigil_confidence: f64,
    pub max_unknowns_before_instability: f64,
}

pub const COMPILER: CompilerConfig = CompilerConfig { minimum_primary_sigil_confidence: 0.62, max_unknowns_before_instability: 4.0 };

/// `renderer.effectSize` section of config.js — the only renderer field the
/// compiler reads. inkColor/guideColor/particleBaseCount/particleCap are
/// rendering-only and belong to the device crate, not this one.
pub struct EffectSizeConfig {
    pub base_scale: f64,
    pub sigil_size_influence: f64,
    pub min_scale: f64,
    pub max_scale: f64,
}

pub const EFFECT_SIZE: EffectSizeConfig =
    EffectSizeConfig { base_scale: 1.28, sigil_size_influence: 2.1, min_scale: 1.0, max_scale: 2.35 };
