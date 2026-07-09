//! Direct port of service/vendor/wha/src/parser/ringDetector.js.
//!
//! Ring detection combines a geometric prepared-ring pass with a topological
//! sealed-ring pass:
//! 1. Build open candidates by fitting circles to long seed strokes, gathering
//!    nearby ring-like strokes, then scoring angular coverage and roundness.
//! 2. Choose a closure reference from the previous open ring, or from the
//!    best current open candidate. When there is a reference, retry
//!    flood-fill closure with only strokes relevant to that ring so distant
//!    outside marks do not distort the closure test.
//! 3. If the filtered closure pass did not produce a closed candidate, run
//!    the flood-fill closure test against all strokes.
//! 4. Merge duplicate candidates for the same physical ring, prefer complete
//!    rings, and report any additional distinct rings as unsupported.
//! 5. Emit activation only for the transition from a prepared open ring to a
//!    sealed ring, not for rings that are already closed on first detection.
//!
//! ponytail: the JS threads a `config` parameter through several leaf
//! functions (strokeCircleMetrics, openCoverageHalfWidth, measureRing, ...)
//! that never actually read a field off it — dropped here; only the two
//! functions that genuinely use config.ring.minRadius / config.layers.boundaryMax
//! take a config reference.

use crate::config::{LayersConfig, RingConfig};
use crate::geometry::{
    all_points, angle_deg_from_center, bounds_for_point_lists, center_of_bounds, clamp, degrees_to_radians, distance, mean,
    path_length, stddev, Bounds, Point,
};
use crate::stroke_cleaner::CleanedStroke;
use crate::topological_flood_fill::{analyze_topological_closure, ClosureResult};
use std::collections::HashSet;

const MIN_CLOSURE_RELEVANT_POINT_RATIO: f64 = 0.15;
const RING_BIN_COUNT: usize = 96;
const MIN_SEED_LENGTH_PX: f64 = 130.0;
const FOUND_COMPLETENESS: f64 = 0.52;
const ACTIVATION_COMPLETENESS_FLOOR: f64 = 0.64;
const MIN_ROUNDNESS: f64 = 0.36;
const OPEN_COVERAGE_HALF_WIDTH_PX: f64 = 12.0;
const OPEN_COVERAGE_HALF_WIDTH_RATIO: f64 = 0.055;
const OPEN_COLLECTION_MIN_RATIO: f64 = 0.45;
const STROKE_SAMPLE_STEP_PX: f64 = 0.75;
const TOPOLOGY_RING_STROKE_MIN_NEAR_CIRCLE_RATIO: f64 = 0.56;
const TOPOLOGY_RING_STROKE_MIN_NEAR_CIRCLE_LENGTH_PX: f64 = 24.0;
const TOPOLOGY_RING_PRUNE_COVERAGE_FLOOR: f64 = 0.88;
const TOPOLOGY_RING_PRUNE_MAX_ANGULAR_SPAN_DEG: f64 = 24.0;

// One physical ring can produce several candidates from different seed strokes or
// topology passes. These tolerances merge those duplicate candidates before we
// decide whether the drawing has unsupported multiple distinct rings.
const SAME_RING_CENTER_DISTANCE_RATIO: f64 = 0.22;
const SAME_RING_RADIUS_RATIO: f64 = 0.18;

#[derive(Debug, Clone, Copy)]
struct Circle {
    center: Point,
    radius: f64,
}

// Solves the 3 unknowns from the circle fit's linearized equation:
// x^2 + y^2 = a*x + b*y + c. Uses Gaussian elimination on a 3x3 system.
fn solve3(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> Option<[f64; 3]> {
    let mut a: [[f64; 4]; 3] =
        [[matrix[0][0], matrix[0][1], matrix[0][2], vector[0]], [matrix[1][0], matrix[1][1], matrix[1][2], vector[1]], [
            matrix[2][0],
            matrix[2][1],
            matrix[2][2],
            vector[2],
        ]];

    for column in 0..3 {
        let mut pivot = column;
        for row in (column + 1)..3 {
            if a[row][column].abs() > a[pivot][column].abs() {
                pivot = row;
            }
        }

        if a[pivot][column].abs() < 1e-8 {
            return None;
        }

        a.swap(column, pivot);
        let divisor = a[column][column];
        for item in column..4 {
            a[column][item] /= divisor;
        }

        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = a[row][column];
            for item in column..4 {
                a[row][item] -= factor * a[column][item];
            }
        }
    }

    Some([a[0][3], a[1][3], a[2][3]])
}

