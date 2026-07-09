//! Direct port of service/vendor/wha/src/utils/geometry.js — keep in 1:1
//! correspondence with the JS so the parity tests stay meaningful.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub width: f64,
    pub height: f64,
}

const FULL_CIRCLE_DEG: f64 = 360.0;
const HALF_CIRCLE_DEG: f64 = 180.0;

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

pub fn clamp01(value: f64) -> f64 {
    clamp(value, 0.0, 1.0)
}

pub fn normalize_angle_deg(value: f64) -> f64 {
    ((value % FULL_CIRCLE_DEG) + FULL_CIRCLE_DEG) % FULL_CIRCLE_DEG
}

pub fn degrees_to_radians(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / HALF_CIRCLE_DEG
}

pub fn radians_to_degrees(radians: f64) -> f64 {
    radians * HALF_CIRCLE_DEG / std::f64::consts::PI
}

pub fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

pub fn path_length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| distance(w[0], w[1])).sum()
}

pub fn bounds_for_points(points: &[Point]) -> Bounds {
    if points.is_empty() {
        return Bounds { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0, width: 0.0, height: 0.0 };
    }
    let min_x = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    Bounds { min_x, min_y, max_x, max_y, width: max_x - min_x, height: max_y - min_y }
}

pub fn center_of_bounds(bounds: Bounds) -> Point {
    Point { x: bounds.min_x + bounds.width / 2.0, y: bounds.min_y + bounds.height / 2.0 }
}

pub fn expand_bounds(bounds: Bounds, amount: f64) -> Bounds {
    Bounds {
        min_x: bounds.min_x - amount,
        min_y: bounds.min_y - amount,
        max_x: bounds.max_x + amount,
        max_y: bounds.max_y + amount,
        width: bounds.width + amount * 2.0,
        height: bounds.height + amount * 2.0,
    }
}

pub fn bounds_overlap(a: Bounds, b: Bounds) -> bool {
    a.min_x <= b.max_x && a.max_x >= b.min_x && a.min_y <= b.max_y && a.max_y >= b.min_y
}

pub fn angle_deg_from_center(point: Point, center: Point) -> f64 {
    normalize_angle_deg(radians_to_degrees((center.y - point.y).atan2(point.x - center.x)))
}

fn angle_from_canvas_vector(x: f64, y: f64) -> f64 {
    normalize_angle_deg(radians_to_degrees((-y).atan2(x)))
}

pub fn vector_from_angle_deg(angle_deg: f64) -> Point {
    let radians = degrees_to_radians(angle_deg);
    Point { x: radians.cos(), y: -radians.sin() }
}

pub fn angular_difference(a: f64, b: f64) -> f64 {
    let diff = (normalize_angle_deg(a) - normalize_angle_deg(b)).abs() % FULL_CIRCLE_DEG;
    if diff > HALF_CIRCLE_DEG { FULL_CIRCLE_DEG - diff } else { diff }
}

pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() { return 0.0; }
    values.iter().sum::<f64>() / values.len() as f64
}

pub fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 { return 0.0; }
    let average = mean(values);
    let variance = mean(&values.iter().map(|v| (v - average).powi(2)).collect::<Vec<_>>());
    variance.sqrt()
}

fn centroid(points: &[Point]) -> Point {
    if points.is_empty() { return Point { x: 0.0, y: 0.0 }; }
    let (sx, sy) = points.iter().fold((0.0, 0.0), |(sx, sy), p| (sx + p.x, sy + p.y));
    Point { x: sx / points.len() as f64, y: sy / points.len() as f64 }
}

