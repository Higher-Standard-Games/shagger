//! SHAG `src/engine/world/Pickup.ts` `loadModel`: how a map pickup places
//! the model the pickup cache cloned for it.
//!
//! The first renderable mesh leads: at the pickup's position, turned by its
//! yaw about Y, scaled by the pickup scale (1.5 when none is given). Every
//! other mesh is parented to it with its position and rotation cleared, so
//! only its own scaling is left. A tossed pickup's physics is not here: map
//! weapons arrive already landed.

use glam::{Mat4, Quat, Vec3};

use crate::loader::model::gltf_model::GltfModel;

/// `options.scale || 1.5`.
#[must_use]
pub fn pickup_scale(scale: Option<f32>) -> f32 {
    match scale {
        Some(s) if s != 0.0 && !s.is_nan() => s,
        _ => 1.5,
    }
}

/// The leader mesh's world matrix: Babylon composes scaling, then
/// `rotation.set(0, yaw, 0)`, then position.
#[must_use]
pub fn leader_matrix(position: Vec3, yaw: f32, scale: f32) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::splat(scale), Quat::from_rotation_y(yaw), position)
}

/// Each renderable primitive's world matrix, by primitive index.
#[must_use]
pub fn flattened_pickup_matrices(model: &GltfModel, leader: Mat4) -> Vec<Mat4> {
    model
        .primitives
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i == 0 {
                leader
            } else {
                leader * Mat4::from_scale(p.node_scale)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leader_scales_then_turns_then_moves() {
        let m = leader_matrix(Vec3::new(10.0, 0.0, 0.0), std::f32::consts::FRAC_PI_2, 2.0);
        // Babylon's RotationY turns +Z to +X.
        let p = m.transform_point3(Vec3::Z);
        assert!((p - Vec3::new(12.0, 0.0, 0.0)).length() < 1e-5);
        assert_eq!(pickup_scale(None), 1.5);
        assert_eq!(pickup_scale(Some(0.0)), 1.5);
        assert_eq!(pickup_scale(Some(2.0)), 2.0);
    }
}
