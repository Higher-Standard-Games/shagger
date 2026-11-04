//! SHAG `src/engine/weapon/WeaponBarrels.ts`: where barrel sub-models sit on
//! a weapon drawn in the world (`findBarrelMount`, `attachWorldBarrels`) and
//! how their materials draw (`prepareBarrelMaterials`,
//! `prepareWorldWeaponMaterials`). Barrels on world models do not spin.
//!
//! On the view model a barrel turns (`WeaponBarrels.update`, `fire`,
//! `reload`): [`BarrelSpin`] holds one barrel's motion, and its caller
//! places the barrel (`load`'s socket and offsets) and draws it.

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

/// `BarrelDef.spinAxis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinAxis {
    #[default]
    X,
    Y,
    Z,
}

/// `BarrelDef`'s motion: how a view barrel spins, steps and slides.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BarrelMotion {
    /// Idle speed, radians a second.
    pub spin_speed: f64,
    /// Speed while firing; `spin_speed` when unset.
    pub firing_spin_speed: Option<f64>,
    /// Seconds to slow from the firing speed back to idle.
    pub spin_down_seconds: Option<f64>,
    /// 1 when unset.
    pub spin_direction: Option<f64>,
    pub spin_axis: SpinAxis,
    /// Degrees turned for each shot, over the refire interval.
    pub fire_step_degrees: Option<f64>,
    /// Model units a sliding part (a bolt) moves back along the spin axis.
    pub slide_distance: Option<f64>,
}

/// `SPIN_UP_SECONDS`.
const SPIN_UP_SECONDS: f64 = 0.15;
/// `DEFAULT_FIRING_HOLD_SECONDS`.
const DEFAULT_FIRING_HOLD_SECONDS: f64 = 0.15;
/// `DEFAULT_STEP_SECONDS`.
const DEFAULT_STEP_SECONDS: f64 = 0.16;
/// `SHOT_SLIDE_SECONDS`.
const SHOT_SLIDE_SECONDS: f64 = 0.1;
/// `RELOAD_RACK_AT`, `RELOAD_RACK_SECONDS`.
const RELOAD_RACK_AT: f64 = 0.45;
const RELOAD_RACK_SECONDS: f64 = 0.35;

/// `slideCycle`: a slide's offset at progress `t`, snapping back and
/// returning more slowly.
#[must_use]
pub fn slide_cycle(t: f64) -> f64 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    if t < 0.3 {
        let back = t / 0.3;
        return 1.0 - (1.0 - back) * (1.0 - back);
    }
    let forward = (t - 0.3) / 0.7;
    1.0 - forward * forward * (3.0 - 2.0 * forward)
}

/// `BarrelInstance`, without its nodes: one view barrel's motion state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarrelSpin {
    spin_speed: f64,
    firing_spin_speed: f64,
    spin_down_seconds: f64,
    speed: f64,
    firing_hold: f64,
    spin_direction: f64,
    pub spin_axis: SpinAxis,
    /// The angle about the spin axis, radians.
    pub rotation: f64,
    target_rotation: f64,
    fire_step_radians: f64,
    step_from: f64,
    step_seconds: f64,
    step_elapsed: f64,
    slide_distance: f64,
    slide_delay: f64,
    slide_seconds: f64,
    slide_elapsed: f64,
    /// How far back the slide is this frame, model units.
    pub slide_back: f64,
}

impl BarrelSpin {
    /// The barrel as `load` pushes it: at rest, idling.
    #[must_use]
    pub fn new(motion: &BarrelMotion) -> Self {
        Self {
            spin_speed: motion.spin_speed,
            firing_spin_speed: motion
                .spin_speed
                .max(motion.firing_spin_speed.unwrap_or(motion.spin_speed)),
            spin_down_seconds: motion.spin_down_seconds.unwrap_or(0.0),
            speed: motion.spin_speed,
            firing_hold: 0.0,
            spin_direction: motion.spin_direction.unwrap_or(1.0),
            spin_axis: motion.spin_axis,
            rotation: 0.0,
            target_rotation: 0.0,
            fire_step_radians: motion.fire_step_degrees.unwrap_or(0.0) * std::f64::consts::PI / 180.0,
            step_from: 0.0,
            step_seconds: 0.0,
            step_elapsed: 0.0,
            slide_distance: motion.slide_distance.unwrap_or(0.0),
            slide_delay: 0.0,
            slide_seconds: 0.0,
            slide_elapsed: 0.0,
            slide_back: 0.0,
        }
    }

    /// `update` for one barrel: the shot's eased step, or the spin up
    /// and down; then the slide.
    pub fn update(&mut self, delta_time: f64) {
        if self.fire_step_radians != 0.0 {
            // Each shot's turn runs eased over the refire interval.
            self.step_elapsed = self.step_seconds.min(self.step_elapsed + delta_time);
            let t = if self.step_seconds > 0.0 {
                self.step_elapsed / self.step_seconds
            } else {
                1.0
            };
            let eased = t * t * (3.0 - 2.0 * t);
            self.rotation = self.step_from + (self.target_rotation - self.step_from) * eased;
        } else {
            // Spin up quickly while firing; after the last shot, slow back
            // to idle over spinDownSeconds.
            let range = self.firing_spin_speed - self.spin_speed;
            self.firing_hold = (self.firing_hold - delta_time).max(0.0);
            if self.firing_hold > 0.0 {
                self.speed = self
                    .firing_spin_speed
                    .min(self.speed + (range / SPIN_UP_SECONDS) * delta_time);
            } else if self.speed > self.spin_speed {
                let rate = if self.spin_down_seconds > 0.0 {
                    range / self.spin_down_seconds
                } else {
                    f64::INFINITY
                };
                self.speed = self.spin_speed.max(self.speed - rate * delta_time);
            }
            self.rotation += self.speed * self.spin_direction * delta_time;
        }
        if self.slide_distance != 0.0 {
            self.update_slide(delta_time);
        }
    }