/// Undirected dominant axis of a point cloud (PCA-style via the 2x2 scatter matrix).
pub fn dominant_axis_orientation_deg(points: &[Point]) -> f64 {
    if points.len() < 2 { return 0.0; }
    let center = centroid(points);
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for p in points {
        let dx = p.x - center.x;
        let dy = p.y - center.y;
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    normalize_angle_deg(angle_from_canvas_vector(angle.cos(), angle.sin()))
}

/// Port of geometry.js's allPoints — flattens a list of strokes' point lists.
pub fn all_points(point_lists: &[Vec<Point>]) -> Vec<Point> {
    point_lists.iter().flatten().copied().collect()
}

/// Port of geometry.js's boundsForStrokes.
pub fn bounds_for_point_lists(point_lists: &[Vec<Point>]) -> Bounds {
    bounds_for_points(&all_points(point_lists))
}

/// Port of geometry.js's directedStrokeAngle: the draw direction from the
/// first point of the first multi-point stroke to the last point of the
/// last multi-point stroke.
pub fn directed_stroke_angle(point_lists: &[Vec<Point>]) -> f64 {
    let first = point_lists.iter().find(|pts| pts.len() > 1);
    let last = point_lists.iter().rev().find(|pts| pts.len() > 1);
    match (first, last) {
        (Some(f), Some(l)) => {
            let first_pt = f[0];
            let last_pt = l[l.len() - 1];
            angle_from_canvas_vector(last_pt.x - first_pt.x, last_pt.y - first_pt.y)
        }
        _ => 0.0,
    }
}

pub fn endpoint_closedness(strokes: &[Vec<Point>], size: f64) -> f64 {
    let endpoints: Vec<Point> = strokes
        .iter()
        .filter(|pts| pts.len() >= 2)
        .flat_map(|pts| [pts[0], pts[pts.len() - 1]])
        .collect();

    if endpoints.len() < 2 || size <= 0.0 {
        return 0.0;
    }

    let mut min_endpoint_distance = f64::INFINITY;
    for a in 0..endpoints.len() {
        for b in (a + 1)..endpoints.len() {
            min_endpoint_distance = min_endpoint_distance.min(distance(endpoints[a], endpoints[b]));
        }
    }

    clamp01(1.0 - min_endpoint_distance / (8.0_f64).max(size * 0.28))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Parity fixtures — same inputs/expected outputs as a `node -e` run against
    // the vendored geometry.js, so any drift from the JS ground truth fails here.
    #[test]
    fn distance_matches_js() {
        assert!((distance(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn path_length_sums_segments() {
        let pts = [Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }, Point { x: 3.0, y: 0.0 }];
        assert!((path_length(&pts) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn bounds_for_points_empty_is_zeroed() {
        let b = bounds_for_points(&[]);
        assert_eq!(b, Bounds { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0, width: 0.0, height: 0.0 });
    }

    #[test]
    fn bounds_for_points_matches_js() {
        let pts = [Point { x: 1.0, y: 5.0 }, Point { x: -2.0, y: 3.0 }, Point { x: 4.0, y: -1.0 }];
        let b = bounds_for_points(&pts);
        assert_eq!(b, Bounds { min_x: -2.0, min_y: -1.0, max_x: 4.0, max_y: 5.0, width: 6.0, height: 6.0 });
    }

    #[test]
    fn normalize_angle_deg_wraps_negative() {
        assert!((normalize_angle_deg(-90.0) - 270.0).abs() < 1e-9);
        assert!((normalize_angle_deg(450.0) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn angular_difference_takes_shorter_arc() {
        assert!((angular_difference(10.0, 350.0) - 20.0).abs() < 1e-9);
        assert!((angular_difference(0.0, 180.0) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn mean_and_stddev_match_js() {
        let values = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((mean(&values) - 5.0).abs() < 1e-9);
        assert!((stddev(&values) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn stddev_below_two_values_is_zero() {
        assert_eq!(stddev(&[1.0]), 0.0);
        assert_eq!(stddev(&[]), 0.0);
    }

    #[test]
    fn vector_from_angle_deg_matches_js() {
        let v = vector_from_angle_deg(90.0);
        assert!((v.x).abs() < 1e-9);
        assert!((v.y - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn endpoint_closedness_detects_closed_ring() {
        // A near-closed ring: two strokes whose endpoints nearly touch.
        let strokes = vec![
            vec![Point { x: 0.0, y: 0.0 }, Point { x: 50.0, y: 0.0 }],
            vec![Point { x: 50.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }],
        ];
        let closedness = endpoint_closedness(&strokes, 100.0);
        assert!(closedness > 0.9, "expected near-closed ring, got {closedness}");
    }

    #[test]
    fn directed_stroke_angle_points_from_first_to_last() {
        let strokes = vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }]];
        // Rightward stroke on a screen (y down) is "east" — canvas-vector angle 0.
        assert!(directed_stroke_angle(&strokes).abs() < 1e-9);
    }

    #[test]
    fn directed_stroke_angle_skips_single_point_strokes() {
        let strokes = vec![vec![Point { x: 5.0, y: 5.0 }], vec![Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: -10.0 }]];
        // Only the second stroke has >1 point, so first==last==that stroke: straight up.
        assert!((directed_stroke_angle(&strokes) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn dominant_axis_matches_js_for_horizontal_cloud() {
        let pts = [Point { x: -10.0, y: 0.0 }, Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }];
        // A point cloud spread along the x-axis has a dominant axis at 0 or 180 deg.
        let angle = dominant_axis_orientation_deg(&pts);
        assert!(angle < 1e-6 || (angle - 180.0).abs() < 1e-6, "got {angle}");
    }
}
