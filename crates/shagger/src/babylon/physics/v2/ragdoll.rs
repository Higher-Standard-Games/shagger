// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, packages/dev/core/src/Physics/v2/ragdoll.ts
// (class Ragdoll), Copyright Babylon.js contributors, licensed under the
// Apache License 2.0 (see ../../LICENSE).
//
// Changes: Havok aggregates and constraints become rapier3d bodies and
// generic joints in shagger's PhysicsSystem; a Babylon Skeleton becomes a
// glTF node hierarchy and its local transforms, with the root transform
// node's world matrix passed in. The joints are inserted when `ragdoll()`
// switches the bodies to dynamic instead of being made disabled up front,
// and `sync` is called by the owner each frame instead of from an
// onBeforeRender observer. `qmesh` is the root's world rotation (the yaw
// the body died facing) rather than the root node's own local rotation.

//! `Ragdoll`: one box body per configured bone, jointed to the nearest
//! configured ancestor, that drives the bones once [`Ragdoll::ragdoll`] hands
//! them to physics.

use glam::{Mat4, Quat, Vec3};
use rapier3d::prelude::{
    ColliderBuilder, GenericJointBuilder, ImpulseJointHandle, JointAxesMask, JointAxis, RigidBodyBuilder,
    RigidBodyHandle, RigidBodyType,
};

use crate::loader::model::gltf_model::{NodeTransform, Skeleton};
use crate::physics::physics_system::{PhysicsSystem, debris_groups, from_rapier, quat_from_rapier, to_rapier};

/// `PhysicsConstraintType`, the two a ragdoll uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsConstraintType {
    BallAndSocket,
    Hinge,
}

/// `RagdollBoneProperties`, as a config entry with its bone named.
#[derive(Debug, Clone, PartialEq)]
pub struct RagdollBoneProperties {
    pub bone: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub depth: Option<f32>,
    pub size: Option<f32>,
    /// `HINGE` when unset (`_defaultJoint`).
    pub joint: Option<PhysicsConstraintType>,
    /// `Axis.X` when unset.
    pub rotation_axis: Option<Vec3>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    /// 10 when unset (`_mass`).
    pub mass: Option<f32>,
    /// 0 when unset (`_restitution`).
    pub restitution: Option<f32>,
    pub box_offset: Option<f32>,
    /// `Axis.Y` when unset.
    pub bone_offset_axis: Option<Vec3>,
    pub put_box_in_bone_center: bool,
}

impl RagdollBoneProperties {
    /// A box `width` by `height` by `depth` on `bone`, the rest unset.
    #[must_use]
    pub fn new(bone: &str, width: f32, height: f32, depth: f32) -> Self {
        Self {
            bone: bone.to_owned(),
            width: Some(width),
            height: Some(height),
            depth: Some(depth),
            size: None,
            joint: None,
            rotation_axis: None,
            min: None,
            max: None,
            mass: None,
            restitution: None,
            box_offset: None,
            bone_offset_axis: None,
            put_box_in_bone_center: false,
        }
    }
}

const DEFAULT_MASS: f32 = 10.0;
const DEFAULT_RESTITUTION: f32 = 0.0;
const FRICTION: f32 = 0.6;

/// A joint waiting for `ragdoll()`.
#[derive(Debug, Clone, Copy)]
struct PendingJoint {
    parent: usize,
    child: usize,
    pivot_a: Vec3,
    pivot_b: Vec3,
    axis: Vec3,
    joint: PhysicsConstraintType,
    limits: Option<[f32; 2]>,
}

/// `Ragdoll`.
#[derive(Debug, Clone)]
pub struct Ragdoll {
    /// Node index of each box's bone (`_bones`).
    bones: Vec<usize>,
    bodies: Vec<RigidBodyHandle>,
    pending: Vec<PendingJoint>,
    constraints: Vec<ImpulseJointHandle>,
    /// Each bone's rotation in model space at creation (`_initialRotation2`).
    initial_rotation2: Vec<Quat>,
    root_bone_index: usize,
    ragdoll_mode: bool,
}

fn rotation_of(m: Mat4) -> Quat {
    m.to_scale_rotation_translation().1.normalize()
}

