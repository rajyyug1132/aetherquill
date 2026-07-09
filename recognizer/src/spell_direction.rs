//! Direct port of service/vendor/wha/src/compiler/spellDirection.js.

use crate::geometry::{clamp, degrees_to_radians, radians_to_degrees, rounded_degrees};

const MAX_COMPONENT_TILT_DEG: f64 = 82.0;
const FORCE_TILT_MAX_DEG: f64 = 76.0;
const MIN_SURFACE_DIRECTION_MAGNITUDE: f64 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpellDirection {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub x_tilt_deg: f64,
    pub y_tilt_deg: f64,
    pub tilt_from_z_deg: f64,
}

pub fn direction_from_tilt_angles(x_tilt_deg: f64, y_tilt_deg: f64) -> SpellDirection {
    let x_tilt = clamp(x_tilt_deg, -MAX_COMPONENT_TILT_DEG, MAX_COMPONENT_TILT_DEG);
    let y_tilt = clamp(y_tilt_deg, -MAX_COMPONENT_TILT_DEG, MAX_COMPONENT_TILT_DEG);
    let x_slope = degrees_to_radians(x_tilt).tan();
    let y_slope = degrees_to_radians(y_tilt).tan();
    let magnitude = x_slope.hypot(y_slope).hypot(1.0);
    let x = x_slope / magnitude;
    let y = y_slope / magnitude;
    let z = 1.0 / magnitude;

    SpellDirection {
        x,
        y,
        z,
        x_tilt_deg: rounded_degrees(x_tilt),
        y_tilt_deg: rounded_degrees(y_tilt),
        tilt_from_z_deg: rounded_degrees(radians_to_degrees(z.acos())),
    }
}

pub fn direction_from_surface_vector(surface_direction: (f64, f64), force: f64) -> SpellDirection {
    let (sx, sy) = surface_direction;
    let surface_magnitude = sx.hypot(sy);
    if surface_magnitude < MIN_SURFACE_DIRECTION_MAGNITUDE {
        return direction_from_tilt_angles(0.0, 0.0);
    }

    let tilt_from_z_deg = clamp(force, 0.0, 1.0) * FORCE_TILT_MAX_DEG;
    let tilt_radians = degrees_to_radians(tilt_from_z_deg);
    let surface_scale = tilt_radians.sin() / surface_magnitude;
    let x = sx * surface_scale;
    let y = sy * surface_scale;
    let z = tilt_radians.cos();

    SpellDirection {
        x,
        y,
        z,
        x_tilt_deg: rounded_degrees(radians_to_degrees(x.atan2(z))),
        y_tilt_deg: rounded_degrees(radians_to_degrees(y.atan2(z))),
        tilt_from_z_deg: rounded_degrees(tilt_from_z_deg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tilt_points_straight_down_the_z_axis() {
        let d = direction_from_tilt_angles(0.0, 0.0);
        assert_eq!(d, SpellDirection { x: 0.0, y: 0.0, z: 1.0, x_tilt_deg: 0.0, y_tilt_deg: 0.0, tilt_from_z_deg: 0.0 });
    }

    #[test]
    fn tilt_is_clamped_to_max_component_tilt() {
        let d = direction_from_tilt_angles(200.0, 0.0);
        assert_eq!(d.x_tilt_deg, MAX_COMPONENT_TILT_DEG);
    }

    #[test]
    fn zero_magnitude_surface_direction_falls_back_to_no_tilt() {
        let d = direction_from_surface_vector((0.0, 0.0), 0.5);
        assert_eq!(d.x, 0.0);
        assert_eq!(d.y, 0.0);
        assert_eq!(d.z, 1.0);
    }

    #[test]
    fn full_force_with_rightward_surface_direction_tilts_toward_positive_x() {
        let d = direction_from_surface_vector((1.0, 0.0), 1.0);
        assert!((d.tilt_from_z_deg - FORCE_TILT_MAX_DEG).abs() < 1e-6);
        assert!(d.x > 0.9, "expected strong positive x tilt, got {}", d.x);
        assert!(d.y.abs() < 1e-9);
    }

    #[test]
    fn zero_force_stays_pointed_at_z_even_with_surface_direction() {
        let d = direction_from_surface_vector((1.0, 0.0), 0.0);
        assert!(d.z > 0.999, "z should be ~1 when force is 0, got {}", d.z);
        assert_eq!(d.tilt_from_z_deg, 0.0);
    }
}
