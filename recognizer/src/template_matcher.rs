//! Direct port of service/vendor/wha/src/parser/templateMatcher.js.
//!
//! ponytail: the JS version caches rendered ink per stroke-template/candidate
//! object identity (WeakMap) because the browser UI re-scores candidates
//! every animation frame. The device pipeline scores once per pen-up, so
//! that cache is speculative here — skipped. Add a HashMap<TemplateId, Ink>
//! cache if profiling on-device ever shows scoring is the bottleneck.

use crate::geometry::{clamp, degrees_to_radians, normalize_angle_deg, Point};
use crate::template_normalizer::{normalize_strokes_for_template, NormalizeOptions};

const INK_SIZE: usize = 40;
const CORE_RADIUS: i32 = 1;
const SOFT_RADIUS: i32 = 2;
const LOOSE_RADIUS: i32 = 4;
const CANDIDATE_SAMPLES_PER_STROKE: usize = 40;
const REGION_GRID_SIZE: usize = 10;
const ROTATION_STABILITY_MARGIN: f64 = 0.018;

#[derive(Default, Clone, Copy)]
struct RotationTransform {
    cos: f64,
    sin: f64,
}

fn rotation_set(allowed_rotations_deg: &Option<Vec<f64>>, rotation_invariant: bool) -> Vec<f64> {
    if let Some(list) = allowed_rotations_deg {
        if !list.is_empty() {
            return list.clone();
        }
    }
    if rotation_invariant {
        return vec![0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
    }
    vec![0.0]
}

fn rotation_transform(degrees: f64) -> Option<RotationTransform> {
    if degrees == 0.0 {
        return None;
    }
    let radians = degrees_to_radians(degrees);
    Some(RotationTransform { cos: radians.cos(), sin: radians.sin() })
}

fn normalized_rotation_magnitude(degrees: f64) -> f64 {
    let normalized = normalize_angle_deg(degrees);
    normalized.min(360.0 - normalized) / 180.0
}

fn rotate_point(point: Point, transform: Option<RotationTransform>) -> Point {
    match transform {
        None => point,
        Some(t) => {
            let x = point.x - 0.5;
            let y = point.y - 0.5;
            Point { x: x * t.cos - y * t.sin + 0.5, y: x * t.sin + y * t.cos + 0.5 }
        }
    }
}

struct Layer {
    mask: Vec<u8>,
    ink: usize,
}

fn create_layer(size: usize) -> Layer {
    Layer { mask: vec![0u8; size * size], ink: 0 }
}

struct Ink {
    core: Layer,
    soft: Layer,
    loose: Layer,
}

fn mark_mask(mask: &mut [u8], size: usize, x: f64, y: f64, radius: i32) {
    let center_x = (clamp(x, 0.0, 1.0) * (size - 1) as f64).round() as i32;
    let center_y = (clamp(y, 0.0, 1.0) * (size - 1) as f64).round() as i32;
    let radius_sq = radius * radius;

    for offset_y in -radius..=radius {
        for offset_x in -radius..=radius {
            if offset_x * offset_x + offset_y * offset_y > radius_sq {
                continue;
            }
            let pixel_x = center_x + offset_x;
            let pixel_y = center_y + offset_y;
            if pixel_x < 0 || pixel_x >= size as i32 || pixel_y < 0 || pixel_y >= size as i32 {
                continue;
            }
            mask[pixel_y as usize * size + pixel_x as usize] = 1;
        }
    }
}

fn mark_ink(ink: &mut Ink, size: usize, x: f64, y: f64) {
    mark_mask(&mut ink.core.mask, size, x, y, CORE_RADIUS);
    mark_mask(&mut ink.soft.mask, size, x, y, SOFT_RADIUS);
    mark_mask(&mut ink.loose.mask, size, x, y, LOOSE_RADIUS);
}

fn draw_segment(ink: &mut Ink, size: usize, start: Point, end: Point) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let steps = (1i32).max((dx.hypot(dy) * size as f64 * 2.0).ceil() as i32);

    for index in 0..=steps {
        let local = index as f64 / steps as f64;
        mark_ink(ink, size, start.x + dx * local, start.y + dy * local);
    }
}

fn count_ink(mask: &[u8]) -> usize {
    mask.iter().map(|&p| p as usize).sum()
}

fn render_ink(strokes: &[Vec<Point>], rotation_deg: f64, size: usize) -> Ink {
    let mut ink = Ink { core: create_layer(size), soft: create_layer(size), loose: create_layer(size) };
    let transform = rotation_transform(rotation_deg);

    for stroke in strokes {
        if stroke.is_empty() {
            continue;
        }
        let points: Vec<Point> = stroke.iter().map(|p| rotate_point(*p, transform)).collect();
        if points.len() == 1 {
            mark_ink(&mut ink, size, points[0].x, points[0].y);
            continue;
        }
        for index in 1..points.len() {
            draw_segment(&mut ink, size, points[index - 1], points[index]);
        }
    }

    ink.core.ink = count_ink(&ink.core.mask);
    ink.soft.ink = count_ink(&ink.soft.mask);
    ink.loose.ink = count_ink(&ink.loose.mask);
    ink
}