/// A node's model-space matrix from its local transforms and its parents'.
fn global_of(skeleton: &Skeleton, locals: &[NodeTransform], node: usize) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    let mut at = Some(node);
    let mut depth = 0;
    while let Some(i) = at {
        let local = locals.get(i).map_or(Mat4::IDENTITY, NodeTransform::matrix);
        m = local * m;
        at = skeleton.nodes.get(i).and_then(|n| n.parent);
        depth += 1;
        if depth > skeleton.nodes.len() {
            break;
        }
    }
    m
}

fn parent_global(skeleton: &Skeleton, locals: &[NodeTransform], node: usize) -> Mat4 {
    skeleton
        .nodes
        .get(node)
        .and_then(|n| n.parent)
        .map_or(Mat4::IDENTITY, |p| global_of(skeleton, locals, p))
}

impl Ragdoll {
    /// `new Ragdoll(skeleton, rootTransformNode, config)`: the boxes, placed
    /// where the bones are posed in `locals` under `root_world`, held
    /// kinematic until [`Self::ragdoll`]. `None` when a bone is missing or
    /// the configured bones do not have exactly one root
    /// (`_defineRootBone`), with the boxes already made taken out again.
    pub fn new(
        physics: &mut PhysicsSystem,
        skeleton: &Skeleton,
        locals: &[NodeTransform],
        root_world: Mat4,
        config: &[RagdollBoneProperties],
    ) -> Option<Self> {
        let mut out = Self {
            bones: Vec::with_capacity(config.len()),
            bodies: Vec::with_capacity(config.len()),
            pending: Vec::new(),
            constraints: Vec::new(),
            initial_rotation2: Vec::with_capacity(config.len()),
            root_bone_index: 0,
            ragdoll_mode: false,
        };
        // `_createColliders`: boxes at the bones' world positions, axis
        // aligned (their transforms start unrotated).
        let mut box_positions = Vec::with_capacity(config.len());
        for cfg in config {
            let Some(bone) = skeleton.find(&cfg.bone) else {
                out.dispose(physics);
                return None;
            };
            let global = global_of(skeleton, locals, bone);
            let world = root_world * global;
            let width = cfg.width.or(cfg.size).unwrap_or(1.0);
            let height = cfg.height.or(cfg.size).unwrap_or(1.0);
            let depth = cfg.depth.or(cfg.size).unwrap_or(1.0);
            // A glTF bone has no length; the centre option falls back to 0.
            let box_offset = if cfg.put_box_in_bone_center {
                0.0
            } else {
                cfg.box_offset.unwrap_or(0.0)
            };
            let axis = cfg.bone_offset_axis.unwrap_or(Vec3::Y);
            let bone_dir = world.transform_vector3(axis).normalize_or_zero();
            let position = world.w_axis.truncate() + bone_dir * box_offset;
            let (body, _) = physics.world.insert(
                RigidBodyBuilder::kinematic_position_based()
                    .translation(to_rapier(position))
                    .angular_damping(0.05)
                    .ccd_enabled(true),
                ColliderBuilder::cuboid(width / 2.0, height / 2.0, depth / 2.0)
                    .mass(cfg.mass.unwrap_or(DEFAULT_MASS))
                    .restitution(cfg.restitution.unwrap_or(DEFAULT_RESTITUTION))
                    .friction(FRICTION)
                    .collision_groups(debris_groups()),
            );
            out.bones.push(bone);
            out.bodies.push(body);
            box_positions.push(position);
            out.initial_rotation2.push(rotation_of(global));
        }
        // `_defineRootBone`: the one configured bone no other configured
        // bone sits above.
        let roots: Vec<usize> = (0..out.bones.len())
            .filter(|&i| out.find_nearest_parent(skeleton, i).is_none())
            .collect();
        let [root] = roots[..] else {
            out.dispose(physics);
            return None;
        };
        out.root_bone_index = root;
        // `_initJoints`: the pivot is the bone, seen from the parent's box
        // and from its own.
        for i in 0..out.bones.len() {
            if i == root {
                continue;
            }
            let Some(parent) = out.find_nearest_parent(skeleton, i) else {
                continue;
            };
            let bone_abs = (root_world * global_of(skeleton, locals, out.bones[i]))
                .w_axis
                .truncate();
            let cfg = &config[i];
            let joint = cfg.joint.unwrap_or(PhysicsConstraintType::Hinge);
            let limits = (cfg.min.is_some() || cfg.max.is_some()).then(|| {
                [
                    cfg.min.unwrap_or(-std::f32::consts::PI),
                    cfg.max.unwrap_or(std::f32::consts::PI),
                ]
            });
            out.pending.push(PendingJoint {
                parent,
                child: i,
                pivot_a: bone_abs - box_positions[parent],
                pivot_b: bone_abs - box_positions[i],
                axis: cfg.rotation_axis.unwrap_or(Vec3::X),
                joint,
                limits,
            });
        }
        Some(out)
    }