fn fit_circle(points: &[Point]) -> Option<Circle> {
    if points.len() < 8 {
        return None;
    }

    let (mut sum_x, mut sum_y, mut sum_x2, mut sum_y2, mut sum_xy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut sum_x3, mut sum_y3, mut sum_x2y, mut sum_xy2) = (0.0, 0.0, 0.0, 0.0);

    for p in points {
        let (x, y) = (p.x, p.y);
        let (x2, y2) = (x * x, y * y);
        sum_x += x;
        sum_y += y;
        sum_x2 += x2;
        sum_y2 += y2;
        sum_xy += x * y;
        sum_x3 += x2 * x;
        sum_y3 += y2 * y;
        sum_x2y += x2 * y;
        sum_xy2 += x * y2;
    }

    let result = solve3(
        [[sum_x2, sum_xy, sum_x], [sum_xy, sum_y2, sum_y], [sum_x, sum_y, points.len() as f64]],
        [sum_x3 + sum_xy2, sum_x2y + sum_y3, sum_x2 + sum_y2],
    )?;

    let [a, b, c] = result;
    let center = Point { x: a / 2.0, y: b / 2.0 };
    let radius_squared = c + center.x * center.x + center.y * center.y;
    if !radius_squared.is_finite() || radius_squared <= 0.0 {
        return None;
    }

    Some(Circle { center, radius: radius_squared.sqrt() })
}

fn fallback_circle(strokes: &[CleanedStroke]) -> Circle {
    let point_lists: Vec<Vec<Point>> = strokes.iter().map(|s| s.points.clone()).collect();
    let bounds = bounds_for_point_lists(&point_lists);
    Circle { center: center_of_bounds(bounds), radius: (bounds.width + bounds.height) / 4.0 }
}

fn mark_angle(bins: &mut [bool], angle_deg: f64) {
    let raw = ((angle_deg / 360.0) * bins.len() as f64).floor() as i64;
    let bin = raw.rem_euclid(bins.len() as i64) as usize;
    bins[bin] = true;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingGap {
    pub start_angle: f64,
    pub end_angle: f64,
    pub size_degrees: f64,
}

fn largest_gap(bins: &[bool]) -> RingGap {
    let size = bins.len();
    let mut best_start = 0i64;
    let mut best_length = 0i64;
    let mut current_start = 0i64;
    let mut current_length = 0i64;

    for index in 0..(size * 2) {
        let bin = bins[index % size];
        if !bin {
            if current_length == 0 {
                current_start = index as i64;
            }
            current_length += 1;
            if current_length > best_length && current_length <= size as i64 {
                best_start = current_start;
                best_length = current_length;
            }
        } else {
            current_length = 0;
        }
    }

    let bin_degrees = 360.0 / size as f64;
    RingGap {
        start_angle: (best_start.rem_euclid(size as i64)) as f64 * bin_degrees,
        end_angle: ((best_start + best_length).rem_euclid(size as i64)) as f64 * bin_degrees,
        size_degrees: best_length as f64 * bin_degrees,
    }
}

fn open_coverage_half_width(radius: f64) -> f64 {
    OPEN_COVERAGE_HALF_WIDTH_PX.max(radius * OPEN_COVERAGE_HALF_WIDTH_RATIO)
}

struct StrokeCircleMetrics {
    total_length: f64,
    near_length: f64,
    near_ratio: f64,
}

fn stroke_circle_metrics(stroke: &CleanedStroke, circle: Circle, mut bins: Option<&mut [bool]>) -> StrokeCircleMetrics {
    let half_width = open_coverage_half_width(circle.radius);
    let mut total_length = 0.0;
    let mut near_length = 0.0;

    for index in 1..stroke.points.len() {
        let previous = stroke.points[index - 1];
        let current = stroke.points[index];
        let segment_length = distance(previous, current);
        if segment_length <= 0.0 {
            continue;
        }

        let steps = (1i64).max((segment_length / STROKE_SAMPLE_STEP_PX).ceil() as i64);
        let sample_length = segment_length / steps as f64;
        total_length += segment_length;

        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let point = Point { x: previous.x + (current.x - previous.x) * t, y: previous.y + (current.y - previous.y) * t };
            let near_circle = (distance(point, circle.center) - circle.radius).abs() <= half_width;
            if near_circle {
                near_length += sample_length;
                if let Some(b) = bins.as_deref_mut() {
                    mark_angle(b, angle_deg_from_center(point, circle.center));
                }
            }
        }
    }

    StrokeCircleMetrics { total_length, near_length, near_ratio: if total_length > 0.0 { near_length / total_length } else { 0.0 } }
}

