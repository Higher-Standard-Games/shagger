// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, packages/dev/core/src/Animations/animatorAvatar.ts
// (AnimatorAvatar.retargetAnimationGroup and _retargetAnimationKeys),
// Copyright Babylon.js contributors, licensed under the Apache License 2.0
// (see ../LICENSE).
//
// Changes: only the path SHAG takes (`fixRootPosition: false`, no ground
// reference, no hierarchy check, no morph targets), over shagger's glTF
// skeletons and clips instead of transform nodes and animation groups. Both
// rigs are measured in glTF scene space: Babylon's glTF loader puts the same
// handedness root above both, which cancels out of every product below.

//! Retargets a clip made on one rig to another whose joints share its names
//! but not its rest orientations or units, as SHAG's player model manager
//! does with `AnimatorAvatar.retargetAnimationGroup`.

use glam::{Mat4, Quat, Vec3};

use super::super::maths::math_glam::decompose;
use crate::loader::model::gltf_animation::{AnimationClip, ChannelPath, ClipChannel};
use crate::loader::model::gltf_model::Skeleton;

/// `retargetAnimationGroup(source, { fixRootPosition: false })`: `clip`'s
/// channels, which target nodes of `source` (the clip file's own rig at its
/// rest), carried onto the bones of `target` at rest. `bone` finds a target
/// bone by name (`findBoneByTransformNode`, then `findBoneByName`); a
/// channel with no bone, or whose node `source` lacks, is removed, as
/// Babylon removes it. Each key keeps its time: the joint's turn away from
/// its rest, in world space, is applied to the target joint's rest.
#[must_use]
pub fn retarget_animation_group(
    clip: &AnimationClip,
    source: &Skeleton,
    target: &Skeleton,
    bone: impl Fn(&str) -> Option<usize>,
) -> AnimationClip {
    let source_rest = source.rest();
    let source_world = source.global_matrices(&source_rest);
    let target_world = target.global_matrices(&target.rest());
    let mut channels = Vec::new();
    for channel in &clip.channels {
        let Some(node) = source.find(&channel.target) else {
            continue;
        };
        let Some(target_node) = bone(&channel.target) else {
            continue;
        };
        let parent_world = |skeleton: &Skeleton, worlds: &[Mat4], n: usize| {
            skeleton.nodes[n].parent.map_or(Mat4::IDENTITY, |p| worlds[p])
        };
        let s_rest = source_rest[node];
        let s_inverse_world = source_world[node].inverse();
        let s_parent_world = parent_world(source, &source_world, node);
        let t_world = target_world[target_node];
        let t_parent_inverse = parent_world(target, &target_world, target_node).inverse();
        // Babylon: targetWorld · sourceInverseWorld · local · sourceParentWorld · targetParentInverse.
        let carry = |local: Mat4| t_parent_inverse * s_parent_world * local * s_inverse_world * t_world;
        let mut out = ClipChannel {
            target: channel.target.clone(),
            ..channel.clone()
        };
        let keys = out.key_values_mut();
        for value in keys {
            match channel.path {
                ChannelPath::Rotation => {
                    let q = Quat::from_xyzw(value[0], value[1], value[2], value[3]);
                    let local = Mat4::from_scale_rotation_translation(s_rest.scale, q, s_rest.translation);
                    let (_, r, _) = decompose(carry(local));
                    value.copy_from_slice(&r.to_array());
                }
                ChannelPath::Translation => {
                    let p = Vec3::new(value[0], value[1], value[2]);
                    let local = Mat4::from_scale_rotation_translation(s_rest.scale, s_rest.rotation, p);
                    let (_, _, t) = decompose(carry(local));
                    value.copy_from_slice(&t.to_array());
                }
                ChannelPath::Scale => {
                    let s = Vec3::new(value[0], value[1], value[2]);
                    let local = Mat4::from_scale_rotation_translation(s, s_rest.rotation, s_rest.translation);
                    let (scale, _, _) = decompose(carry(local));
                    value.copy_from_slice(&scale.to_array());
                }
            }
        }
        channels.push(out);
    }
    AnimationClip {
        name: clip.name.clone(),
        channels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::model::gltf_model::{NodeTransform, SkeletonNode};

    fn node(name: &str, parent: Option<usize>, rotation: Quat, translation: Vec3, scale: f32) -> SkeletonNode {
        SkeletonNode {
            name: name.into(),
            parent,
            rest: NodeTransform {
                translation,
                rotation,
                scale: Vec3::splat(scale),
            },
        }
    }

    #[test]
    fn a_turn_away_from_rest_carries_to_another_rest() {
        // The source arm rests straight; the target's rests turned a quarter
        // round Z under a root exported in centimetres.
        let source = Skeleton {
            nodes: vec![
                node("root", None, Quat::IDENTITY, Vec3::ZERO, 1.0),
                node("arm", Some(0), Quat::IDENTITY, Vec3::Y, 1.0),
            ],
            skins: Vec::new(),
        };
        let rest = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let target = Skeleton {
            nodes: vec![
                node("root", None, Quat::IDENTITY, Vec3::ZERO, 0.01),
                node("arm", Some(0), rest, Vec3::Y * 100.0, 1.0),
            ],
            skins: Vec::new(),
        };
        let turn = Quat::from_rotation_x(0.6);
        let clip = AnimationClip {
            name: "c".into(),
            channels: vec![ClipChannel {
                target: "arm".into(),
                path: ChannelPath::Rotation,
                times: vec![0.0],
                values: turn.to_array().to_vec(),
                cubic: false,
            }],
        };
        let out = retarget_animation_group(&clip, &source, &target, |n| target.find(n));
        let q = Quat::from_slice(&out.channels[0].values);
        // The world turn (about X) is kept: the target's new local is that
        // turn applied to its rest.
        assert!(q.angle_between(turn * rest) < 1e-5, "{q:?}");
        // A bone the target lacks loses its channel.
        let none = retarget_animation_group(&clip, &source, &target, |_| None);
        assert!(none.channels.is_empty());
    }
}
