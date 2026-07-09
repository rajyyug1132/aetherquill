//! Direct port of service/vendor/wha/src/parser/templateNormalizer.js.

use crate::geometry::{bounds_for_points, clamp01, distance, Point};

pub struct NormalizeOptions {
    pub samples_per_stroke: usize,
    pub digits: i32,
    pub fit_to_bounds: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        NormalizeOptions { samples_per_stroke: 32, digits: 4, fit_to_bounds: false }
    }
}

#[derive(Debug, Clone)]
pub struct NormalizedTemplate {
    pub source_aspect_ratio: f64,
    pub strokes: Vec<Vec<Point>>,
}

fn resample_stroke(points: &[Point], target_count: usize) -> Vec<Point> {
    if points.is_empty() || target_count == 0 {
        return vec![];
    }
    if points.len() == 1 || target_count == 1 {
        return vec![points[0]; target_count];
    }

    let mut cumulative = vec![0.0];
    for i in 1..points.len() {
        cumulative.push(cumulative[i - 1] + distance(points[i - 1], points[i]));
    }

    let total = *cumulative.last().unwrap();
    if total <= 0.0001 {
        return vec![points[0]; target_count];
    }

    let mut result = Vec::with_capacity(target_count);
    let mut segment_index = 1;
    for sample in 0..target_count {
        let target = total * sample as f64 / (1usize).max(target_count - 1) as f64;
        while segment_index < cumulative.len() - 1 && cumulative[segment_index] < target {
            segment_index += 1;
        }

        let previous_distance = cumulative[segment_index - 1];
        let next_distance = cumulative[segment_index];
        let local = clamp01((target - previous_distance) / (0.0001_f64).max(next_distance - previous_distance));
        let previous = points[segment_index - 1];
        let next = points[segment_index];
        result.push(Point { x: previous.x + (next.x - previous.x) * local, y: previous.y + (next.y - previous.y) * local });
    }

    result
}

fn round_point(point: Point, digits: i32) -> Point {
    let factor = 10f64.powi(digits);
    Point { x: (point.x * factor).round() / factor, y: (point.y * factor).round() / factor }
}

pub fn normalize_strokes_for_template(strokes: &[Vec<Point>], options: &NormalizeOptions) -> NormalizedTemplate {
    let source_strokes: Vec<Vec<Point>> = strokes.iter().filter(|pts| !pts.is_empty()).cloned().collect();
    let all_points: Vec<Point> = source_strokes.iter().flatten().copied().collect();

    if all_points.is_empty() {
        return NormalizedTemplate { source_aspect_ratio: 1.0, strokes: vec![] };
    }

    let bounds = bounds_for_points(&all_points);
    let scale = if options.fit_to_bounds {
        bounds.width.max(bounds.height).max(0.0001)
    } else {
        bounds.width.max(bounds.height).max(1.0)
    };
    let center = Point { x: bounds.min_x + bounds.width / 2.0, y: bounds.min_y + bounds.height / 2.0 };

    let normalized_strokes: Vec<Vec<Point>> = source_strokes
        .iter()
        .map(|points| {
            resample_stroke(points, options.samples_per_stroke)
                .into_iter()
                .map(|p| round_point(Point { x: (p.x - center.x) / scale + 0.5, y: (p.y - center.y) / scale + 0.5 }, options.digits))
                .collect()
        })
        .collect();

    let aspect_denom: f64 = if options.fit_to_bounds { 0.0001 } else { 1.0 };
    let source_aspect_ratio = ((bounds.width / aspect_denom.max(bounds.height)) * 1000.0).round() / 1000.0;

    NormalizedTemplate { source_aspect_ratio, strokes: normalized_strokes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_default_template() {
        let out = normalize_strokes_for_template(&[], &NormalizeOptions::default());
        assert_eq!(out.source_aspect_ratio, 1.0);
        assert!(out.strokes.is_empty());
    }

    #[test]
    fn single_point_stroke_repeats_that_point() {
        let strokes = vec![vec![Point { x: 5.0, y: 5.0 }]];
        let out = normalize_strokes_for_template(&strokes, &NormalizeOptions { samples_per_stroke: 4, ..Default::default() });
        assert_eq!(out.strokes[0].len(), 4);
        // A single-point stroke normalizes to the center of its own (zero) bounds → (0.5, 0.5).
        for p in &out.strokes[0] {
            assert_eq!(*p, Point { x: 0.5, y: 0.5 });
        }
    }

    #[test]
    fn resample_count_matches_requested() {
        let strokes = vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }]];
        let out = normalize_strokes_for_template(&strokes, &NormalizeOptions { samples_per_stroke: 8, ..Default::default() });
        assert_eq!(out.strokes[0].len(), 8);
    }

    #[test]
    fn normalized_points_stay_within_unit_square_ish_bounds() {
        let strokes = vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 20.0, y: 0.0 }, Point { x: 20.0, y: 20.0 }, Point { x: 0.0, y: 20.0 }]];
        let out = normalize_strokes_for_template(&strokes, &NormalizeOptions::default());
        for p in &out.strokes[0] {
            assert!((-0.01..=1.01).contains(&p.x), "x={}", p.x);
            assert!((-0.01..=1.01).contains(&p.y), "y={}", p.y);
        }
    }

    #[test]
    fn digits_option_rounds_output() {
        let strokes = vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 7.0 }]];
        let out = normalize_strokes_for_template(&strokes, &NormalizeOptions { samples_per_stroke: 5, digits: 1, ..Default::default() });
        for p in &out.strokes[0] {
            assert_eq!((p.x * 10.0).round() / 10.0, p.x, "should already be rounded to 1 digit");
        }
    }
}