struct OpenCoverage {
    coverage_ratio: f64,
    gap: RingGap,
    gap_arc_length: f64,
    near_circle_ink_ratio: f64,
}

fn measure_open_coverage(strokes: &[CleanedStroke], circle: Circle) -> OpenCoverage {
    let mut bins = vec![false; RING_BIN_COUNT];
    let mut near_length = 0.0;
    let mut total_length = 0.0;

    for stroke in strokes {
        let metrics = stroke_circle_metrics(stroke, circle, Some(&mut bins));
        near_length += metrics.near_length;
        total_length += metrics.total_length;
    }

    let coverage_bins = bins.iter().filter(|&&c| c).count();
    let coverage_ratio = coverage_bins as f64 / (1usize).max(bins.len()) as f64;
    let gap = largest_gap(&bins);

    OpenCoverage {
        coverage_ratio,
        gap_arc_length: degrees_to_radians(gap.size_degrees) * circle.radius,
        gap,
        near_circle_ink_ratio: if total_length > 0.0 { near_length / total_length } else { 0.0 },
    }
}

#[derive(Debug, Clone)]
pub struct RingCandidate {
    pub found: bool,
    pub center: Point,
    pub radius: f64,
    pub complete: bool,
    pub completeness: f64,
    pub coverage_ratio: f64,
    pub gap: RingGap,
    pub gap_arc_length: f64,
    pub roundness: f64,
    pub line_smoothness: f64,
    pub neatness: f64,
    pub overdraw_amount: f64,
    pub stroke_ids: Vec<String>,
    pub score: f64,
}

fn measure_ring(
    strokes: &[CleanedStroke],
    reference: Option<(Point, f64, f64)>, // (center, radius, roundness)
    topology: Option<&ClosureResult>,
) -> Option<RingCandidate> {
    let point_lists: Vec<Vec<Point>> = strokes.iter().map(|s| s.points.clone()).collect();
    let points = all_points(&point_lists);
    let topology_closed = topology.map(|t| t.closed).unwrap_or(false);
    if points.len() < 8 && !topology_closed {
        return None;
    }

    let fitted = if points.len() >= 8 { Some(fit_circle(&points).unwrap_or_else(|| fallback_circle(strokes))) } else { None };
    let center = reference.map(|r| r.0).or(fitted.map(|f| f.center)).or(topology.map(|t| t.center));
    let center = center?;
    let radius = reference
        .map(|r| r.1)
        .or(fitted.map(|f| f.radius))
        .or(topology.map(|t| t.radius))
        .unwrap_or_else(|| mean(&points.iter().map(|p| distance(*p, center)).collect::<Vec<_>>()));
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }

    let radial_distances: Vec<f64> = points.iter().map(|p| distance(*p, center)).collect();
    let residual = if topology_closed { topology.unwrap().normalized_rmse } else { stddev(&radial_distances) / (1.0_f64).max(radius) };
    let bounds =
        if !points.is_empty() { bounds_for_point_lists(&point_lists) } else { Bounds { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0, width: radius * 2.0, height: radius * 2.0 } };
    let aspect = bounds.width.min(bounds.height) / (1.0_f64).max(bounds.width.max(bounds.height));
    let fitted_roundness = clamp((1.0 - residual * 3.1) * 0.78 + aspect * 0.22, 0.0, 1.0);
    let roundness = if topology_closed { topology.unwrap().perfection } else { reference.map(|r| r.2).unwrap_or(fitted_roundness) };
    let coverage = measure_open_coverage(strokes, Circle { center, radius });
    let complete = topology_closed;
    let completeness = if complete { 1.0 } else { coverage.coverage_ratio };
    let circumference = std::f64::consts::TAU * radius;
    let ink_length: f64 = strokes.iter().map(|s| path_length(&s.points)).sum();
    let overdraw = (0.0_f64).max(ink_length / (1.0_f64).max(circumference) - 1.08);
    let closure_quality = if complete { topology.unwrap().perfection } else { clamp(coverage.coverage_ratio, 0.0, 1.0) };
    let line_smoothness = clamp(
        (if complete { topology.unwrap().perfection } else { coverage.near_circle_ink_ratio }) * 0.72 + (1.0 - residual) * 0.28
            - overdraw * 0.12,
        0.0,
        1.0,
    );
    let neatness = clamp(roundness * 0.42 + line_smoothness * 0.36 + closure_quality * 0.22, 0.0, 1.0);

    Some(RingCandidate {
        found: true,
        center,
        radius,
        complete,
        completeness,
        coverage_ratio: coverage.coverage_ratio,
        gap: coverage.gap,
        gap_arc_length: coverage.gap_arc_length,
        roundness,
        line_smoothness,
        neatness,
        overdraw_amount: clamp(overdraw, 0.0, 1.0),
        stroke_ids: strokes.iter().map(|s| s.id.clone()).collect(),
        score: 0.0,
    })
}

