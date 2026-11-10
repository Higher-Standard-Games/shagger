//! How a packaged body is fitted and what its hands hold (SHAG
//! `src/engine/animation/PlayerModelFit.ts`).
//!
//! Each body is measured once, at load, in the first frame of a clip: its
//! standing height (so every body stands the gameplay hull's height with
//! its feet on the floor), where its supporting palm and front are for a
//! held gun, and how a blade stands in the blade stance. SHAG measures the
//! template under the glTF loader's handedness root ([`TEMPLATE_ROOT`]);
//! the measurements below put it in front as well, so a direction taken
//! from the template's world (the body's sideways X) comes out as the
//! browser's.

use glam::{Mat4, Quat, Vec3};

use crate::loader::model::gltf_model::{GltfModel, NodeTransform};

/// Babylon's glTF root in a left-handed scene: turned half way round Y and
/// scaled -1 in Z, which mirrors X.
pub const TEMPLATE_ROOT: Mat4 = Mat4::from_cols_array(&[
    -1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
]);

/// `PlayerModelFit`: the root's extra scale and the feet's lift, in the
/// model's own units before the actor's scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerModelFit {
    pub scale: f32,
    pub offset_y: f32,
}

/// `HandBasis`: a weapon hand's rotation and scale relative to its bone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandBasis {
    pub rotation: Quat,
    pub scale: Vec3,
}

impl HandBasis {
    /// The weapon hand's local matrix under the bone.
    #[must_use]
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, Vec3::ZERO)
    }
}

/// `GunSupport`: what a held gun is fitted to, in the weapon hand's frame
/// in the standing idle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GunSupport {
    /// The supporting palm, which the gun's barrel rests on.
    pub palm: Vec3,
    /// The body's front (+Z in the template), which the barrel points along.
    pub forward: Vec3,
}

/// `BladeStance`: how a held blade stands in its stance, in the weapon
/// hand's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BladeStance {
    /// The blade's direction from its handle to its tip.
    pub axis: Vec3,
    /// The body's sideways direction, square to `axis`.
    pub side: Vec3,
    /// The centre of the gun hand's fist, where the handle passes.
    pub fist: Vec3,
}

/// A posed template: every node's world matrix under [`TEMPLATE_ROOT`].
pub struct TemplatePose<'a> {
    pub model: &'a GltfModel,
    pub locals: Vec<NodeTransform>,
    pub worlds: Vec<Mat4>,
}

impl<'a> TemplatePose<'a> {
    /// `withClipPose`'s pose: the rest pose with `apply` writing a clip's
    /// first frame over it. None without a `mixamorig:Head` joint, as SHAG
    /// declines to measure such a rig.
    pub fn new(model: &'a GltfModel, apply: impl FnOnce(&mut [NodeTransform])) -> Option<Self> {
        let skeleton = &model.skeleton;
        skeleton.find("mixamorig:Head")?;
        let mut locals = skeleton.rest();
        apply(&mut locals);
        let worlds = skeleton
            .global_matrices(&locals)
            .into_iter()
            .map(|m| TEMPLATE_ROOT * m)
            .collect();
        Some(Self { model, locals, worlds })
    }

    fn world(&self, name: &str) -> Option<Mat4> {
        self.model.skeleton.find(name).map(|n| self.worlds[n])
    }

    fn position(&self, name: &str) -> Option<Vec3> {
        self.world(name).map(|m| m.w_axis.truncate())
    }

    /// `weaponHandWorld`.
    fn weapon_hand_world(&self, basis: &HandBasis) -> Option<Mat4> {
        Some(self.world("mixamorig:RightHand")? * basis.matrix())
    }

    /// `palmCentre`.
    fn palm_centre(&self, side: &str) -> Option<Vec3> {
        let wrist = self.position(&format!("mixamorig:{side}Hand"))?;
        Some(match self.position(&format!("mixamorig:{side}HandMiddle1")) {
            Some(finger) => (wrist + finger) * 0.5,
            None => wrist,
        })
    }
}

