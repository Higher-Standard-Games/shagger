//! An additive spine bend toward the aim pitch (SHAG
//! `src/engine/animation/PlayerAimPose.ts`).
//!
//! SHAG writes the bend into the posed spine quaternions after the clips
//! evaluate and restores them before the next evaluation; here the clips
//! pose fresh locals every frame, so the bend is applied to those locals and
//! never accumulates.

use glam::{Mat4, Quat, Vec3};

use crate::loader::model::gltf_model::{NodeTransform, Skeleton};

/// The joints that share the bend, each by a third.
pub const AIM_JOINTS: [&str; 3] = ["mixamorig:Spine", "mixamorig:Spine1", "mixamorig:Spine2"];

/// `PlayerAimPose`: the bending joints of one body.
#[derive(Debug, Clone, Default)]
pub struct PlayerAimPose {
    joints: Vec<usize>,
}

impl PlayerAimPose {
    #[must_use]
    pub fn new(skeleton: &Skeleton) -> Self {
        Self {
            joints: AIM_JOINTS.iter().filter_map(|n| skeleton.find(n)).collect(),
        }
    }

    /// `apply`: bend each joint by `pitch / joints` about the body's right
    /// (the anchor's +X, which the anchor's yaw turns to `(cos, 0, -sin)` in
    /// the world) seen from the joint's parent. `root` is the skeleton's
    /// placement under the anchor (the mesh's lift and scale).
    pub fn apply(&self, skeleton: &Skeleton, locals: &mut [NodeTransform], root: Mat4, pitch: f32) {
        if pitch == 0.0 || self.joints.is_empty() {
            return;
        }
        let share = pitch / self.joints.len() as f32;
        for &joint in &self.joints {
            let Some(parent) = skeleton.nodes[joint].parent else {
                continue;
            };
            // Each parent's world after the joints before it bent.
            let world = root * parent_world(skeleton, locals, parent);
            let axis = world.inverse().transform_vector3(Vec3::X).normalize();
            let angle = share * if world.determinant() < 0.0 { -1.0 } else { 1.0 };
            let rotation = Quat::from_axis_angle(axis, angle);
            locals[joint].rotation = rotation * locals[joint].rotation;
        }
    }
}

fn parent_world(skeleton: &Skeleton, locals: &[NodeTransform], node: usize) -> Mat4 {
    let mut m = locals[node].matrix();
    let mut at = skeleton.nodes[node].parent;
    while let Some(p) = at {
        m = locals[p].matrix() * m;
        at = skeleton.nodes[p].parent;
    }
    m
}