fn template_ink(template_strokes: &[Vec<Point>]) -> Ink {
    let options = NormalizeOptions { samples_per_stroke: CANDIDATE_SAMPLES_PER_STROKE, fit_to_bounds: true, digits: 5 };
    let normalized = normalize_strokes_for_template(template_strokes, &options);
    render_ink(&normalized.strokes, 0.0, INK_SIZE)
}

fn candidate_ink(candidate_strokes: &[Vec<Point>], rotation_deg: f64) -> Ink {
    let options = NormalizeOptions { samples_per_stroke: CANDIDATE_SAMPLES_PER_STROKE, fit_to_bounds: true, digits: 5 };
    let normalized = normalize_strokes_for_template(candidate_strokes, &options);
    render_ink(&normalized.strokes, rotation_deg, INK_SIZE)
}

fn mask_overlap(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).filter(|(x, y)| **x != 0 && **y != 0).count()
}

fn dice_score(a: &[u8], b: &[u8], a_ink: usize, b_ink: usize) -> f64 {
    if a_ink == 0 || b_ink == 0 {
        return 0.0;
    }
    clamp((mask_overlap(a, b) * 2) as f64 / (a_ink + b_ink) as f64, 0.0, 1.0)
}

fn occupied_cells(mask: &[u8], size: usize, grid_size: usize) -> Vec<u8> {
    let mut cells = vec![0u8; grid_size * grid_size];
    for y in 0..size {
        for x in 0..size {
            if mask[y * size + x] == 0 {
                continue;
            }
            let cell_x = ((x as f64 / size as f64) * grid_size as f64).floor() as usize;
            let cell_y = ((y as f64 / size as f64) * grid_size as f64).floor() as usize;
            let cell_x = cell_x.min(grid_size - 1);
            let cell_y = cell_y.min(grid_size - 1);
            cells[cell_y * grid_size + cell_x] = 1;
        }
    }
    cells
}

struct RegionStats {
    required_cell_coverage: f64,
    forbidden_cell_ink_ratio: f64,
    region_score: f64,
}

fn cell_stats(candidate_ink: &Ink, reference_ink: &Ink) -> RegionStats {
    let candidate_core_cells = occupied_cells(&candidate_ink.core.mask, INK_SIZE, REGION_GRID_SIZE);
    let candidate_loose_cells = occupied_cells(&candidate_ink.loose.mask, INK_SIZE, REGION_GRID_SIZE);
    let reference_core_cells = occupied_cells(&reference_ink.core.mask, INK_SIZE, REGION_GRID_SIZE);
    let reference_loose_cells = occupied_cells(&reference_ink.loose.mask, INK_SIZE, REGION_GRID_SIZE);

    let mut required_count = 0usize;
    let mut required_covered = 0usize;
    let mut candidate_count = 0usize;
    let mut forbidden_candidate_count = 0usize;

    for index in 0..reference_core_cells.len() {
        if reference_core_cells[index] != 0 {
            required_count += 1;
            if candidate_loose_cells[index] != 0 {
                required_covered += 1;
            }
        }
        if candidate_core_cells[index] != 0 {
            candidate_count += 1;
            if reference_loose_cells[index] == 0 {
                forbidden_candidate_count += 1;
            }
        }
    }

    let required_cell_coverage = if required_count > 0 { required_covered as f64 / required_count as f64 } else { 0.0 };
    let forbidden_cell_ink_ratio = if candidate_count > 0 { forbidden_candidate_count as f64 / candidate_count as f64 } else { 1.0 };
    let region_score = clamp(required_cell_coverage * 0.68 + (1.0 - forbidden_cell_ink_ratio) * 0.32, 0.0, 1.0);

    RegionStats { required_cell_coverage, forbidden_cell_ink_ratio, region_score }
}

#[derive(Debug, Clone, Copy)]
struct InkMatch {
    ink_score: f64,
    #[allow(dead_code)]
    candidate_explained_ratio: f64,
    template_covered_ratio: f64,
    soft_dice_score: f64,
    unexplained_ink_ratio: f64,
    #[allow(dead_code)]
    missing_ink_ratio: f64,
    #[allow(dead_code)]
    contamination_risk: f64,
    required_cell_coverage: f64,
    forbidden_cell_ink_ratio: f64,
    region_score: f64,
}

