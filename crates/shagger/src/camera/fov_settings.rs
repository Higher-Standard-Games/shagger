//! SHAG `src/engine/camera/FovSettings.ts`: `cl_fov` is horizontal degrees
//! at 16:9, as Quake players state it; the camera wants vertical radians.
//! The vertical angle is held and the horizontal follows the window (Hor+).

use crate::core::cvar::Cvars;
use crate::movement::movement_vars::movement_var;

/// The aspect `cl_fov` is quoted against.
const REFERENCE_ASPECT: f64 = 16.0 / 9.0;

/// `horizontalFovToVerticalRadians`.
#[must_use]
pub fn horizontal_fov_to_vertical_radians(horizontal_degrees: f64) -> f64 {
    let half_horizontal = (horizontal_degrees * std::f64::consts::PI) / 360.0;
    2.0 * (half_horizontal.tan() / REFERENCE_ASPECT).atan()
}

/// `verticalRadiansToHorizontalFov`.
#[must_use]
pub fn vertical_radians_to_horizontal_fov(vertical_radians: f64) -> f64 {
    (2.0 * ((vertical_radians / 2.0).tan() * REFERENCE_ASPECT).atan() * 180.0) / std::f64::consts::PI
}

/// `currentBaseFov`: `cl_fov` as the camera's vertical radians.
#[must_use]
pub fn current_base_fov(cvars: &Cvars) -> f64 {
    horizontal_fov_to_vertical_radians(movement_var(cvars, "cl_fov"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ninety_at_sixteen_by_nine() {
        let v = horizontal_fov_to_vertical_radians(90.0);
        assert!((v - 1.024_778_7).abs() < 1e-6, "{v}");
        assert!((vertical_radians_to_horizontal_fov(v) - 90.0).abs() < 1e-9);
    }
}
