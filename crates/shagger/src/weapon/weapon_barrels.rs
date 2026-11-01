//! SHAG `src/engine/weapon/WeaponBarrels.ts`: where barrel sub-models sit on
//! a weapon drawn in the world (`findBarrelMount`, `attachWorldBarrels`) and
//! how their materials draw (`prepareBarrelMaterials`,
//! `prepareWorldWeaponMaterials`). Barrels on world models do not spin.

use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::loader::model::gltf_model::{AttachmentPoint, ModelMaterial};
use crate::loader::model::tags_loader::{TagDef, find_tag};
use crate::rendering::model_renderer::ModelMaterialMode;

/// `BarrelDef`, the parts a world model uses.
#[derive(Debug, Clone, PartialEq)]
pub struct BarrelDef {
    pub model_path: String,
    /// Offset from the mount, in model space.
    pub offset: Option<Vec3>,
}

/// `BarrelMount`: where a world weapon model carries its barrels, in the
/// leader mesh's space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarrelMount {
    pub position: Vec3,
    /// Euler angles, used when there is no quaternion.
    pub rotation: Vec3,
    pub rotation_quaternion: Option<Quat>,
}

/// `findBarrelMount`. `tags` is the first barrel's directory `tags.txt`.
/// The fallback to a skeleton bone's node, for a tag naming a bone that is
/// not among the attachment points, is left out: every Washoi barrel tag
/// names an authored `tag_*` node.
#[must_use]
pub fn find_barrel_mount(
    tags: &[TagDef],
    attachment_points: &std::collections::BTreeMap<String, AttachmentPoint>,
) -> BarrelMount {
    let tag = find_tag(tags, "tag_barrel");
    let attachment = tag
        .filter(|t| !t.bone_name.is_empty())
        .and_then(|t| attachment_points.get(&t.bone_name))
        .or_else(|| attachment_points.get("tag_barrel"));
    let mut position = attachment.map_or(Vec3::ZERO, |a| a.position);
    if let Some(tag) = tag {
        position += tag.offset;
    }
    BarrelMount {
        position,
        rotation_quaternion: attachment.map(|a| a.rotation),
        // A glTF node's rotation lives in its quaternion; its Euler angles stay zero.
        rotation: match (attachment, tag) {
            (Some(_), _) | (None, None) => Vec3::ZERO,
            (None, Some(tag)) => tag.rotation,
        },
    }
}

/// Babylon's `Quaternion.RotationYawPitchRoll(y, x, z)` for a `rotation`.
#[must_use]
pub fn babylon_euler(rotation: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, rotation.y, rotation.x, rotation.z)
}

/// The barrel's `__root__` in the leader's space (`attachWorldBarrels`): at
/// the mount plus the barrel's offset, turned by the mount, keeping the
/// loader's handedness flip (scaling (1, 1, -1)) and nothing else of it.
#[must_use]
pub fn barrel_root_matrix(mount: &BarrelMount, barrel: &BarrelDef) -> Mat4 {
    let rotation = mount
        .rotation_quaternion
        .unwrap_or_else(|| babylon_euler(mount.rotation));
    Mat4::from_scale_rotation_translation(
        Vec3::new(1.0, 1.0, -1.0),
        rotation,
        mount.position + barrel.offset.unwrap_or(Vec3::ZERO),
    )
}

/// `prepareBarrelMaterials`: energy layers (`_plasma`, `_halo`, `_glow`,
/// `_emit`) draw as added light, `_glass` translucent at alpha 0.35.
#[must_use]
pub fn barrel_material_mode(material: &ModelMaterial) -> ModelMaterialMode {
    let name = material.name.to_lowercase();
    if ["_plasma", "_halo", "_glow", "_emit"].iter().any(|s| name.ends_with(s)) {
        ModelMaterialMode::AddedLight
    } else if name.ends_with("_glass") {
        ModelMaterialMode::Glass(0.35)
    } else {
        ModelMaterialMode::Lit
    }
}

/// `prepareWorldWeaponMaterials`: only `_glow` layers draw as added light.
#[must_use]
pub fn world_weapon_material_mode(material: &ModelMaterial) -> ModelMaterialMode {
    if material.name.to_lowercase().ends_with("_glow") {
        ModelMaterialMode::AddedLight
    } else {
        ModelMaterialMode::Lit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::model::tags_loader::parse_tags;
    use std::collections::BTreeMap;

    fn points() -> BTreeMap<String, AttachmentPoint> {
        let mut p = BTreeMap::new();
        p.insert(
            "tag_barrel".to_owned(),
            AttachmentPoint {
                position: Vec3::new(1.0, 2.0, 3.0),
                rotation: Quat::from_rotation_x(0.5),
            },
        );
        p.insert(
            "tag_barrel2".to_owned(),
            AttachmentPoint {
                position: Vec3::new(4.0, 5.0, 6.0),
                rotation: Quat::IDENTITY,
            },
        );
        p
    }

    #[test]
    fn a_tag_naming_a_bone_picks_that_node() {
        let tags = parse_tags("tag \"tag_barrel2\" \"tag_barrel\" 1 0 0 0 0 0");
        let mount = find_barrel_mount(&tags, &points());
        assert_eq!(mount.position, Vec3::new(5.0, 5.0, 6.0));
        assert_eq!(mount.rotation_quaternion, Some(Quat::IDENTITY));
    }

    #[test]
    fn without_tags_the_tag_barrel_node_is_the_mount() {
        let mount = find_barrel_mount(&[], &points());
        assert_eq!(mount.position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(mount.rotation_quaternion, Some(Quat::from_rotation_x(0.5)));
        let none = find_barrel_mount(&[], &BTreeMap::new());
        assert_eq!(none.position, Vec3::ZERO);
        assert_eq!(none.rotation_quaternion, None);
    }

    #[test]
    fn the_barrel_root_keeps_the_handedness_flip() {
        let mount = find_barrel_mount(&[], &BTreeMap::new());
        let barrel = BarrelDef {
            model_path: String::new(),
            offset: Some(Vec3::new(0.0, 0.0, -4.0)),
        };
        let m = barrel_root_matrix(&mount, &barrel);
        assert_eq!(m.transform_point3(Vec3::new(0.0, 0.0, 1.0)), Vec3::new(0.0, 0.0, -5.0));
    }

    #[test]
    fn babylon_yaw_pitch_roll_order() {
        // Yaw a quarter turn: +Z goes to +X in Babylon's left-handed space.
        let q = babylon_euler(Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0));
        assert!((q * Vec3::Z - Vec3::X).length() < 1e-6);
    }
}