fn collect_open_ring_strokes(seed_ring: &RingCandidate, strokes: &[CleanedStroke]) -> Vec<CleanedStroke> {
    let circle = Circle { center: seed_ring.center, radius: seed_ring.radius };
    strokes
        .iter()
        .filter(|stroke| {
            if seed_ring.stroke_ids.iter().any(|id| id == &stroke.id) {
                return true;
            }
            stroke_circle_metrics(stroke, circle, None).near_ratio >= OPEN_COLLECTION_MIN_RATIO
        })
        .cloned()
        .collect()
}

struct StrokeAngularCoverage {
    angular_span_deg: f64,
}

fn stroke_angular_coverage(stroke: &CleanedStroke, circle: Circle) -> StrokeAngularCoverage {
    let mut bins = vec![false; RING_BIN_COUNT];
    stroke_circle_metrics(stroke, circle, Some(&mut bins));
    let covered_bin_count = bins.iter().filter(|&&c| c).count();
    StrokeAngularCoverage { angular_span_deg: covered_bin_count as f64 * (360.0 / (1usize).max(bins.len()) as f64) }
}

fn prune_redundant_short_ring_strokes(ring_strokes: &[CleanedStroke], circle: Circle, ring_config: &RingConfig) -> Vec<CleanedStroke> {
    let mut kept: Vec<CleanedStroke> = ring_strokes.to_vec();
    let mut sorted_by_length: Vec<&CleanedStroke> = ring_strokes.iter().collect();
    sorted_by_length.sort_by(|a, b| path_length(&a.points).partial_cmp(&path_length(&b.points)).unwrap());

    for stroke in sorted_by_length {
        if kept.len() <= 1 || !kept.iter().any(|item| item.id == stroke.id) {
            continue;
        }

        let coverage = stroke_angular_coverage(stroke, circle);
        if coverage.angular_span_deg > TOPOLOGY_RING_PRUNE_MAX_ANGULAR_SPAN_DEG {
            continue;
        }

        let without_stroke: Vec<CleanedStroke> = kept.iter().filter(|item| item.id != stroke.id).cloned().collect();
        let without_coverage = measure_open_coverage(&without_stroke, circle);
        let without_topology = analyze_topological_closure(&without_stroke, ring_config);
        if without_coverage.coverage_ratio >= TOPOLOGY_RING_PRUNE_COVERAGE_FLOOR && without_topology.closed {
            kept = without_stroke;
        }
    }

    kept
}

