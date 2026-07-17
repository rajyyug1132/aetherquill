//! Pure stroke geometry: perfect-shape snapping (reMarkable-style).
//! No qtfb/device dependencies — unit-testable on any host.

pub fn path_length(points: &[(f64, f64)]) -> f64 {
    points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
}

/// Ramer-Douglas-Peucker polyline simplification — keeps only corners.
pub fn rdp(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let seg_len = dx.hypot(dy).max(1e-9);
    let (mut max_d, mut max_i) = (0.0, 0);
    for (i, p) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let d = ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / seg_len;
        if d > max_d {
            max_d = d;
            max_i = i;
        }
    }
    if max_d <= epsilon {
        return vec![a, b];
    }
    let mut left = rdp(&points[..=max_i], epsilon);
    let right = rdp(&points[max_i..], epsilon);
    left.pop();
    left.extend(right);
    left
}

/// Sampled points along a polygon's edges (winding = corner order).
pub fn polygon_points(corners: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for w in corners.windows(2) {
        for i in 0..12 {
            let t = i as f64 / 12.0;
            out.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
        }
    }
    out.push(*corners.last().unwrap());
    out
}

/// Auto-straighten: strict line test for every pen-up (no hold gesture).
/// Only fires on strokes that are already clearly a line — endpoints stay
/// exactly where the user drew them, so direction/placement is untouched.
/// Deliberately shorter min-length than snap_stroke so sign strokes qualify.
pub fn straighten(points: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    let len = path_length(points);
    if points.len() < 6 || len < 60.0 {
        return None;
    }
    let (p0, pn) = (points[0], points[points.len() - 1]);
    let end_dist = (pn.0 - p0.0).hypot(pn.1 - p0.1);
    if end_dist / len <= 0.97 {
        return None; // not confidently a line — leave the ink alone
    }
    Some((0..16).map(|i| {
        let t = i as f64 / 15.0;
        (p0.0 + (pn.0 - p0.0) * t, p0.1 + (pn.1 - p0.1) * t)
    }).collect())
}

