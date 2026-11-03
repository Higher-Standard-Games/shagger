//! A free camera in Babylon's conventions (RUST.md 7.2): Y up, left-handed,
//! yaw about Y and pitch about X as `FreeCamera.rotation` holds them, depth
//! 0 to 1.

use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Babylon space.
    pub position: Vec3,
    /// Radians; 0 looks along +Z.
    pub yaw: f32,
    /// Radians; positive looks down.
    pub pitch: f32,
    /// Radians about the view direction (Babylon `rotation.z`); the view
    /// effects' lean and roll.
    pub roll: f32,
    /// Vertical field of view in radians (Babylon's default is 0.8).
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_y: 0.8,
            near: 1.0,
            far: 16384.0,
        }
    }
}

impl Camera {
    /// The view direction: `Matrix.RotationYawPitchRoll(yaw, pitch, 0)` applied to +Z.
    #[must_use]
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            -self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    /// The direction to the camera's right, level with the horizon.
    #[must_use]
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// The view's up: `Matrix.RotationYawPitchRoll(yaw, pitch, roll)`
    /// applied to +Y.
    #[must_use]
    pub fn up(&self) -> Vec3 {
        let level_up = Vec3::new(
            self.yaw.sin() * self.pitch.sin(),
            self.pitch.cos(),
            self.yaw.cos() * self.pitch.sin(),
        );
        if self.roll == 0.0 {
            return level_up;
        }
        level_up * self.roll.cos() - self.right() * self.roll.sin()
    }

    #[must_use]
    pub fn view(&self) -> Mat4 {
        glam::camera::lh::view::look_to_mat4(self.position, self.forward(), self.up())
    }

    #[must_use]
    pub fn projection(&self, aspect: f32) -> Mat4 {
        // wgpu clip space is Y up with depth 0 to 1, the Direct3D convention.
        glam::camera::lh::proj::directx::perspective(self.fov_y, aspect, self.near, self.far)
    }

    #[must_use]
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Look from a Quake 3 origin with a Quake 3 yaw in degrees (the `angle`
    /// key of a spawn point: counter-clockwise from +X about Z).
    #[must_use]
    pub fn from_q3(origin: [f32; 3], angle_degrees: f32) -> Self {
        Self {
            position: Vec3::new(origin[0], origin[2], origin[1]),
            yaw: (90.0 - angle_degrees).to_radians(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_turns_the_view_about_its_direction() {
        let mut c = Camera {
            yaw: 0.7,
            pitch: 0.3,
            ..Camera::default()
        };
        assert!(c.up().dot(c.forward()).abs() < 1e-6);
        // Unrolled, the up the view used before roll existed.
        let before = glam::camera::lh::view::look_to_mat4(c.position, c.forward(), Vec3::Y);
        assert!(c.view().abs_diff_eq(before, 1e-5));
        c.roll = 0.2;
        let up = c.up();
        assert!(up.dot(c.forward()).abs() < 1e-6);
        assert!((up.length() - 1.0).abs() < 1e-6);
        // Positive roll leans the top of the view to the left.
        assert!(up.dot(c.right()) < 0.0);
    }
}