fn compare_ink(candidate_ink: &Ink, reference_ink: &Ink) -> InkMatch {
    let candidate_ink_count = candidate_ink.core.ink;
    let reference_ink_count = reference_ink.core.ink;

    if candidate_ink_count == 0 || reference_ink_count == 0 {
        return InkMatch {
            ink_score: 0.0,
            candidate_explained_ratio: 0.0,
            template_covered_ratio: 0.0,
            soft_dice_score: 0.0,
            unexplained_ink_ratio: 1.0,
            missing_ink_ratio: 1.0,
            contamination_risk: 1.0,
            required_cell_coverage: 0.0,
            forbidden_cell_ink_ratio: 1.0,
            region_score: 0.0,
        };
    }

    let candidate_explained_ratio =
        clamp(mask_overlap(&candidate_ink.core.mask, &reference_ink.loose.mask) as f64 / candidate_ink_count as f64, 0.0, 1.0);
    let template_covered_ratio =
        clamp(mask_overlap(&reference_ink.core.mask, &candidate_ink.loose.mask) as f64 / reference_ink_count as f64, 0.0, 1.0);
    let soft_dice_score =
        dice_score(&candidate_ink.soft.mask, &reference_ink.soft.mask, candidate_ink.soft.ink, reference_ink.soft.ink);
    let unexplained_ink_ratio = clamp(1.0 - candidate_explained_ratio, 0.0, 1.0);
    let missing_ink_ratio = clamp(1.0 - template_covered_ratio, 0.0, 1.0);
    let regions = cell_stats(candidate_ink, reference_ink);
    let ink_score = clamp(
        candidate_explained_ratio * 0.32
            + template_covered_ratio * 0.32
            + soft_dice_score * 0.14
            + regions.required_cell_coverage * 0.16
            + (1.0 - regions.forbidden_cell_ink_ratio) * 0.06,
        0.0,
        1.0,
    );
    let contamination_risk = clamp(
        clamp((unexplained_ink_ratio - 0.26) / 0.34, 0.0, 1.0) * 0.58
            + clamp((missing_ink_ratio - 0.46) / 0.34, 0.0, 1.0) * 0.22
            + clamp((regions.forbidden_cell_ink_ratio - 0.18) / 0.46, 0.0, 1.0) * 0.2,
        0.0,
        1.0,
    );

    InkMatch {
        ink_score,
        candidate_explained_ratio,
        template_covered_ratio,
        soft_dice_score,
        unexplained_ink_ratio,
        missing_ink_ratio,
        contamination_risk,
        required_cell_coverage: regions.required_cell_coverage,
        forbidden_cell_ink_ratio: regions.forbidden_cell_ink_ratio,
        region_score: regions.region_score,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MatchOptions {
    pub allowed_rotations_deg: Option<[f64; 8]>,
    pub allowed_rotations_len: usize,
    pub rotation_invariant: bool,
}

impl MatchOptions {
    fn rotation_list(&self) -> Option<Vec<f64>> {
        self.allowed_rotations_deg.map(|arr| arr[..self.allowed_rotations_len].to_vec())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TemplateScore {
    pub available: bool,
    pub confidence: f64,
    pub rotation_deg: f64,
    pub recognition_rotation_deg: f64,
    pub ink_score: f64,
    pub soft_dice_score: f64,
    pub candidate_explained_ratio: f64,
    pub template_covered_ratio: f64,
    pub unexplained_ink_ratio: f64,
    pub missing_ink_ratio: f64,
    pub contamination_risk: f64,
    pub required_cell_coverage: f64,
    pub forbidden_cell_ink_ratio: f64,
    pub region_score: f64,
}

impl Default for TemplateScore {
    fn default() -> Self {
        TemplateScore {
            available: false,
            confidence: 0.0,
            rotation_deg: 0.0,
            recognition_rotation_deg: 0.0,
            ink_score: 0.0,
            soft_dice_score: 0.0,
            candidate_explained_ratio: 0.0,
            template_covered_ratio: 0.0,
            unexplained_ink_ratio: 1.0,
            missing_ink_ratio: 1.0,
            contamination_risk: 1.0,
            required_cell_coverage: 0.0,
            forbidden_cell_ink_ratio: 1.0,
            region_score: 0.0,
        }
    }
}

pub fn score_stroke_template(
    candidate_strokes: &[Vec<Point>],
    template_strokes: &[Vec<Point>],
    options: &MatchOptions,
) -> TemplateScore {
    if template_strokes.is_empty() {
        return TemplateScore::default();
    }

    let reference_ink = template_ink(template_strokes);

    let mut best_rotation_deg = 0.0;
    let mut best_ranking_score = -1.0;
    let mut best_match = InkMatch {
        ink_score: 0.0,
        candidate_explained_ratio: 0.0,
        template_covered_ratio: 0.0,
        soft_dice_score: 0.0,
        unexplained_ink_ratio: 1.0,
        missing_ink_ratio: 1.0,
        contamination_risk: 1.0,
        required_cell_coverage: 0.0,
        forbidden_cell_ink_ratio: 1.0,
        region_score: 0.0,
    };

    for rotation_deg in rotation_set(&options.rotation_list(), options.rotation_invariant) {
        let ink_match = compare_ink(&candidate_ink(candidate_strokes, rotation_deg), &reference_ink);
        let rotation_penalty = normalized_rotation_magnitude(rotation_deg) * ROTATION_STABILITY_MARGIN;
        let ranking_score = ink_match.ink_score - rotation_penalty;
        if ranking_score > best_ranking_score {
            best_ranking_score = ranking_score;
            best_rotation_deg = rotation_deg;
            best_match = ink_match;
        }
    }

    let contamination_cap = if best_match.unexplained_ink_ratio > 0.36 && best_match.template_covered_ratio < 0.82 {
        clamp(0.62 - (best_match.unexplained_ink_ratio - 0.36) * 0.8, 0.2, 1.0)
    } else {
        1.0
    };

    TemplateScore {
        available: true,
        confidence: clamp(best_ranking_score, 0.0, 1.0).min(contamination_cap),
        rotation_deg: best_rotation_deg,
        recognition_rotation_deg: best_rotation_deg,
        ink_score: best_match.ink_score,
        soft_dice_score: best_match.soft_dice_score,
        candidate_explained_ratio: best_match.candidate_explained_ratio,
        template_covered_ratio: best_match.template_covered_ratio,
        unexplained_ink_ratio: best_match.unexplained_ink_ratio,
        missing_ink_ratio: best_match.missing_ink_ratio,
        contamination_risk: best_match.contamination_risk,
        required_cell_coverage: best_match.required_cell_coverage,
        forbidden_cell_ink_ratio: best_match.forbidden_cell_ink_ratio,
        region_score: best_match.region_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(cx: f64, cy: f64, half: f64) -> Vec<Vec<Point>> {
        vec![vec![
            Point { x: cx - half, y: cy - half },
            Point { x: cx + half, y: cy - half },
            Point { x: cx + half, y: cy + half },
            Point { x: cx - half, y: cy + half },
            Point { x: cx - half, y: cy - half },
        ]]
    }

    #[test]
    fn empty_template_is_unavailable() {
        let candidate = square(0.0, 0.0, 10.0);
        let score = score_stroke_template(&candidate, &[], &MatchOptions::default());
        assert!(!score.available);
        assert_eq!(score.confidence, 0.0);
    }

    #[test]
    fn identical_shape_scores_high_confidence() {
        let shape = square(0.0, 0.0, 10.0);
        let score = score_stroke_template(&shape, &shape, &MatchOptions::default());
        assert!(score.available);
        assert!(score.confidence > 0.9, "identical shapes should score near 1.0, got {}", score.confidence);
    }

    #[test]
    fn unrelated_shapes_score_zero() {
        // Ground truth from the real JS templateMatcher on this exact input: 0.
        let square_shape = square(0.0, 0.0, 10.0);
        let dot = vec![vec![Point { x: 0.0, y: 0.0 }]];
        let score = score_stroke_template(&dot, &square_shape, &MatchOptions::default());
        assert_eq!(score.confidence, 0.0);
    }

    #[test]
    fn rotated_square_matches_js_ground_truth() {
        // A square's bounding box is smaller (side) than its 45deg-rotated diamond's
        // bounding box (diagonal), so per-candidate bbox-fit normalization does NOT
        // make rotation_invariant recover a near-1.0 match here — verified against
        // the real JS: confidence ~0.4361, best rotation 0, same with/without
        // rotation_invariant. Do not "fix" this — it's the JS's actual behavior.
        let shape = square(0.0, 0.0, 10.0);
        let (s, c) = (45f64.to_radians().sin(), 45f64.to_radians().cos());
        let rotated: Vec<Vec<Point>> =
            shape.iter().map(|stroke| stroke.iter().map(|p| Point { x: p.x * c - p.y * s, y: p.x * s + p.y * c }).collect()).collect();

        let plain = score_stroke_template(&rotated, &shape, &MatchOptions::default());
        let invariant =
            score_stroke_template(&rotated, &shape, &MatchOptions { rotation_invariant: true, ..Default::default() });

        assert!((plain.confidence - 0.43608366132597504).abs() < 1e-9, "got {}", plain.confidence);
        assert!((invariant.confidence - 0.43608366132597504).abs() < 1e-9, "got {}", invariant.confidence);
        assert_eq!(invariant.rotation_deg, 0.0);
    }
}
