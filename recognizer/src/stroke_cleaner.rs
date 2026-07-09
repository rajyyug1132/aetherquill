//! Direct port of service/vendor/wha/src/parser/strokeCleaner.js.

use crate::config::InputConfig;
use crate::geometry::{bounds_for_points, path_length, Bounds, Point};

#[derive(Debug, Clone)]
pub struct RawStroke {
    pub id: String,
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrokeMetrics {
    pub length: f64,
    pub bounds: Bounds,
    pub point_count: usize,
}

#[derive(Debug, Clone)]
pub struct CleanedStroke {
    pub id: String,
    pub points: Vec<Point>,
    pub metrics: StrokeMetrics,
}

fn smooth_points(points: &[Point]) -> Vec<Point> {
    if points.len() < 4 {
        return points.to_vec();
    }
    let last = points.len() - 1;
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i == 0 || i == last {
                *p
            } else {
                let prev = points[i - 1];
                let next = points[i + 1];
                // Pull each interior point toward the midpoint of its neighbors to
                // reduce hand jitter while keeping the original endpoints fixed.
                Point {
                    x: prev.x * 0.25 + p.x * 0.5 + next.x * 0.25,
                    y: prev.y * 0.25 + p.y * 0.5 + next.y * 0.25,
                }
            }
        })
        .collect()
}

pub fn clean_strokes(raw_strokes: &[RawStroke], config: &InputConfig) -> Vec<CleanedStroke> {
    raw_strokes
        .iter()
        .filter_map(|stroke| {
            let mut points = stroke.points.clone();
            for _ in 0..config.smoothing_passes {
                points = smooth_points(&points);
            }

            let length = path_length(&points);
            let bounds = bounds_for_points(&points);
            let point_count = points.len();

            if length >= config.min_stroke_length {
                Some(CleanedStroke {
                    id: stroke.id.clone(),
                    points,
                    metrics: StrokeMetrics { length, bounds, point_count },
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::INPUT;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn short_strokes_are_filtered_out() {
        let strokes = vec![RawStroke { id: "s1".into(), points: vec![pt(0.0, 0.0), pt(1.0, 0.0)] }];
        let cleaned = clean_strokes(&strokes, &INPUT);
        assert!(cleaned.is_empty(), "1px stroke should be below minStrokeLength=7");
    }

    #[test]
    fn long_stroke_survives_with_metrics() {
        let strokes = vec![RawStroke { id: "s1".into(), points: vec![pt(0.0, 0.0), pt(20.0, 0.0)] }];
        let cleaned = clean_strokes(&strokes, &INPUT);
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].id, "s1");
        assert_eq!(cleaned[0].metrics.point_count, 2);
        assert!((cleaned[0].metrics.length - 20.0).abs() < 1e-9);
    }

    #[test]
    fn smoothing_preserves_endpoints() {
        // Mirrors the JS smoothPoints behavior: interior points move, endpoints don't.
        let strokes = vec![RawStroke {
            id: "s1".into(),
            points: vec![pt(0.0, 0.0), pt(5.0, 10.0), pt(10.0, -10.0), pt(15.0, 0.0), pt(20.0, 0.0)],
        }];
        let cleaned = clean_strokes(&strokes, &INPUT);
        assert_eq!(cleaned[0].points.first().copied().unwrap(), pt(0.0, 0.0));
        assert_eq!(cleaned[0].points.last().copied().unwrap(), pt(20.0, 0.0));
        // Interior point should have moved toward its neighbors' midpoint (smoothed).
        assert_ne!(cleaned[0].points[1], pt(5.0, 10.0));
    }

    #[test]
    fn zero_smoothing_passes_is_a_no_op() {
        let config = InputConfig { smoothing_passes: 0, min_stroke_length: 0.0 };
        let strokes = vec![RawStroke { id: "s1".into(), points: vec![pt(0.0, 0.0), pt(5.0, 10.0), pt(10.0, 0.0)] }];
        let cleaned = clean_strokes(&strokes, &config);
        assert_eq!(cleaned[0].points, vec![pt(0.0, 0.0), pt(5.0, 10.0), pt(10.0, 0.0)]);
    }
}