fn collect_topological_ring_strokes(strokes: &[CleanedStroke], topology: &ClosureResult, ring_config: &RingConfig) -> Vec<CleanedStroke> {
    let edge_stroke_ids: HashSet<&str> = topology.stroke_ids.iter().map(String::as_str).collect();
    let edge_strokes: Vec<CleanedStroke> = strokes.iter().filter(|s| edge_stroke_ids.contains(s.id.as_str())).cloned().collect();
    let edge_points: Vec<Point> = edge_strokes.iter().flat_map(|s| s.points.clone()).collect();
    let refined_circle = fit_circle(&edge_points).unwrap_or(Circle { center: topology.center, radius: topology.radius });
    let ring_strokes: Vec<CleanedStroke> = edge_strokes
        .iter()
        .filter(|stroke| {
            let metrics = stroke_circle_metrics(stroke, refined_circle, None);
            metrics.near_length >= TOPOLOGY_RING_STROKE_MIN_NEAR_CIRCLE_LENGTH_PX && metrics.near_ratio >= TOPOLOGY_RING_STROKE_MIN_NEAR_CIRCLE_RATIO
        })
        .cloned()
        .collect();

    if !ring_strokes.is_empty() {
        prune_redundant_short_ring_strokes(&ring_strokes, refined_circle, ring_config)
    } else {
        edge_strokes
    }
}

// When a nearly complete ring already exists, retry closure using only strokes near
// that ring so distant stray marks cannot distort flood-fill bounds or circle scoring.
struct ClosureReference {
    center: Point,
    radius: f64,
    stroke_ids: Vec<String>,
}

fn closure_relevant_strokes(strokes: &[CleanedStroke], reference: Option<&ClosureReference>, layers_config: &LayersConfig) -> Vec<CleanedStroke> {
    let Some(reference) = reference else { return strokes.to_vec() };

    let previous_ring_stroke_ids: HashSet<&str> = reference.stroke_ids.iter().map(String::as_str).collect();
    let boundary_radius = reference.radius * layers_config.boundary_max;

    strokes
        .iter()
        .filter(|stroke| {
            if previous_ring_stroke_ids.contains(stroke.id.as_str()) {
                return true;
            }
            let point_count = (1usize).max(stroke.points.len());
            let inside_or_boundary_ratio =
                stroke.points.iter().filter(|p| distance(**p, reference.center) <= boundary_radius).count() as f64 / point_count as f64;
            inside_or_boundary_ratio >= MIN_CLOSURE_RELEVANT_POINT_RATIO
        })
        .cloned()
        .collect()
}

fn score_candidate(ring: Option<&RingCandidate>, ring_config: &RingConfig) -> f64 {
    let Some(ring) = ring else { return 0.0 };
    if ring.radius < ring_config.min_radius {
        return 0.0;
    }
    let radius_score = clamp((ring.radius - ring_config.min_radius) / 180.0, 0.0, 1.0);
    let closure_bonus = if ring.complete { 0.3 } else { 0.0 };
    clamp(ring.completeness * 0.38 + ring.roundness * 0.25 + ring.neatness * 0.19 + radius_score * 0.08 + closure_bonus, 0.0, 1.3)
}

fn add_topological_candidate(candidates: &mut Vec<RingCandidate>, strokes: &[CleanedStroke], ring_config: &RingConfig) -> ClosureResult {
    let topology = analyze_topological_closure(strokes, ring_config);
    if topology.closed {
        let ring_strokes = collect_topological_ring_strokes(strokes, &topology, ring_config);
        let measure_input: &[CleanedStroke] = if !ring_strokes.is_empty() { &ring_strokes } else { strokes };
        let measured = measure_ring(measure_input, None, Some(&topology));
        let score = score_candidate(measured.as_ref(), ring_config);
        if let Some(mut measured) = measured {
            if score > 0.0 {
                measured.score = score;
                candidates.push(measured);
            }
        }
    }
    topology
}

// Finds prepared rings before they are topologically closed. Each long, wide
// stroke gets one chance to act as a seed circle. Nearby strokes are then
// gathered onto that circle and scored as one open ring candidate.
fn build_open_ring_candidates(strokes: &[CleanedStroke], ring_config: &RingConfig) -> Vec<RingCandidate> {
    let mut candidates = vec![];
    let seeds: Vec<&CleanedStroke> = strokes
        .iter()
        .filter(|stroke| {
            let length = stroke.metrics.length;
            let bounds = stroke.metrics.bounds;
            let diagonal = bounds.width.hypot(bounds.height);
            length >= MIN_SEED_LENGTH_PX && diagonal >= ring_config.min_radius * 1.35
        })
        .collect();

    for seed in seeds {
        let Some(first_pass) = measure_ring(std::slice::from_ref(seed), None, None) else { continue };
        let ring_strokes = collect_open_ring_strokes(&first_pass, strokes);
        let measured = measure_ring(&ring_strokes, Some((first_pass.center, first_pass.radius, first_pass.roundness)), None);
        let score = score_candidate(measured.as_ref(), ring_config);
        if let Some(mut measured) = measured {
            if score > 0.0 && measured.completeness >= FOUND_COMPLETENESS && measured.roundness >= MIN_ROUNDNESS {
                measured.score = score;
                candidates.push(measured);
            }
        }
    }

    candidates
}

