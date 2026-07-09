//! Direct port of service/vendor/wha/src/parser/coordinateNormalizer.js.

use crate::config::LayersConfig;
use crate::geometry::{angle_deg_from_center, distance, Point};
use crate::layer_mapper::{map_radius_to_layer, LayerInfo};
pub use crate::ring_detector::Ring;
use crate::stroke_cleaner::CleanedStroke;
use std::collections::HashSet;

/// Ring-relative measurements for one point. The JS spreads the original
/// point in too; nothing downstream reads those copies, so they're omitted.
#[derive(Debug, Clone, Copy)]
pub struct NormalizedPoint {
    pub radius_norm: f64,
    pub angle_deg: f64,
    pub centered_x: f64,
    pub centered_y: f64,
}

// radiusNorm is the point's distance as a fraction of the detected ring
// radius, and centeredY is flipped so positive values behave like an upward
// math axis.
fn normalize_point(point: Point, ring: &Ring) -> NormalizedPoint {
    NormalizedPoint {
        radius_norm: distance(point, ring.center) / (1.0_f64).max(ring.radius),
        angle_deg: angle_deg_from_center(point, ring.center),
        centered_x: point.x - ring.center.x,
        centered_y: ring.center.y - point.y,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeClass {
    Unbounded,
    RingInk,
    Inside,
    Outside,
    BoundaryCrossing,
    BoundaryNear,
}

impl StrokeClass {
    pub fn as_str(self) -> &'static str {
        match self {
            StrokeClass::Unbounded => "unbounded",
            StrokeClass::RingInk => "ring",
            StrokeClass::Inside => "inside",
            StrokeClass::Outside => "outside",
            StrokeClass::BoundaryCrossing => "boundary-crossing",
            StrokeClass::BoundaryNear => "boundary-near",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StrokeClassification {
    pub stroke_id: String,
    pub classification: StrokeClass,
    pub inside_ratio: f64,
    pub outside_ratio: f64,
    pub boundary_ratio: f64,
    pub used_by_parser: bool,
    pub can_join_symbol: bool,
}

// Classify each stroke by where its points sit relative to the detected ring.
// Ring strokes are reserved as boundary ink, inside strokes can become
// symbols, and mostly outside or crossing strokes are kept out of grouping.
pub fn classify_strokes_against_ring(
    strokes: &[CleanedStroke],
    ring: &Ring,
    layers: &LayersConfig,
) -> Vec<StrokeClassification> {
    if !ring.found {
        return strokes
            .iter()
            .map(|stroke| StrokeClassification {
                stroke_id: stroke.id.clone(),
                classification: StrokeClass::Unbounded,
                inside_ratio: 0.0,
                outside_ratio: 0.0,
                boundary_ratio: 0.0,
                used_by_parser: false,
                can_join_symbol: false,
            })
            .collect();
    }

    let ring_stroke_ids: HashSet<&str> = ring.stroke_ids.iter().map(String::as_str).collect();

    strokes
        .iter()
        .map(|stroke| {
            if ring_stroke_ids.contains(stroke.id.as_str()) {
                return StrokeClassification {
                    stroke_id: stroke.id.clone(),
                    classification: StrokeClass::RingInk,
                    inside_ratio: 0.0,
                    outside_ratio: 0.0,
                    boundary_ratio: 1.0,
                    used_by_parser: false,
                    can_join_symbol: false,
                };
            }

            let normalized: Vec<NormalizedPoint> =
                stroke.points.iter().map(|p| normalize_point(*p, ring)).collect();
            let denom = (1usize).max(normalized.len()) as f64;
            let inside_ratio =
                normalized.iter().filter(|p| p.radius_norm < layers.outer_max).count() as f64 / denom;
            let boundary_ratio = normalized
                .iter()
                .filter(|p| {
                    p.radius_norm >= layers.outer_max - layers.boundary_tolerance
                        && p.radius_norm <= layers.boundary_max
                })
                .count() as f64
                / denom;
            let outside_ratio =
                normalized.iter().filter(|p| p.radius_norm > layers.boundary_max).count() as f64 / denom;

            // These ratios deliberately leave a little tolerance around the outer
            // layer so near-boundary sign strokes can still join symbols when they
            // are mostly on the paper instead of being treated as stray outside ink.
            let mut classification = StrokeClass::Inside;
            if outside_ratio > 0.62 {
                classification = StrokeClass::Outside;
            } else if inside_ratio > 0.12 && outside_ratio > 0.18 {
                classification = StrokeClass::BoundaryCrossing;
            } else if boundary_ratio > 0.55 && inside_ratio < 0.45 {
                classification = StrokeClass::BoundaryNear;
            }
            let used_by_parser = classification == StrokeClass::Inside && inside_ratio >= 0.45;

            StrokeClassification {
                stroke_id: stroke.id.clone(),
                classification,
                inside_ratio,
                outside_ratio,
                boundary_ratio,
                used_by_parser,
                can_join_symbol: used_by_parser
                    || (classification == StrokeClass::BoundaryNear && outside_ratio <= 0.08),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub struct PolarSummary {
    pub radius_norm: f64,
    pub angle_deg: f64,
    pub layer_info: LayerInfo,
}

pub fn summarize_polar(point: Point, ring: &Ring, layers: &LayersConfig) -> PolarSummary {
    let normalized = normalize_point(point, ring);
    PolarSummary {
        radius_norm: normalized.radius_norm,
        angle_deg: normalized.angle_deg,
        layer_info: map_radius_to_layer(normalized.radius_norm, layers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{INPUT, LAYERS};
    use crate::stroke_cleaner::{clean_strokes, RawStroke};

    fn ring_at(cx: f64, cy: f64, radius: f64) -> Ring {
        Ring { found: true, center: Point { x: cx, y: cy }, radius, stroke_ids: vec!["ring".into()], ..Default::default() }
    }

    #[test]
    fn no_ring_classifies_everything_unbounded() {
        let ring = Ring { found: false, center: Point { x: 0.0, y: 0.0 }, radius: 0.0, stroke_ids: vec![], ..Default::default() };
        let strokes = vec![CleanedStroke {
            id: "s1".into(),
            points: vec![Point { x: 0.0, y: 0.0 }],
            metrics: crate::stroke_cleaner::StrokeMetrics {
                length: 0.0,
                bounds: crate::geometry::bounds_for_points(&[]),
                point_count: 1,
            },
        }];
        let out = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        assert_eq!(out[0].classification, StrokeClass::Unbounded);
        assert!(!out[0].can_join_symbol);
    }

    #[test]
    fn ring_stroke_is_reserved_as_boundary_ink() {
        let ring = ring_at(0.0, 0.0, 100.0);
        let strokes = vec![CleanedStroke {
            id: "ring".into(),
            points: vec![Point { x: 100.0, y: 0.0 }],
            metrics: crate::stroke_cleaner::StrokeMetrics {
                length: 0.0,
                bounds: crate::geometry::bounds_for_points(&[]),
                point_count: 1,
            },
        }];
        let out = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        assert_eq!(out[0].classification, StrokeClass::RingInk);
        assert_eq!(out[0].boundary_ratio, 1.0);
    }

    #[test]
    fn centered_stroke_is_inside_and_parser_usable() {
        let ring = ring_at(0.0, 0.0, 100.0);
        let raw = vec![RawStroke {
            id: "s".into(),
            points: (0..10).map(|i| Point { x: i as f64 * 2.0, y: 0.0 }).collect(),
        }];
        let strokes = clean_strokes(&raw, &INPUT);
        let out = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        assert_eq!(out[0].classification, StrokeClass::Inside);
        assert!(out[0].used_by_parser);
        assert!(out[0].can_join_symbol);
    }

    #[test]
    fn far_outside_stroke_is_outside() {
        let ring = ring_at(0.0, 0.0, 100.0);
        let raw = vec![RawStroke {
            id: "s".into(),
            points: (0..10).map(|i| Point { x: 300.0 + i as f64 * 2.0, y: 300.0 }).collect(),
        }];
        let strokes = clean_strokes(&raw, &INPUT);
        let out = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        assert_eq!(out[0].classification, StrokeClass::Outside);
        assert!(!out[0].can_join_symbol);
    }

    // --- parity against the JS pipeline fixtures ---

    #[test]
    fn parity_with_js_classifications() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/pipeline.json"))
            .expect("fixtures/pipeline.json — regenerate with: node service/parity-gen.mjs");
        let scenarios: serde_json::Value = serde_json::from_str(&raw).unwrap();

        let mut checked = 0;
        for scenario in scenarios.as_array().unwrap() {
            let name = scenario["name"].as_str().unwrap();
            let ring_js = &scenario["ring"];
            let ring = Ring {
                found: ring_js["found"].as_bool().unwrap_or(false),
                center: Point {
                    x: ring_js["center"]["x"].as_f64().unwrap_or(0.0),
                    y: ring_js["center"]["y"].as_f64().unwrap_or(0.0),
                },
                radius: ring_js["radius"].as_f64().unwrap_or(0.0),
                stroke_ids: ring_js["strokeIds"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
                    .unwrap_or_default(),
                ..Default::default()
            };
            let strokes: Vec<CleanedStroke> = scenario["cleanedStrokes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    let points: Vec<Point> = s["points"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| Point { x: p["x"].as_f64().unwrap(), y: p["y"].as_f64().unwrap() })
                        .collect();
                    CleanedStroke {
                        id: s["id"].as_str().unwrap().to_string(),
                        metrics: crate::stroke_cleaner::StrokeMetrics {
                            length: 0.0,
                            bounds: crate::geometry::bounds_for_points(&points),
                            point_count: points.len(),
                        },
                        points,
                    }
                })
                .collect();

            let ours = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
            let expected = scenario["classifications"].as_array().unwrap();
            assert_eq!(ours.len(), expected.len(), "{name}: classification count");

            for (mine, theirs) in ours.iter().zip(expected) {
                let ctx = format!("{name}/{}", mine.stroke_id);
                assert_eq!(mine.stroke_id, theirs["strokeId"].as_str().unwrap(), "{ctx}: strokeId");
                assert_eq!(
                    mine.classification.as_str(),
                    theirs["classification"].as_str().unwrap(),
                    "{ctx}: classification"
                );
                // JS fixture values are roundedDeep(3 digits) → tolerance 2e-3.
                for (label, mine_v, theirs_v) in [
                    ("insideRatio", mine.inside_ratio, &theirs["insideRatio"]),
                    ("outsideRatio", mine.outside_ratio, &theirs["outsideRatio"]),
                    ("boundaryRatio", mine.boundary_ratio, &theirs["boundaryRatio"]),
                ] {
                    let t = theirs_v.as_f64().unwrap();
                    assert!((mine_v - t).abs() < 2e-3, "{ctx}: {label} ours={mine_v} js={t}");
                }
                assert_eq!(mine.used_by_parser, theirs["usedByParser"].as_bool().unwrap(), "{ctx}: usedByParser");
                assert_eq!(mine.can_join_symbol, theirs["canJoinSymbol"].as_bool().unwrap(), "{ctx}: canJoinSymbol");
                checked += 1;
            }
        }
        assert!(checked > 20, "expected to parity-check many strokes, got {checked}");
    }
}