    /// `_findNearestParent`: the closest ancestor that has a box.
    fn find_nearest_parent(&self, skeleton: &Skeleton, index: usize) -> Option<usize> {
        let mut at = skeleton.nodes.get(self.bones[index]).and_then(|n| n.parent);
        let mut depth = 0;
        while let Some(node) = at {
            if let Some(found) = self.bones.iter().position(|&b| b == node) {
                return Some(found);
            }
            at = skeleton.nodes.get(node).and_then(|n| n.parent);
            depth += 1;
            if depth > skeleton.nodes.len() {
                break;
            }
        }
        None
    }

    /// `getAggregate`: a box's body; the root's for an index out of range.
    #[must_use]
    pub fn get_aggregate(&self, index: usize) -> Option<RigidBodyHandle> {
        self.bodies
            .get(index)
            .or_else(|| self.bodies.get(self.root_bone_index))
            .copied()
    }

    /// Every box's body, in config order.
    #[must_use]
    pub fn bodies(&self) -> &[RigidBodyHandle] {
        &self.bodies
    }

    /// The root box's world position.
    #[must_use]
    pub fn root_position(&self, physics: &PhysicsSystem) -> Option<Vec3> {
        let body = physics.world.bodies.get(self.get_aggregate(self.root_bone_index)?)?;
        Some(from_rapier(body.translation()))
    }

    /// `ragdoll()`: joints on, bodies dynamic.
    pub fn ragdoll(&mut self, physics: &mut PhysicsSystem) {
        if self.ragdoll_mode {
            return;
        }
        self.ragdoll_mode = true;
        for joint in std::mem::take(&mut self.pending) {
            let locked = match joint.joint {
                PhysicsConstraintType::BallAndSocket => JointAxesMask::LOCKED_SPHERICAL_AXES,
                PhysicsConstraintType::Hinge => JointAxesMask::LOCKED_REVOLUTE_AXES,
            };
            let mut builder = GenericJointBuilder::new(locked)
                .local_anchor1(to_rapier(joint.pivot_a))
                .local_anchor2(to_rapier(joint.pivot_b))
                .local_axis1(to_rapier(joint.axis))
                .local_axis2(to_rapier(joint.axis))
                .contacts_enabled(false);
            if let Some(limits) = joint.limits {
                builder = builder.limits(JointAxis::AngX, limits);
            }
            let handle = physics.world.insert_impulse_joint(
                self.bodies[joint.parent],
                self.bodies[joint.child],
                builder.build(),
            );
            self.constraints.push(handle);
        }
        for &body in &self.bodies {
            if let Some(body) = physics.world.bodies.get_mut(body) {
                body.set_body_type(RigidBodyType::Dynamic, true);
            }
        }
    }

    /// `_syncBonesAndBoxes` in ragdoll mode: each bone turns as its box has
    /// turned since creation, and the root bone goes where its box is.
    /// `root_world` is the root transform's world matrix, as at creation.
    pub fn sync(&self, physics: &PhysicsSystem, skeleton: &Skeleton, locals: &mut [NodeTransform], root_world: Mat4) {
        if !self.ragdoll_mode {
            return;
        }
        let q_mesh = rotation_of(root_world);
        let inverse_root = root_world.inverse();
        let order =
            std::iter::once(self.root_bone_index).chain((0..self.bones.len()).filter(|&i| i != self.root_bone_index));
        for i in order {
            let Some(body) = physics.world.bodies.get(self.bodies[i]) else {
                continue;
            };
            let q_phys = quat_from_rapier(*body.rotation());
            // `_setBodyOrientationToBone`: qphys * qmesh * qbind, in world
            // space; brought into the bone's parent's space.
            let world_rotation = q_phys * q_mesh * self.initial_rotation2[i];
            let model_rotation = q_mesh.inverse() * world_rotation;
            let node = self.bones[i];
            let parent = parent_global(skeleton, locals, node);
            if let Some(local) = locals.get_mut(node) {
                local.rotation = (rotation_of(parent).inverse() * model_rotation).normalize();
                if i == self.root_bone_index {
                    let model = inverse_root.transform_point3(from_rapier(body.translation()));
                    local.translation = parent.inverse().transform_point3(model);
                }
            }
        }
    }

