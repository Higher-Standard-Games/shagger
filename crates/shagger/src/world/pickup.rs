//! SHAG `src/engine/world/Pickup.ts` `loadModel`: how a map pickup places
//! the model the pickup cache cloned for it.
//!
//! The first renderable mesh leads: at the pickup's position, turned by its
//! yaw about Y, scaled by the pickup scale (1.5 when none is given). Every
//! other mesh is parented to it with its position and rotation cleared, so
//! only its own scaling is left. A tossed pickup's physics is not here: map
//! weapons arrive already landed.
//!
//! A `bobbing` model (the packaged items) floats, bobs and turns like a
//! classic Quake item: lifted so its lowest point clears its rest position
//! (plus `floatLift`), bobbing [`ITEM_BOB`] units around a point that much
//! above, and turning once every [`ITEM_TURN_MS`] in step with every other
//! item.

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

/// Milliseconds per full turn of a bobbing item, the same for all so they
/// turn together.
pub const ITEM_TURN_MS: f64 = 4096.0;
/// How far a bobbing item rises and falls, around a point this far above
/// its rest height.
pub const ITEM_BOB: f64 = 4.0;

/// `floatHeight`: the height a model floats at, lifted so its lowest point
/// (`min_y`, the placed model's world bounds) sits at its rest position,
/// plus `float_lift`.
#[must_use]
pub fn float_height(leader_y: f64, min_y: f64, float_lift: Option<f64>) -> f64 {
    leader_y + (leader_y - min_y).max(0.0) + float_lift.unwrap_or(0.0)
}

/// The lowest world Y of a model placed by `leader` (Babylon's
/// `getHierarchyBoundingVectors`), or the leader's own Y for an empty one.
#[must_use]
pub fn placed_min_y(model: &GltfModel, leader: Mat4) -> f32 {
    flattened_pickup_matrices(model, leader)
        .iter()
        .zip(&model.primitives)
        .flat_map(|(world, p)| {
            p.positions
                .iter()
                .map(move |&v| world.transform_point3(Vec3::from(v)).y)
        })
        .reduce(f32::min)
        .unwrap_or(leader.w_axis.y)
}

/// `bob`: a bobbing item's height and yaw at `now_ms` (the page clock),
/// bobbing on a phase of its own pickup id's.
#[must_use]
pub fn item_bob(rest_y: f64, pickup_id: u32, yaw: f64, now_ms: f64) -> (f64, f64) {
    let rate = 0.005 + f64::from(pickup_id) * 0.00001;
    let y = rest_y + ITEM_BOB + ((now_ms + 1000.0) * rate).cos() * ITEM_BOB;
    let turn = yaw + (now_ms % ITEM_TURN_MS) / ITEM_TURN_MS * std::f64::consts::PI * 2.0;
    (y, turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_float_clear_and_turn_together() {
        // A model reaching 3 below its leader floats 3 up, plus its lift.
        assert_eq!(float_height(10.0, 7.0, None), 13.0);
        assert_eq!(float_height(10.0, 12.0, Some(10.0)), 20.0);
        let (y, a) = item_bob(20.0, 0, 0.0, 0.0);
        assert!((y - (24.0 + 5.0_f64.cos() * 4.0)).abs() < 1e-12);
        assert_eq!(a, 0.0);
        let (_, a) = item_bob(20.0, 7, 0.5, 1024.0);
        assert!((a - (0.5 + std::f64::consts::FRAC_PI_2)).abs() < 1e-12);
        let (_, b) = item_bob(20.0, 9, 0.5, 1024.0 + ITEM_TURN_MS);
        assert!((a - b).abs() < 1e-9);
    }

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
