//! Direct port of service/vendor/wha/src/parser/signRotation.js.

use crate::geometry::{bounds_for_point_lists, center_of_bounds, degrees_to_radians, normalize_angle_deg, Point};
use crate::stroke_grouper::Candidate;
use crate::template_matcher::MatchOptions;

const CANONICAL_SIGN_ANGLE_DEG: f64 = 270.0;
const SIGN_ROTATION_TOLERANCE_DEG: f64 = 15.0;

/// Minimal stand-in for a dictionary entry's recognition-tuning fields.
/// Extended when the `dictionaries` task wires in real sigils.json/signs.json.
#[derive(Debug, Clone, Default)]
pub struct RecognitionEntry {
    pub recognition_rotation_invariant: Option<bool>,
    pub allowed_rotations_deg: Option<Vec<f64>>,
}

// Based on what's observed in the fan wiki: sign templates are authored /
// registered as if the sign sits at the bottom of the ring. Rotate a copy of
// each sign candidate into that frame before template matching.
fn sign_candidate_to_template_rotation_deg(candidate_angle_deg: Option<f64>) -> f64 {
    normalize_angle_deg(candidate_angle_deg.unwrap_or(CANONICAL_SIGN_ANGLE_DEG) - CANONICAL_SIGN_ANGLE_DEG)
}

// After the ring-relative rotation, only allow a small matching wiggle.
// Larger rotations would erase orientation, which is part of sign meaning.
fn sign_recognition_rotations() -> [f64; 3] {
    [normalize_angle_deg(-SIGN_ROTATION_TOLERANCE_DEG), 0.0, SIGN_ROTATION_TOLERANCE_DEG]
}

struct RotationTransform {
    cos: f64,
    sin: f64,
}

fn rotation_transform(degrees: f64) -> Option<RotationTransform> {
    if degrees == 0.0 {
        return None;
    }
    let radians = degrees_to_radians(degrees);
    Some(RotationTransform { cos: radians.cos(), sin: radians.sin() })
}

fn rotate_point(point: Point, center: Point, transform: &Option<RotationTransform>) -> Point {
    match transform {
        None => point,
        Some(t) => {
            let x = point.x - center.x;
            let y = point.y - center.y;
            Point { x: center.x + x * t.cos - y * t.sin, y: center.y + x * t.sin + y * t.cos }
        }
    }
}

fn rotate_candidate(candidate: &Candidate, rotation_deg: f64) -> Candidate {
    let transform = rotation_transform(rotation_deg);
    if transform.is_none() {
        return candidate.clone();
    }

    // Rotate only the recognition copy. The public candidate keeps its
    // original ring-relative angle so the compiler can still use
    // orientation as meaning.
    let center = candidate.center;
    let strokes: Vec<crate::stroke_cleaner::CleanedStroke> = candidate
        .strokes
        .iter()
        .map(|s| {
            let points: Vec<Point> = s.points.iter().map(|p| rotate_point(*p, center, &transform)).collect();
            crate::stroke_cleaner::CleanedStroke {
                id: s.id.clone(),
                metrics: crate::stroke_cleaner::StrokeMetrics {
                    length: s.metrics.length,
                    bounds: crate::geometry::bounds_for_points(&points),
                    point_count: points.len(),
                },
                points,
            }
        })
        .collect();
    let point_lists: Vec<Vec<Point>> = strokes.iter().map(|s| s.points.clone()).collect();
    let bounds = bounds_for_point_lists(&point_lists);

    Candidate {
        bounds,
        center: center_of_bounds(bounds),
        orientation_deg: normalize_angle_deg(candidate.orientation_deg + rotation_deg),
        directed_orientation_deg: normalize_angle_deg(candidate.directed_orientation_deg + rotation_deg),
        strokes,
        ..candidate.clone()
    }
}

#[derive(Debug, Clone)]
pub struct RecognitionPlan {
    pub candidate: Candidate,
    pub base_rotation_deg: f64,
    pub options: MatchOptions,
}

