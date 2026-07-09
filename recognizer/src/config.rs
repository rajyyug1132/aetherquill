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

pub const LAYERS: LayersConfig = LayersConfig {
    center_max: 0.32,
    middle_max: 0.66,
    outer_max: 0.94,
    boundary_max: 1.06,
    boundary_tolerance: 0.055,
};
