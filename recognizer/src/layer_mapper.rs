//! Direct port of service/vendor/wha/src/parser/layerMapper.js.

use crate::config::LayersConfig;
use crate::geometry::clamp01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Center,
    Middle,
    Outer,
    RingBoundary,
    Outside,
}

impl Layer {
    /// The JS string name, for parity fixtures and wire output.
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Center => "center",
            Layer::Middle => "middle",
            Layer::Outer => "outer",
            Layer::RingBoundary => "ringBoundary",
            Layer::Outside => "outside",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerInfo {
    pub layer: Layer,
    pub near_boundary: bool,
}

pub fn map_radius_to_layer(radius_norm: f64, layers: &LayersConfig) -> LayerInfo {
    let layer = if radius_norm <= layers.center_max {
        Layer::Center
    } else if radius_norm <= layers.middle_max {
        Layer::Middle
    } else if radius_norm <= layers.outer_max {
        Layer::Outer
    } else if radius_norm <= layers.boundary_max {
        Layer::RingBoundary
    } else {
        Layer::Outside
    };

    let boundaries = [0.0, layers.center_max, layers.middle_max, layers.outer_max, layers.boundary_max];
    let nearest_boundary_distance = boundaries
        .iter()
        .map(|b| (radius_norm - b).abs())
        .fold(f64::INFINITY, f64::min);
    let boundary_distance_score = clamp01(nearest_boundary_distance / (0.001_f64).max(layers.boundary_tolerance));

    LayerInfo { layer, near_boundary: boundary_distance_score < 0.55 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LAYERS;

    #[test]
    fn layer_thresholds_match_js() {
        assert_eq!(map_radius_to_layer(0.0, &LAYERS).layer, Layer::Center);
        assert_eq!(map_radius_to_layer(0.32, &LAYERS).layer, Layer::Center);
        assert_eq!(map_radius_to_layer(0.33, &LAYERS).layer, Layer::Middle);
        assert_eq!(map_radius_to_layer(0.66, &LAYERS).layer, Layer::Middle);
        assert_eq!(map_radius_to_layer(0.9, &LAYERS).layer, Layer::Outer);
        assert_eq!(map_radius_to_layer(1.0, &LAYERS).layer, Layer::RingBoundary);
        assert_eq!(map_radius_to_layer(1.2, &LAYERS).layer, Layer::Outside);
    }

    #[test]
    fn near_boundary_flags_points_close_to_layer_edges() {
        // Right on a boundary → distance 0 → score 0 → near.
        assert!(map_radius_to_layer(0.32, &LAYERS).near_boundary);
        // Mid-layer, far from every boundary → not near.
        assert!(!map_radius_to_layer(0.49, &LAYERS).near_boundary);
    }
}