/// `measurePlayerModelFit`: the standing silhouette, skinned on the CPU in
/// the posed template (the first skin's meshes only), scaled to `height`
/// with its lowest point at zero.
#[must_use]
pub fn measure_player_model_fit(pose: &TemplatePose<'_>, height: f32) -> Option<PlayerModelFit> {
    let skeleton = &pose.model.skeleton;
    let globals = skeleton.global_matrices(&pose.locals);
    let joints = skeleton.joint_matrices(0, &globals);
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for primitive in &pose.model.primitives {
        let Some(skin) = primitive.skin.as_ref().filter(|s| s.skin == 0) else {
            continue;
        };
        for (i, p) in primitive.positions.iter().enumerate() {
            // `getPositionData(true)`: the weighted matrices summed as they
            // are, the point then divided by the sum's w.
            let mut m = Mat4::ZERO;
            for k in 0..4 {
                let w = skin.weights.get(i).map_or(0.0, |w| w[k]);
                if w > 0.0 {
                    let j = skin.joints.get(i).map_or(0, |j| j[k]) as usize;
                    m += joints.get(j).copied().unwrap_or(Mat4::IDENTITY) * w;
                }
            }
            let y = (TEMPLATE_ROOT * m).project_point3(Vec3::from(*p)).y;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    if !(min_y + max_y).is_finite() || max_y - min_y < 0.001 {
        return None;
    }
    let scale = height / (max_y - min_y);
    Some(PlayerModelFit {
        scale,
        offset_y: -min_y * scale,
    })
}

/// `measureGunSupport`.
#[must_use]
pub fn measure_gun_support(pose: &TemplatePose<'_>, basis: &HandBasis) -> Option<GunSupport> {
    let palm = pose.palm_centre("Left")?;
    let to_hand = pose.weapon_hand_world(basis)?.inverse();
    let local = to_hand.project_point3(palm);
    let forward = to_hand.transform_vector3(Vec3::Z).normalize();
    if local.length_squared() < 1e-8 || !(forward.x + forward.y + forward.z).is_finite() {
        return None;
    }
    Some(GunSupport { palm: local, forward })
}

/// `measureBladeStance`: up, leaning `lean` radians toward the body's front,
/// with the handle through the gun hand's fist; a rig without finger joints
/// puts the fist a third of a forearm along the fingers from the wrist.
#[must_use]
pub fn measure_blade_stance(pose: &TemplatePose<'_>, basis: &HandBasis, lean: f32) -> Option<BladeStance> {
    let world = pose.weapon_hand_world(basis)?;
    let wrist = pose.position("mixamorig:RightHand")?;
    let forearm = pose.position("mixamorig:RightForeArm")?;
    let to_hand = world.inverse();
    let axis = to_hand
        .transform_vector3(Vec3::new(0.0, lean.cos(), lean.sin()))
        .normalize();
    let across = to_hand.transform_vector3(Vec3::X);
    let side = (across - axis * across.dot(axis)).normalize();
    let fingers = world.transform_vector3(Vec3::Y).normalize();
    let fist_at = match pose.position("mixamorig:RightHandMiddle1") {
        Some(finger) => (wrist + finger) * 0.5,
        None => wrist + fingers * (wrist.distance(forearm) / 3.0),
    };
    let fist = to_hand.project_point3(fist_at);
    if ![axis, side, fist].iter().all(|v| (v.x + v.y + v.z).is_finite()) {
        return None;
    }
    Some(BladeStance { axis, side, fist })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_root_mirrors_x() {
        assert_eq!(
            TEMPLATE_ROOT.transform_point3(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(-1.0, 2.0, 3.0)
        );
        let babylon =
            Mat4::from_quat(Quat::from_rotation_y(std::f32::consts::PI)) * Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
        assert!((babylon.transform_point3(Vec3::new(1.0, 2.0, 3.0)) - Vec3::new(-1.0, 2.0, 3.0)).length() < 1e-6);
    }
}