/// Snap a finished-looking stroke to a perfect line, triangle, rectangle, or
/// circle (reMarkable "perfect shapes" style). Returns None when the stroke
/// isn't close enough to any. Winding is preserved — spell direction
/// semantics (sign rotation) read stroke orientation.
pub fn snap_stroke(points: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    let len = path_length(points);
    if points.len() < 8 || len < 100.0 {
        return None;
    }
    let (p0, pn) = (points[0], points[points.len() - 1]);
    let end_dist = (pn.0 - p0.0).hypot(pn.1 - p0.1);

    // Straight line: path barely longer than the endpoint chord.
    if end_dist / len > 0.95 {
        return Some((0..16).map(|i| {
            let t = i as f64 / 15.0;
            (p0.0 + (pn.0 - p0.0) * t, p0.1 + (pn.1 - p0.1) * t)
        }).collect());
    }

    let (min_x, max_x) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let (min_y, max_y) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let diag = (max_x - min_x).hypot(max_y - min_y);
    let closed = end_dist < diag * 0.25;

    // Triangle / rectangle: closed stroke that cleans up to 3-4 corners.
    if closed {
        // Resample first — pen points cluster where the hand slows (corners!),
        // which skews RDP; a uniform 6px spacing evens the vote.
        let mut resampled = vec![points[0]];
        let mut acc = 0.0;
        for w in points.windows(2) {
            let d = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            acc += d;
            if acc >= 6.0 {
                resampled.push(w[1]);
                acc = 0.0;
            }
        }
        // Close the loop, then split it at the point farthest from the start:
        // RDP with start == end has a degenerate baseline and returns garbage,
        // so each half gets real endpoints. The split point may be mid-edge —
        // it becomes an interior phantom corner the collinear pass removes.
        resampled.push(p0);
        let far = resampled
            .iter()
            .enumerate()
            .max_by(|a, b| {
                let da = (a.1.0 - p0.0).hypot(a.1.1 - p0.1);
                let db = (b.1.0 - p0.0).hypot(b.1.1 - p0.1);
                da.partial_cmp(&db).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap();
        let mut corners = rdp(&resampled[..=far], diag * 0.025);
        corners.pop(); // the split point starts the second half
        corners.extend(rdp(&resampled[far..], diag * 0.025));
        // Merge corners that are practically the same point (jitter doubles),
        // then drop near-collinear ones (mid-edge phantom corners).
        corners.dedup_by(|b, a| (b.0 - a.0).hypot(b.1 - a.1) < diag * 0.08);
        let mut i = 1;
        while i + 1 < corners.len() {
            let (a, b, c) = (corners[i - 1], corners[i], corners[i + 1]);
            let (v1, v2) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
            let cross = v1.0 * v2.1 - v1.1 * v2.0;
            let dot = v1.0 * v2.0 + v1.1 * v2.1;
            // Turn angle under ~14 degrees = straight-enough edge, not a corner.
            if cross.atan2(dot).abs() < 0.25 {
                corners.remove(i);
            } else {
                i += 1;
            }
        }
        // Seam handling: the stroke's start point is a locked RDP endpoint, so
        // a shape begun mid-edge keeps it as a phantom corner — test the seam
        // for collinearity and rotate it out.
        if corners.len() >= 4 {
            let (a, b, c) = (corners[corners.len() - 2], corners[0], corners[1]);
            let (v1, v2) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
            if (v1.0 * v2.1 - v1.1 * v2.0).atan2(v1.0 * v2.0 + v1.1 * v2.1).abs() < 0.25 {
                corners.remove(0);
                *corners.last_mut().unwrap() = corners[0];
            }
        }
        // Ensure loop closure after cleanup.
        if corners.len() >= 2 {
            let (first, last) = (corners[0], *corners.last().unwrap());
            if (first.0 - last.0).hypot(first.1 - last.1) > 1.0 {
                corners.push(first);
            }
        }
        let n_corners = corners.len() - 1; // first == last (closed loop)
        if n_corners == 3 {
            return Some(polygon_points(&corners));
        }
        if n_corners == 4 {
            // Near-axis-aligned quads become perfect bounding-box rectangles.
            let axis_aligned = corners.windows(2).all(|w| {
                let ang = (w[1].1 - w[0].1).atan2(w[1].0 - w[0].0).abs();
                ang < 0.18 || (ang - std::f64::consts::FRAC_PI_2).abs() < 0.18 || (ang - std::f64::consts::PI).abs() < 0.18
            });
            if axis_aligned {
                // Keep winding: order bbox corners starting nearest the
                // stroke's first corner, following the original direction.
                let signed_area: f64 = points.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
                let mut rect = vec![(min_x, min_y), (max_x, min_y), (max_x, max_y), (min_x, max_y)];
                if signed_area < 0.0 {
                    rect.reverse();
                }
                let start = rect.iter().enumerate().min_by(|a, b| {
                    let da = (a.1.0 - corners[0].0).hypot(a.1.1 - corners[0].1);
                    let db = (b.1.0 - corners[0].0).hypot(b.1.1 - corners[0].1);
                    da.partial_cmp(&db).unwrap()
                }).map(|(i, _)| i).unwrap_or(0);
                rect.rotate_left(start);
                rect.push(rect[0]);
                return Some(polygon_points(&rect));
            }
            return Some(polygon_points(&corners));
        }
    }

    // Circle: centroid fit, low radius variance, ends near each other.
    let n = points.len() as f64;
    let (cx, cy) = points.iter().fold((0.0, 0.0), |(ax, ay), p| (ax + p.0, ay + p.1));
    let (cx, cy) = (cx / n, cy / n);
    let radii: Vec<f64> = points.iter().map(|p| (p.0 - cx).hypot(p.1 - cy)).collect();
    let mean_r = radii.iter().sum::<f64>() / n;
    let dev = (radii.iter().map(|r| (r - mean_r).powi(2)).sum::<f64>() / n).sqrt();
    if !(end_dist < mean_r * 0.5 && mean_r > 30.0 && dev / mean_r < 0.18) {
        return None;
    }
    // Winding via signed area.
    let signed_area: f64 = points.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
    let dir = if signed_area >= 0.0 { 1.0 } else { -1.0 };
    let start_angle = (p0.1 - cy).atan2(p0.0 - cx);
    Some((0..=64).map(|i| {
        let a = start_angle + dir * std::f64::consts::TAU * i as f64 / 64.0;
        (cx + mean_r * a.cos(), cy + mean_r * a.sin())
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic pseudo-noise: hand jitter without rand dependency.
    fn jitter(i: usize, amp: f64) -> f64 {
        ((i as f64 * 12.9898).sin() * 43758.5453).fract() * amp * 2.0 - amp
    }

    fn hand_circle(cx: f64, cy: f64, r: f64, n: usize, cw: bool) -> Vec<(f64, f64)> {
        (0..=n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64 * if cw { 1.0 } else { -1.0 };
                let rr = r + jitter(i, r * 0.04);
                (cx + a.cos() * rr, cy + a.sin() * rr)
            })
            .collect()
    }

    fn hand_polyline(corners: &[(f64, f64)], per_edge: usize, amp: f64) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for w in corners.windows(2) {
            for i in 0..per_edge {
                let t = i as f64 / per_edge as f64;
                out.push((
                    w[0].0 + (w[1].0 - w[0].0) * t + jitter(out.len(), amp),
                    w[0].1 + (w[1].1 - w[0].1) * t + jitter(out.len() + 7, amp),
                ));
            }
        }
        out.push(*corners.last().unwrap());
        out
    }

    fn winding(points: &[(f64, f64)]) -> f64 {
        points.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum()
    }

    fn max_center_dev(points: &[(f64, f64)], cx: f64, cy: f64, r: f64) -> f64 {
        points
            .iter()
            .map(|p| ((p.0 - cx).hypot(p.1 - cy) - r).abs())
            .fold(0.0, f64::max)
    }


    #[test]
    fn straight_line_snaps() {
        let pts = hand_polyline(&[(100.0, 100.0), (600.0, 400.0)], 40, 3.0);
        let snapped = snap_stroke(&pts).expect("line should snap");
        // Every snapped point sits on the chord.
        for p in &snapped {
            let d = ((p.0 - 100.0) * 300.0 - (p.1 - 100.0) * 500.0).abs() / 583.1;
            assert!(d < 1.0, "off-chord by {d}");
        }
    }

    #[test]
    fn circle_snaps_and_keeps_winding() {
        for cw in [true, false] {
            let pts = hand_circle(700.0, 900.0, 250.0, 90, cw);
            let snapped = snap_stroke(&pts).expect("circle should snap");
            assert!(max_center_dev(&snapped, 700.0, 900.0, 250.0) < 15.0);
            assert_eq!(winding(&snapped) > 0.0, winding(&pts) > 0.0, "winding flipped (cw={cw})");
        }
    }

    #[test]
    fn triangle_snaps_to_three_corners() {
        let tri = [(300.0, 800.0), (700.0, 800.0), (500.0, 400.0), (300.0, 800.0)];
        let pts = hand_polyline(&tri, 35, 4.0);
        let snapped = snap_stroke(&pts).expect("triangle should snap");
        assert_eq!(snapped.len(), 3 * 12 + 1, "expected a 3-edge polygon");
        // Each detected corner within jitter distance of a true corner.
        for c in rdp(&hand_polyline(&tri, 35, 0.0), 10.0) {
            let near = snapped.iter().any(|p| (p.0 - c.0).hypot(p.1 - c.1) < 20.0);
            assert!(near, "no snapped point near true corner {c:?}");
        }
    }

    #[test]
    fn axis_rect_snaps_to_bbox() {
        let rect = [(300.0, 400.0), (900.0, 400.0), (900.0, 900.0), (300.0, 900.0), (300.0, 400.0)];
        let pts = hand_polyline(&rect, 30, 4.0);
        let snapped = snap_stroke(&pts).expect("rect should snap");
        assert_eq!(snapped.len(), 4 * 12 + 1, "expected a 4-edge polygon");
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.0), h.max(p.0)));
        let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.1), h.max(p.1)));
        for p in &snapped {
            let on_x = (p.0 - lo_x).abs() < 1.0 || (p.0 - hi_x).abs() < 1.0;
            let on_y = (p.1 - lo_y).abs() < 1.0 || (p.1 - hi_y).abs() < 1.0;
            assert!(on_x || on_y, "point {p:?} off the input bbox edges");
        }
    }

    #[test]
    fn rect_started_mid_edge_still_snaps_as_quad() {
        // Start halfway along the top edge — the seam phantom corner case.
        let rect = [
            (600.0, 400.0), (900.0, 400.0), (900.0, 900.0),
            (300.0, 900.0), (300.0, 400.0), (600.0, 400.0),
        ];
        let pts = hand_polyline(&rect, 30, 4.0);
        let snapped = snap_stroke(&pts).expect("mid-edge rect should snap, not fall to circle");
        // Must NOT have become a circle: bbox corners must be present.
        let has_corner = snapped.iter().any(|p| (p.0 - 900.0).abs() < 10.0 && (p.1 - 900.0).abs() < 10.0);
        assert!(has_corner, "corner missing — likely circle-snapped");
    }

    #[test]
    fn scribble_does_not_snap() {
        let pts: Vec<(f64, f64)> = (0..80)
            .map(|i| (400.0 + jitter(i, 150.0), 600.0 + jitter(i + 31, 150.0)))
            .collect();
        assert!(snap_stroke(&pts).is_none(), "random scribble must stay raw ink");
    }

    #[test]
    fn tiny_stroke_does_not_snap() {
        let pts = hand_circle(500.0, 500.0, 20.0, 30, true);
        assert!(snap_stroke(&pts).is_none(), "sub-threshold stroke must stay raw");
    }
}

