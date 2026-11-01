//! View frustum planes and box tests, for culling what the camera cannot
//! see (Babylon's `Frustum.GetPlanes` and `isInFrustum`, in wgpu's 0 to 1
//! depth).

use glam::{Mat4, Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    /// Each plane as (normal, distance), the inside where `n·p + d >= 0`.
    planes: [Vec4; 6],
}

impl Frustum {
    /// The planes of a view-projection whose clip depth runs 0 to 1.
    #[must_use]
    pub fn from_view_projection(m: Mat4) -> Self {
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        Self {
            planes: [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2],
        }
    }

    /// Whether any of the box is inside (conservative: a box near a corner
    /// can pass while outside).
    #[must_use]
    pub fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|p| {
            let n = p.truncate();
            // The corner farthest along the plane's normal.
            let far = Vec3::select(n.cmpge(Vec3::ZERO), max, min);
            n.dot(far) + p.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::camera::Camera;

    #[test]
    fn boxes_ahead_pass_and_behind_fail() {
        let camera = Camera::default();
        let frustum = Frustum::from_view_projection(camera.view_projection(16.0 / 9.0));
        let ahead = Vec3::new(0.0, 0.0, 100.0);
        assert!(frustum.intersects_box(ahead - 1.0, ahead + 1.0));
        assert!(!frustum.intersects_box(-ahead - 1.0, -ahead + 1.0));
        let left = Vec3::new(-1000.0, 0.0, 100.0);
        assert!(!frustum.intersects_box(left - 1.0, left + 1.0));
        let far = Vec3::new(0.0, 0.0, 20000.0);
        assert!(!frustum.intersects_box(far - 1.0, far + 1.0));
    }
}
