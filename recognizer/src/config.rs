//! Port of the `input` section of service/vendor/wha/src/config.js.
//! Extend with more sections (ring/layers/recognition/compiler) as later
//! modules get ported — ponytail: only what's used today.

pub struct InputConfig {
    pub smoothing_passes: u32,
    pub min_stroke_length: f64,
}

pub const INPUT: InputConfig = InputConfig { smoothing_passes: 1, min_stroke_length: 7.0 };
