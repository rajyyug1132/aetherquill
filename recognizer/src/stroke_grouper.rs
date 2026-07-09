//! Direct port of service/vendor/wha/src/parser/strokeGrouper.js.

use crate::config::LayersConfig;
use crate::coordinate_normalizer::{summarize_polar, Ring, StrokeClassification};
use crate::geometry::{
    all_points, angular_difference, bounds_for_point_lists, bounds_overlap, center_of_bounds, clamp01,
    directed_stroke_angle, distance, dominant_axis_orientation_deg, endpoint_closedness, expand_bounds, path_length,
    Bounds, Point,
};
use crate::layer_mapper::Layer;
use crate::stroke_cleaner::CleanedStroke;
use std::collections::{HashMap, HashSet};

const BBOX_PADDING_NORM: f64 = 0.075;
const CENTER_DISTANCE_NORM: f64 = 0.2;
const ENDPOINT_DISTANCE_NORM: f64 = 0.085;
const MAX_SYMBOL_SIZE_NORM: f64 = 0.52;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialFacing {
    Outward,
    Inward,
    Counterclockwise,
    Clockwise,
    Unclear,
}

impl RadialFacing {
    pub fn as_str(self) -> &'static str {
        match self {
            RadialFacing::Outward => "outward",
            RadialFacing::Inward => "inward",
            RadialFacing::Counterclockwise => "counterclockwise",
            RadialFacing::Clockwise => "clockwise",
            RadialFacing::Unclear => "unclear",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub candidate_id: String,
    pub stroke_ids: Vec<String>,
    pub raw_stroke_count: usize,
    pub cleaned_stroke_count: usize,
    pub bounds: Bounds,
    pub center: Point,
    pub radius_norm: f64,
    pub angle_deg: f64,
    pub layer: Layer,
    pub near_boundary: bool,
    pub size_norm: f64,
    pub length_norm: f64,
    pub orientation_deg: f64,
    pub directed_orientation_deg: f64,
    pub radial_facing: RadialFacing,
    pub closedness: f64,
    pub overdraw_amount: f64,
    pub neatness: f64,
    pub strokes: Vec<CleanedStroke>,
}

fn endpoint_distance(a: &CleanedStroke, b: &CleanedStroke) -> f64 {
    let endpoints_a = [a.points.first(), a.points.last()];
    let endpoints_b = [b.points.first(), b.points.last()];
    let mut best = f64::INFINITY;
    for pa in endpoints_a.into_iter().flatten() {
        for pb in endpoints_b.into_iter().flatten() {
            best = best.min(distance(*pa, *pb));
        }
    }
    best
}

fn should_group(a: &CleanedStroke, b: &CleanedStroke, ring: &Ring) -> bool {
    let padding = ring.radius * BBOX_PADDING_NORM;
    let a_bounds = expand_bounds(a.metrics.bounds, padding);
    let b_bounds = expand_bounds(b.metrics.bounds, padding);
    let centers_close =
        distance(center_of_bounds(a.metrics.bounds), center_of_bounds(b.metrics.bounds)) <= ring.radius * CENTER_DISTANCE_NORM;
    let endpoints_close = endpoint_distance(a, b) <= ring.radius * ENDPOINT_DISTANCE_NORM;
    bounds_overlap(a_bounds, b_bounds) || centers_close || endpoints_close
}

fn classify_radial_facing(directed_angle: f64, radial_angle: f64) -> RadialFacing {
    let outward = angular_difference(directed_angle, radial_angle);
    let inward = angular_difference(directed_angle, radial_angle + 180.0);
    let counterclockwise = angular_difference(directed_angle, radial_angle + 90.0);
    let clockwise = angular_difference(directed_angle, radial_angle - 90.0);
    let best = outward.min(inward).min(counterclockwise).min(clockwise);

    if best > 48.0 {
        RadialFacing::Unclear
    } else if best == outward {
        RadialFacing::Outward
    } else if best == inward {
        RadialFacing::Inward
    } else if best == counterclockwise {
        RadialFacing::Counterclockwise
    } else {
        RadialFacing::Clockwise
    }
}

fn build_candidate(strokes: Vec<CleanedStroke>, index: usize, ring: &Ring, layers: &LayersConfig) -> Candidate {
    let point_lists: Vec<Vec<Point>> = strokes.iter().map(|s| s.points.clone()).collect();
    let points = all_points(&point_lists);
    let bounds = bounds_for_point_lists(&point_lists);
    let center = center_of_bounds(bounds);
    let polar = summarize_polar(center, ring, layers);
    let length: f64 = strokes.iter().map(|s| path_length(&s.points)).sum();
    let size = bounds.width.max(bounds.height);
    let size_norm = size / (1.0_f64).max(ring.radius * 2.0);
    let length_norm = length / (1.0_f64).max(std::f64::consts::PI * 2.0 * ring.radius);
    let orientation_deg = dominant_axis_orientation_deg(&points);
    let directed_orientation_deg = directed_stroke_angle(&point_lists);
    let radial_facing = classify_radial_facing(directed_orientation_deg, polar.angle_deg);
    let compact_perimeter = (1.0_f64).max((bounds.width + bounds.height) * 2.0);
    let overdraw_amount = clamp01(length / compact_perimeter - 0.72).max(0.0).min(1.0);
    let closedness = endpoint_closedness(&point_lists, size.max(1.0));
    let stroke_count = strokes.len();

    Candidate {
        candidate_id: format!("c{}", index + 1),
        stroke_ids: strokes.iter().map(|s| s.id.clone()).collect(),
        raw_stroke_count: stroke_count,
        cleaned_stroke_count: stroke_count,
        bounds,
        center,
        radius_norm: polar.radius_norm,
        angle_deg: polar.angle_deg,
        layer: polar.layer_info.layer,
        near_boundary: polar.layer_info.near_boundary,
        size_norm,
        length_norm,
        orientation_deg,
        directed_orientation_deg,
        radial_facing,
        closedness,
        overdraw_amount,
        neatness: clamp01(0.92 - overdraw_amount * 0.28 - (0.0_f64).max(stroke_count as f64 - 4.0) * 0.035),
        strokes,
    }
}

pub fn build_symbol_candidates(
    strokes: &[CleanedStroke],
    classifications: &[StrokeClassification],
    ring: &Ring,
    layers: &LayersConfig,
) -> Vec<Candidate> {
    if !ring.found {
        return vec![];
    }

    let classification_by_id: HashMap<&str, &StrokeClassification> =
        classifications.iter().map(|c| (c.stroke_id.as_str(), c)).collect();

    let seed_ids: HashSet<&str> = strokes
        .iter()
        .filter(|s| classification_by_id.get(s.id.as_str()).map(|c| c.used_by_parser).unwrap_or(false))
        .map(|s| s.id.as_str())
        .collect();
    let joinable: Vec<&CleanedStroke> = strokes
        .iter()
        .filter(|s| classification_by_id.get(s.id.as_str()).map(|c| c.can_join_symbol).unwrap_or(false))
        .collect();

    let mut visited: HashSet<String> = HashSet::new();
    let mut groups: Vec<Vec<CleanedStroke>> = Vec::new();

    for stroke in strokes.iter().filter(|s| seed_ids.contains(s.id.as_str())) {
        if visited.contains(&stroke.id) {
            continue;
        }

        let mut group: Vec<CleanedStroke> = Vec::new();
        let mut queue: Vec<CleanedStroke> = vec![stroke.clone()];
        visited.insert(stroke.id.clone());

        while let Some(current) = queue.first().cloned() {
            queue.remove(0);
            for other in &joinable {
                if visited.contains(&other.id) {
                    continue;
                }
                if should_group(&current, other, ring) {
                    visited.insert(other.id.clone());
                    queue.push((*other).clone());
                }
            }
            group.push(current);
        }

        groups.push(group);
    }

    groups
        .into_iter()
        .enumerate()
        .map(|(index, group)| build_candidate(group, index, ring, layers))
        .filter(|candidate| candidate.size_norm <= MAX_SYMBOL_SIZE_NORM)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{INPUT, LAYERS};
    use crate::coordinate_normalizer::classify_strokes_against_ring;
    use crate::stroke_cleaner::{clean_strokes, RawStroke};

    fn ring_at(cx: f64, cy: f64, radius: f64) -> Ring {
        Ring { found: true, center: Point { x: cx, y: cy }, radius, stroke_ids: vec!["ring".into()], ..Default::default() }
    }

    fn line_stroke(id: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> RawStroke {
        let n = 10;
        let points = (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                Point { x: x0 + (x1 - x0) * t, y: y0 + (y1 - y0) * t }
            })
            .collect();
        RawStroke { id: id.into(), points }
    }

    #[test]
    fn no_ring_yields_no_candidates() {
        let ring = Ring { found: false, center: Point { x: 0.0, y: 0.0 }, radius: 0.0, stroke_ids: vec![], ..Default::default() };
        let out = build_symbol_candidates(&[], &[], &ring, &LAYERS);
        assert!(out.is_empty());
    }

    #[test]
    fn single_centered_stroke_becomes_one_candidate() {
        let ring = ring_at(0.0, 0.0, 100.0);
        let raw = vec![line_stroke("s1", -10.0, 0.0, 10.0, 0.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let classifications = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        let candidates = build_symbol_candidates(&strokes, &classifications, &ring, &LAYERS);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].stroke_ids, vec!["s1"]);
        assert_eq!(candidates[0].layer, Layer::Center);
    }

    #[test]
    fn nearby_strokes_join_one_candidate() {
        let ring = ring_at(0.0, 0.0, 100.0);
        let raw = vec![line_stroke("s1", -10.0, 0.0, 0.0, 0.0), line_stroke("s2", 0.5, 0.0, 10.0, 0.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let classifications = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        let candidates = build_symbol_candidates(&strokes, &classifications, &ring, &LAYERS);
        assert_eq!(candidates.len(), 1, "adjacent endpoints should join into one group");
        assert_eq!(candidates[0].stroke_ids.len(), 2);
    }

    #[test]
    fn oversized_group_is_filtered_out() {
        let ring = ring_at(0.0, 0.0, 100.0);
        // A stroke spanning nearly the whole ring diameter exceeds MAX_SYMBOL_SIZE_NORM.
        let raw = vec![line_stroke("s1", -95.0, 0.0, 95.0, 0.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let classifications = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
        let candidates = build_symbol_candidates(&strokes, &classifications, &ring, &LAYERS);
        assert!(candidates.is_empty(), "oversized candidate should be filtered by MAX_SYMBOL_SIZE_NORM");
    }

    // --- parity against the JS pipeline fixtures ---

    #[test]
    fn parity_with_js_candidates() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/pipeline.json"))
            .expect("fixtures/pipeline.json — regenerate with: node service/parity-gen.mjs");
        let scenarios: serde_json::Value = serde_json::from_str(&raw).unwrap();

        let mut checked = 0;
        for scenario in scenarios.as_array().unwrap() {
            let name = scenario["name"].as_str().unwrap();
            let ring_js = &scenario["ring"];
            if !ring_js["found"].as_bool().unwrap_or(false) {
                continue;
            }
            let ring = Ring {
                found: true,
                center: Point { x: ring_js["center"]["x"].as_f64().unwrap(), y: ring_js["center"]["y"].as_f64().unwrap() },
                radius: ring_js["radius"].as_f64().unwrap(),
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
            let classifications = classify_strokes_against_ring(&strokes, &ring, &LAYERS);
            let candidates = build_symbol_candidates(&strokes, &classifications, &ring, &LAYERS);
            let expected = scenario["candidates"].as_array().unwrap();

            assert_eq!(candidates.len(), expected.len(), "{name}: candidate count");
            for (mine, theirs) in candidates.iter().zip(expected) {
                let ctx = format!("{name}/{}", mine.candidate_id);
                assert_eq!(mine.candidate_id, theirs["candidateId"].as_str().unwrap(), "{ctx}: candidateId");
                assert_eq!(mine.layer.as_str(), theirs["layer"].as_str().unwrap(), "{ctx}: layer");
                assert_eq!(mine.radial_facing.as_str(), theirs["radialFacing"].as_str().unwrap(), "{ctx}: radialFacing");
                for (label, mine_v, theirs_v) in [
                    ("radiusNorm", mine.radius_norm, &theirs["radiusNorm"]),
                    ("sizeNorm", mine.size_norm, &theirs["sizeNorm"]),
                    ("lengthNorm", mine.length_norm, &theirs["lengthNorm"]),
                    ("neatness", mine.neatness, &theirs["neatness"]),
                    ("closedness", mine.closedness, &theirs["closedness"]),
                ] {
                    let t = theirs_v.as_f64().unwrap();
                    assert!((mine_v - t).abs() < 2e-3, "{ctx}: {label} ours={mine_v} js={t}");
                }
                checked += 1;
            }
        }
        assert!(checked > 5, "expected to parity-check several candidates, got {checked}");
    }
}