    /// `fire`: a step over the refire interval, the firing speed held
    /// until the next shot is due, and a slide cycle.
    pub fn fire(&mut self, refire_seconds: Option<f64>) {
        let hold = refire_seconds.map_or(DEFAULT_FIRING_HOLD_SECONDS, |r| r * 1.5);
        if self.slide_distance != 0.0 {
            self.start_slide(
                0.0,
                SHOT_SLIDE_SECONDS.min(refire_seconds.unwrap_or(SHOT_SLIDE_SECONDS)),
            );
        }
        self.step_from = self.rotation;
        self.step_seconds = refire_seconds.unwrap_or(DEFAULT_STEP_SECONDS);
        self.step_elapsed = 0.0;
        self.target_rotation += self.fire_step_radians * self.spin_direction;
        self.firing_hold = hold;
    }

    /// `reload`: a sliding part is racked once.
    pub fn reload(&mut self, seconds: f64) {
        if self.slide_distance != 0.0 {
            self.start_slide(seconds * RELOAD_RACK_AT, RELOAD_RACK_SECONDS);
        }
    }

    fn start_slide(&mut self, delay: f64, seconds: f64) {
        self.slide_delay = delay;
        self.slide_seconds = seconds;
        self.slide_elapsed = 0.0;
    }

    fn update_slide(&mut self, delta_time: f64) {
        let mut offset = 0.0;
        if self.slide_seconds > 0.0 {
            if self.slide_delay > 0.0 {
                self.slide_delay -= delta_time;
            } else {
                self.slide_elapsed += delta_time;
                let t = self.slide_elapsed / self.slide_seconds;
                if t >= 1.0 {
                    self.slide_seconds = 0.0;
                } else {
                    offset = slide_cycle(t);
                }
            }
        }
        self.slide_back = offset * self.slide_distance;
    }

    /// The barrel root's position and Euler rotation, from where `load`
    /// put it (`rest`, `base_rotation`): the spin axis's angle replaced by
    /// the spin, and a sliding part moved back along it.
    #[must_use]
    pub fn pose(&self, rest: Vec3, base_rotation: Vec3) -> (Vec3, Vec3) {
        let mut position = rest;
        let mut rotation = base_rotation;
        let angle = self.rotation as f32;
        let back = self.slide_back as f32;
        let slides = self.slide_distance != 0.0;
        match self.spin_axis {
            SpinAxis::X => {
                rotation.x = angle;
                if slides {
                    position.x = rest.x - back;
                }
            }
            SpinAxis::Y => {
                rotation.y = angle;
                if slides {
                    position.y = rest.y - back;
                }
            }
            SpinAxis::Z => {
                rotation.z = angle;
                if slides {
                    position.z = rest.z - back;
                }
            }
        }
        (position, rotation)
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

    #[test]
    fn a_spinning_barrel_spins_up_on_a_shot_and_down_after() {
        let mut spin = BarrelSpin::new(&BarrelMotion {
            spin_speed: 2.6,
            firing_spin_speed: Some(9.75),
            spin_down_seconds: Some(1.5),
            spin_direction: Some(-1.0),
            ..BarrelMotion::default()
        });
        spin.update(1.0);
        assert!((spin.rotation + 2.6).abs() < 1e-12);
        spin.fire(Some(0.104));
        spin.update(0.15);
        assert!((spin.speed - 9.75).abs() < 1e-9);
        for _ in 0..200 {
            spin.update(0.01);
        }
        assert!((spin.speed - 2.6).abs() < 1e-9);
    }

    #[test]
    fn a_stepping_barrel_turns_one_step_a_shot() {
        let mut spin = BarrelSpin::new(&BarrelMotion {
            fire_step_degrees: Some(405.0),
            ..BarrelMotion::default()
        });
        spin.fire(Some(0.8));
        spin.update(0.4);
        let half = 405f64.to_radians() * 0.5;
        assert!((spin.rotation - half).abs() < 1e-9);
        spin.update(1.0);
        assert!((spin.rotation - 405f64.to_radians()).abs() < 1e-9);
    }

    #[test]
    fn a_slide_cycles_on_a_shot_and_racks_on_a_reload() {
        let mut spin = BarrelSpin::new(&BarrelMotion {
            slide_distance: Some(1.5),
            ..BarrelMotion::default()
        });
        spin.fire(Some(0.104));
        spin.update(0.03);
        assert!((spin.slide_back - 1.5).abs() < 1e-9);
        let (at, _) = spin.pose(Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO);
        assert!((at.x - 0.5).abs() < 1e-6);
        spin.update(0.1);
        assert_eq!(spin.slide_back, 0.0);
        spin.reload(1.0);
        spin.update(0.3);
        assert_eq!(spin.slide_back, 0.0);
        // The delay runs out on this frame; the rack starts on the next.
        spin.update(0.2);
        assert_eq!(spin.slide_back, 0.0);
        spin.update(0.05);
        assert!(spin.slide_back > 0.0);
        assert_eq!(slide_cycle(0.0), 0.0);
        assert_eq!(slide_cycle(0.3), 1.0);
    }
}