pub fn recognition_plan_for_symbol(kind: &str, entry: &RecognitionEntry, candidate: &Candidate) -> RecognitionPlan {
    // Only support sign rotation for now.
    if kind != "sign" {
        let rotation_invariant = entry.recognition_rotation_invariant.unwrap_or(true);
        let mut fixed = [0.0; 8];
        let len = match &entry.allowed_rotations_deg {
            Some(list) => {
                let n = list.len().min(8);
                fixed[..n].copy_from_slice(&list[..n]);
                n
            }
            None => 0,
        };
        return RecognitionPlan {
            candidate: candidate.clone(),
            base_rotation_deg: 0.0,
            options: MatchOptions {
                rotation_invariant,
                allowed_rotations_deg: if entry.allowed_rotations_deg.is_some() { Some(fixed) } else { None },
                allowed_rotations_len: len,
            },
        };
    }

    // Signs get normalized to the bottom-of-ring template frame, then the
    // matcher tests only the small tolerance rotations from
    // sign_recognition_rotations().
    let base_rotation_deg = sign_candidate_to_template_rotation_deg(Some(candidate.angle_deg));
    let mut fixed = [0.0; 8];
    let tolerances = sign_recognition_rotations();
    fixed[..3].copy_from_slice(&tolerances);

    RecognitionPlan {
        candidate: rotate_candidate(candidate, base_rotation_deg),
        base_rotation_deg,
        options: MatchOptions { rotation_invariant: false, allowed_rotations_deg: Some(fixed), allowed_rotations_len: 3 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_mapper::Layer;
    use crate::stroke_cleaner::{CleanedStroke, StrokeMetrics};

    fn dummy_candidate(angle_deg: f64) -> Candidate {
        let points = vec![Point { x: -5.0, y: 0.0 }, Point { x: 5.0, y: 0.0 }];
        let bounds = crate::geometry::bounds_for_points(&points);
        Candidate {
            candidate_id: "c1".into(),
            stroke_ids: vec!["s1".into()],
            raw_stroke_count: 1,
            cleaned_stroke_count: 1,
            bounds,
            center: Point { x: 0.0, y: 0.0 },
            radius_norm: 0.2,
            angle_deg,
            layer: Layer::Outer,
            near_boundary: false,
            size_norm: 0.1,
            length_norm: 0.1,
            orientation_deg: 0.0,
            directed_orientation_deg: 0.0,
            radial_facing: crate::stroke_grouper::RadialFacing::Outward,
            closedness: 0.0,
            overdraw_amount: 0.0,
            neatness: 0.9,
            strokes: vec![CleanedStroke {
                id: "s1".into(),
                points: points.clone(),
                metrics: StrokeMetrics { length: 10.0, bounds, point_count: 2 },
            }],
        }
    }

    #[test]
    fn sigil_kind_is_unrotated_and_defaults_rotation_invariant() {
        let candidate = dummy_candidate(90.0);
        let entry = RecognitionEntry::default();
        let plan = recognition_plan_for_symbol("sigil", &entry, &candidate);
        assert_eq!(plan.base_rotation_deg, 0.0);
        assert!(plan.options.rotation_invariant);
        assert_eq!(plan.candidate.strokes[0].points, candidate.strokes[0].points);
    }

    #[test]
    fn sign_at_canonical_angle_needs_no_rotation() {
        // A sign already at the bottom of the ring (270deg) needs 0 base rotation.
        let candidate = dummy_candidate(270.0);
        let entry = RecognitionEntry::default();
        let plan = recognition_plan_for_symbol("sign", &entry, &candidate);
        assert_eq!(plan.base_rotation_deg, 0.0);
        assert!(!plan.options.rotation_invariant);
        assert_eq!(plan.options.allowed_rotations_len, 3);
    }

    #[test]
    fn sign_at_top_of_ring_rotates_180_to_canonical_frame() {
        let candidate = dummy_candidate(90.0); // opposite side of the ring from 270deg
        let entry = RecognitionEntry::default();
        let plan = recognition_plan_for_symbol("sign", &entry, &candidate);
        assert!((plan.base_rotation_deg - 180.0).abs() < 1e-9);
        // Rotating [-5,0]..[5,0] by 180deg around origin flips both points.
        assert!((plan.candidate.strokes[0].points[0].x - 5.0).abs() < 1e-9);
        assert!((plan.candidate.strokes[0].points[1].x - (-5.0)).abs() < 1e-9);
    }
}