    /// `dispose`: the bodies, their boxes and the joints leave the world.
    pub fn dispose(&mut self, physics: &mut PhysicsSystem) {
        for handle in self.constraints.drain(..) {
            physics.world.remove_impulse_joint(handle);
        }
        for handle in self.bodies.drain(..) {
            physics.world.remove_body(handle);
        }
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::model::gltf_model::SkeletonNode;

    /// A two-bone chain: hips at 10 up, a leg 5 below it.
    fn chain() -> (Skeleton, Vec<NodeTransform>) {
        let node = |name: &str, parent, y| SkeletonNode {
            name: name.into(),
            parent,
            rest: NodeTransform {
                translation: Vec3::new(0.0, y, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        };
        let skeleton = Skeleton {
            nodes: vec![node("Hips", None, 10.0), node("Leg", Some(0), -5.0)],
            skins: Vec::new(),
        };
        let locals = skeleton.rest();
        (skeleton, locals)
    }

    fn config() -> Vec<RagdollBoneProperties> {
        vec![
            RagdollBoneProperties::new("Hips", 2.0, 2.0, 2.0),
            RagdollBoneProperties {
                joint: Some(PhysicsConstraintType::BallAndSocket),
                min: Some(-0.5),
                max: Some(0.5),
                ..RagdollBoneProperties::new("Leg", 1.0, 4.0, 1.0)
            },
        ]
    }

    #[test]
    fn boxes_sit_on_the_bones() {
        let (skeleton, locals) = chain();
        let mut physics = PhysicsSystem::new(800.0);
        let root = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
        let ragdoll = Ragdoll::new(&mut physics, &skeleton, &locals, root, &config()).unwrap();
        assert_eq!(ragdoll.root_position(&physics), Some(Vec3::new(100.0, 10.0, 0.0)));
        let leg = physics.world.bodies[ragdoll.bodies()[1]].translation();
        assert_eq!(from_rapier(leg), Vec3::new(100.0, 5.0, 0.0));
        assert_eq!(ragdoll.pending[0].pivot_a, Vec3::new(0.0, -5.0, 0.0));
        assert_eq!(ragdoll.pending[0].pivot_b, Vec3::ZERO);
        assert_eq!(ragdoll.get_aggregate(99), Some(ragdoll.bodies()[0]));
    }

    #[test]
    fn a_missing_bone_or_two_roots_make_no_ragdoll() {
        let (skeleton, locals) = chain();
        let mut physics = PhysicsSystem::new(800.0);
        let mut missing = config();
        missing[1].bone = "Arm".into();
        assert!(Ragdoll::new(&mut physics, &skeleton, &locals, Mat4::IDENTITY, &missing).is_none());
        assert_eq!(physics.body_count(), 0);
        let mut no_leg_parent = skeleton.clone();
        no_leg_parent.nodes[1].parent = None;
        assert!(Ragdoll::new(&mut physics, &no_leg_parent, &locals, Mat4::IDENTITY, &config()).is_none());
        assert_eq!(physics.body_count(), 0);
    }

    #[test]
    fn the_bones_follow_the_falling_boxes() {
        let (skeleton, mut locals) = chain();
        let mut physics = PhysicsSystem::new(800.0);
        let mut ragdoll = Ragdoll::new(&mut physics, &skeleton, &locals, Mat4::IDENTITY, &config()).unwrap();
        ragdoll.ragdoll(&mut physics);
        for _ in 0..30 {
            physics.step(1.0 / 60.0);
        }
        ragdoll.sync(&physics, &skeleton, &mut locals, Mat4::IDENTITY);
        let hips = ragdoll.root_position(&physics).unwrap();
        assert!(hips.y < 0.0, "fell under gravity to {hips}");
        assert!((locals[0].translation - hips).length() < 1e-4);
        // The joint holds the leg 5 under the hips.
        let leg = from_rapier(physics.world.bodies[ragdoll.bodies()[1]].translation());
        assert!(((leg - hips).length() - 5.0).abs() < 0.5, "leg {leg} hips {hips}");
        ragdoll.dispose(&mut physics);
        assert_eq!(physics.body_count(), 0);
    }
}
