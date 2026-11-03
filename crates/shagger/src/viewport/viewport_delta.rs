//! SHAG `src/engine/viewport/ViewportDelta.ts`.

use glam::DVec3;

/// What one effect does to the camera this frame: an offset in camera
/// space (right, up, forward), a rotation in degrees (pitch, yaw, roll)
/// and a field-of-view multiplier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportDelta {
    pub position_offset: DVec3,
    pub rotation_offset: DVec3,
    pub fov_multiplier: f64,
}

impl Default for ViewportDelta {
    /// `createNeutralDelta`.
    fn default() -> Self {
        Self {
            position_offset: DVec3::ZERO,
            rotation_offset: DVec3::ZERO,
            fov_multiplier: 1.0,
        }
    }
}

impl ViewportDelta {
    /// `combineDelta`: offsets add, field-of-view multipliers multiply.
    pub fn combine(&mut self, add: &ViewportDelta) {
        self.position_offset += add.position_offset;
        self.rotation_offset += add.rotation_offset;
        self.fov_multiplier *= add.fov_multiplier;
    }

    /// `isDeltaNeutral`.
    #[must_use]
    pub fn is_neutral(&self, threshold: f64) -> bool {
        self.position_offset.length_squared() < threshold
            && self.rotation_offset.length_squared() < threshold
            && (self.fov_multiplier - 1.0).abs() < threshold
    }
}