fn add_reference_filtered_closure_candidate(
    candidates: &mut Vec<RingCandidate>,
    strokes: &[CleanedStroke],
    reference: Option<&ClosureReference>,
    ring_config: &RingConfig,
    layers_config: &LayersConfig,
) -> bool {
    let Some(reference) = reference else { return false };

    let relevant_strokes = closure_relevant_strokes(strokes, Some(reference), layers_config);
    if relevant_strokes.len() == strokes.len() || relevant_strokes.len() < 2 {
        return false;
    }

    let previous_candidate_count = candidates.len();
    add_topological_candidate(candidates, &relevant_strokes, ring_config);
    candidates.len() > previous_candidate_count
}

fn best_open_ring_candidate(open_candidates: &[RingCandidate]) -> Option<&RingCandidate> {
    open_candidates
        .iter()
        .filter(|c| !c.complete)
        .max_by(|a, b| (a.score + a.radius * 0.001).partial_cmp(&(b.score + b.radius * 0.001)).unwrap())
}

fn closure_reference_ring<'a>(previous_ring: Option<&'a Ring>, open_candidates: &'a [RingCandidate]) -> Option<ClosureReference> {
    if let Some(previous) = previous_ring {
        if previous.found && !previous.complete {
            return Some(ClosureReference { center: previous.center, radius: previous.radius, stroke_ids: previous.stroke_ids.clone() });
        }
    }
    best_open_ring_candidate(open_candidates).map(|c| ClosureReference { center: c.center, radius: c.radius, stroke_ids: c.stroke_ids.clone() })
}

fn is_same_physical_ring(a: &RingCandidate, b: &RingCandidate) -> bool {
    let average_radius = (1.0_f64).max((a.radius + b.radius) / 2.0);
    let center_distance = distance(a.center, b.center);
    let radius_ratio = (a.radius - b.radius).abs() / average_radius;
    center_distance <= average_radius * SAME_RING_CENTER_DISTANCE_RATIO && radius_ratio <= SAME_RING_RADIUS_RATIO
}

fn distinct_ring_candidates(candidates: Vec<RingCandidate>) -> Vec<RingCandidate> {
    let mut distinct: Vec<RingCandidate> = vec![];
    for candidate in candidates {
        if !distinct.iter().any(|existing| is_same_physical_ring(existing, &candidate)) {
            distinct.push(candidate);
        }
    }
    distinct
}

#[derive(Debug, Clone)]
pub struct UnsupportedRing {
    pub center: Point,
    pub radius: f64,
    pub complete: bool,
    pub completeness: f64,
    pub stroke_ids: Vec<String>,
}

fn summarize_unsupported_ring(candidate: &RingCandidate) -> UnsupportedRing {
    UnsupportedRing {
        center: candidate.center,
        radius: candidate.radius,
        complete: candidate.complete,
        completeness: candidate.completeness,
        stroke_ids: candidate.stroke_ids.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct Ring {
    pub found: bool,
    pub center: Point,
    pub radius: f64,
    pub complete: bool,
    pub completeness: f64,
    pub coverage_ratio: f64,
    pub gap: RingGap,
    pub gap_arc_length: f64,
    pub roundness: f64,
    pub line_smoothness: f64,
    pub neatness: f64,
    pub overdraw_amount: f64,
    pub stroke_ids: Vec<String>,
    pub activation_event: bool,
    pub unsupported_nested_rings: Vec<UnsupportedRing>,
    pub unsupported_multiple_rings: Vec<UnsupportedRing>,
}

impl Default for Ring {
    fn default() -> Ring {
        Ring {
            found: false,
            center: Point { x: 0.0, y: 0.0 },
            radius: 0.0,
            complete: false,
            completeness: 0.0,
            coverage_ratio: 0.0,
            gap: RingGap { start_angle: 0.0, end_angle: 0.0, size_degrees: 0.0 },
            gap_arc_length: 0.0,
            roundness: 0.0,
            line_smoothness: 0.0,
            neatness: 0.0,
            overdraw_amount: 0.0,
            stroke_ids: vec![],
            activation_event: false,
            unsupported_nested_rings: vec![],
            unsupported_multiple_rings: vec![],
        }
    }
}

pub fn detect_ring(strokes: &[CleanedStroke], previous_ring: Option<&Ring>, ring_config: &RingConfig, layers_config: &LayersConfig) -> Ring {
    let mut candidates: Vec<RingCandidate> = vec![];

    let open_candidates = build_open_ring_candidates(strokes, ring_config);

    let reference = closure_reference_ring(previous_ring, &open_candidates);
    let filtered_closure_found =
        add_reference_filtered_closure_candidate(&mut candidates, strokes, reference.as_ref(), ring_config, layers_config);
    if !filtered_closure_found {
        add_topological_candidate(&mut candidates, strokes, ring_config);
    }
    candidates.extend(open_candidates);

    if candidates.is_empty() {
        return Ring::default();
    }

    candidates.sort_by(|a, b| {
        let complete_order = (b.complete as i32).cmp(&(a.complete as i32));
        if complete_order != std::cmp::Ordering::Equal {
            return complete_order;
        }
        (b.score + b.radius * 0.001).partial_cmp(&(a.score + a.radius * 0.001)).unwrap()
    });
    let distinct_rings = distinct_ring_candidates(candidates);
    let ring = &distinct_rings[0];
    let unsupported_multiple_rings: Vec<UnsupportedRing> = distinct_rings[1..].iter().map(summarize_unsupported_ring).collect();
    let unsupported_nested_rings: Vec<UnsupportedRing> = distinct_rings[1..]
        .iter()
        .filter(|c| c.radius < ring.radius * 0.78 && c.roundness >= 0.68 && c.complete)
        .map(summarize_unsupported_ring)
        .collect();

    let activation_event = previous_ring
        .map(|p| {
            p.found && !p.complete && ring.complete && p.completeness >= ACTIVATION_COMPLETENESS_FLOOR && unsupported_multiple_rings.is_empty()
        })
        .unwrap_or(false);

    Ring {
        found: ring.found,
        center: ring.center,
        radius: ring.radius,
        complete: ring.complete,
        completeness: ring.completeness,
        coverage_ratio: ring.coverage_ratio,
        gap: ring.gap,
        gap_arc_length: ring.gap_arc_length,
        roundness: ring.roundness,
        line_smoothness: ring.line_smoothness,
        neatness: ring.neatness,
        overdraw_amount: ring.overdraw_amount,
        stroke_ids: ring.stroke_ids.clone(),
        activation_event,
        unsupported_nested_rings,
        unsupported_multiple_rings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{INPUT, LAYERS, RING};
    use crate::stroke_cleaner::{clean_strokes, RawStroke};

    fn ring_stroke(cx: f64, cy: f64, radius: f64) -> RawStroke {
        let points = (0..=130)
            .map(|i| {
                let angle = (i as f64 / 128.0) * std::f64::consts::TAU;
                Point { x: cx + angle.cos() * radius, y: cy + angle.sin() * radius }
            })
            .collect();
        RawStroke { id: "ring".into(), points }
    }

    fn open_ring_stroke(cx: f64, cy: f64, radius: f64) -> RawStroke {
        // ~77% of the circle — an open ring with a real gap.
        let points = (0..=100)
            .map(|i| {
                let angle = (i as f64 / 128.0) * std::f64::consts::TAU;
                Point { x: cx + angle.cos() * radius, y: cy + angle.sin() * radius }
            })
            .collect();
        RawStroke { id: "ring".into(), points }
    }

    #[test]
    fn empty_strokes_are_not_found() {
        let ring = detect_ring(&[], None, &RING, &LAYERS);
        assert!(!ring.found);
    }

    #[test]
    fn closed_ring_is_found_and_complete() {
        let raw = vec![ring_stroke(350.0, 450.0, 260.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let ring = detect_ring(&strokes, None, &RING, &LAYERS);
        assert!(ring.found);
        assert!(ring.complete, "a clean closed ring should be complete");
        assert!((ring.radius - 260.0).abs() < 10.0, "radius {} should be close to 260", ring.radius);
        assert!((ring.center.x - 350.0).abs() < 10.0);
        assert!((ring.center.y - 450.0).abs() < 10.0);
    }

    #[test]
    fn open_ring_is_found_but_not_complete() {
        let raw = vec![open_ring_stroke(350.0, 450.0, 260.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let ring = detect_ring(&strokes, None, &RING, &LAYERS);
        assert!(ring.found, "a long curved stroke should seed an open ring candidate");
        assert!(!ring.complete);
    }

    #[test]
    fn activation_event_fires_on_transition_from_open_to_closed() {
        let raw_open = vec![open_ring_stroke(350.0, 450.0, 260.0)];
        let open_strokes = clean_strokes(&raw_open, &INPUT);
        let open = detect_ring(&open_strokes, None, &RING, &LAYERS);
        assert!(open.found && !open.complete);

        let raw_closed = vec![ring_stroke(350.0, 450.0, 260.0)];
        let closed_strokes = clean_strokes(&raw_closed, &INPUT);
        let closed = detect_ring(&closed_strokes, Some(&open), &RING, &LAYERS);
        assert!(closed.complete);
        // Only fires when the previous ring was open enough (completeness floor) — this
        // synthetic open ring is a real ~77% arc so it should clear ACTIVATION_COMPLETENESS_FLOOR.
        if open.completeness >= ACTIVATION_COMPLETENESS_FLOOR {
            assert!(closed.activation_event, "expected activation event, previous completeness={}", open.completeness);
        }
    }

    #[test]
    fn no_activation_event_on_first_detection_already_closed() {
        let raw = vec![ring_stroke(350.0, 450.0, 260.0)];
        let strokes = clean_strokes(&raw, &INPUT);
        let ring = detect_ring(&strokes, None, &RING, &LAYERS);
        assert!(!ring.activation_event, "a ring closed on first detection (no previous open ring) should not activate");
    }

    // --- parity against the JS pipeline fixtures ---

    #[test]
    fn parity_with_js_ring() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/pipeline.json"))
            .expect("fixtures/pipeline.json — regenerate with: node service/parity-gen.mjs");
        let scenarios: serde_json::Value = serde_json::from_str(&raw).unwrap();

        let mut checked = 0;
        for scenario in scenarios.as_array().unwrap() {
            let name = scenario["name"].as_str().unwrap();
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
                            length: path_length(&points),
                            bounds: crate::geometry::bounds_for_points(&points),
                            point_count: points.len(),
                        },
                        points,
                    }
                })
                .collect();

            let ring = detect_ring(&strokes, None, &RING, &LAYERS);
            let expected = &scenario["ring"];
            let ctx = format!("{name}/ring");

            assert_eq!(ring.found, expected["found"].as_bool().unwrap(), "{ctx}: found");
            if ring.found {
                assert_eq!(ring.complete, expected["complete"].as_bool().unwrap(), "{ctx}: complete");
                // JS rounds ring fields to 3 digits (roundedDeep on glyphAST) — tolerance 2e-3.
                for (label, mine, theirs) in [
                    ("radius", ring.radius, expected["radius"].as_f64().unwrap()),
                    ("completeness", ring.completeness, expected["completeness"].as_f64().unwrap()),
                    ("roundness", ring.roundness, expected["roundness"].as_f64().unwrap()),
                    ("neatness", ring.neatness, expected["neatness"].as_f64().unwrap()),
                ] {
                    assert!((mine - theirs).abs() < 2e-3, "{ctx}: {label} ours={mine} js={theirs}");
                }
                assert!((ring.center.x - expected["center"]["x"].as_f64().unwrap()).abs() < 2e-3, "{ctx}: center.x");
                assert!((ring.center.y - expected["center"]["y"].as_f64().unwrap()).abs() < 2e-3, "{ctx}: center.y");
                checked += 1;
            }
        }
        assert!(checked > 5, "expected to parity-check several ring-found scenarios, got {checked}");
    }
}
