//! SHAG `src/engine/weapon/WeaponViewManager.ts`: where the first-person
//! weapon sits and how it is drawn.
//!
//! Ported so far: the weapon camera (its own FOV, near 0.1, far 1000, a
//! depth clear before it draws), the weapon-only fill light and the blade's
//! specular light, a GLB view model's orientation and material fix-ups
//! (`loadGlbSource`), the hip placement with handedness (`setupWeapon`,
//! `applyPosition`), and per frame the aim-down-sights lerp, `WeaponBob`
//! and `WeaponRecoil` (`updateWeaponBob`), the recoil and melee swing of
//! `presentFire`, and the reload and switch animations (the switch runs
//! in the holder, which swaps models, and hands its offsets in).
//!
//! Not yet: MD3 view models, the holster and blade charge animations, the
//! slash trail, the muzzle flash light and model, the on-weapon ammo
//! counter, spinning barrels, the quad outline, the underwater drift and the light
//! grid tint (`WeaponLighting`, which only reaches StandardMaterial layers
//! and the blade's light).

use glam::{DVec3, Mat4, Vec3};

use super::weapon_barrels::babylon_euler;
use super::weapon_bob::{BobInput, GunBobVars, WeaponBob};
use super::weapon_recoil::WeaponRecoil;
use super::weapon_reload_anim::WeaponReloadAnim;
use super::weapon_slash::WeaponSlash;
use super::weapon_switch_anim::SwitchAnimOffsets;
use crate::babylon::materials::standard_material::{StandardMaterialParams, TextureUse};
use crate::loader::model::gltf_model::ModelMaterial;
use crate::rendering::model_renderer::{ModelLight, ModelMaterialMode, Outline};

/// The weapon camera's near plane.
pub const WEAPON_CAMERA_MIN_Z: f32 = 0.1;
/// The weapon camera's far plane.
pub const WEAPON_CAMERA_MAX_Z: f32 = 1000.0;
/// `cg_weaponfov`'s default, in degrees.
pub const DEFAULT_WEAPON_FOV: f64 = 60.0;
/// `cl_ads_time`'s default.
pub const DEFAULT_ADS_TIME: f64 = 0.08;
/// `cl_ads_offset_y`'s default.
pub const DEFAULT_ADS_OFFSET_Y: f64 = 2.0;

/// `HAND_OFFSETS` (`cg_handedness` style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Handedness {
    Left,
    #[default]
    Right,
    Center,
}

impl Handedness {
    /// `cg_handedness`: 0 left, 1 right, 2 centre.
    #[must_use]
    pub fn from_cvar(value: i32) -> Self {
        match value {
            0 => Self::Left,
            2 => Self::Center,
            _ => Self::Right,
        }
    }

    /// The hand's X offset.
    #[must_use]
    pub fn offset(self) -> f64 {
        match self {
            Self::Right => 12.0,
            Self::Center => 0.0,
            Self::Left => -12.0,
        }
    }
}

/// `WeaponPosition`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponPosition {
    /// Offset from the camera.
    pub offset: DVec3,
    /// Euler rotation.
    pub rotation: DVec3,
    pub scale: f64,
}

/// The parts of a `WeaponDef` the view model reads (`viewModel`, `ads`,
/// `adsRequireReady`, `recoilScale`, `meleeRadius`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewModelDef {
    pub hip_offset: DVec3,
    pub hip_rotation: DVec3,
    pub ads_offset: Option<DVec3>,
    pub scale: f64,
    /// `ads.time`.
    pub ads_time: Option<f64>,
    pub ads_require_ready: bool,
}

/// `weaponLight`: a hemispheric fill for the weapon layer only.
#[must_use]
pub fn weapon_light() -> ModelLight {
    ModelLight::Hemispheric {
        direction: Vec3::new(0.3, 1.0, 0.5),
        diffuse: Vec3::new(1.0, 0.95, 0.9),
        specular: Vec3::ONE,
        ground: Vec3::new(0.3, 0.3, 0.35),
        intensity: 1.2,
    }
}

/// `bladeLight`: the blade's specular source, a directional light parented
/// to the weapon camera; `camera_rotation` turns its direction to world.
#[must_use]
pub fn blade_light(camera_rotation: glam::Quat) -> ModelLight {
    ModelLight::Directional {
        direction: camera_rotation * Vec3::new(0.3, -0.4, 1.0),
        diffuse: Vec3::ONE,
        specular: Vec3::ONE,
        intensity: 3.0,
    }
}

/// `loadGlbSource`'s orientation of the glTF `__root__` (Euler angles; its
/// quaternion is cleared, its (1, 1, -1) scaling kept).
#[must_use]
pub fn glb_root_rotation(weapon: &str) -> Vec3 {
    if weapon == "blade" {
        // Flip the barrel and tilt it up for a melee carry.
        Vec3::new(std::f32::consts::PI, 0.9, 0.0)
    } else {
        // IQM-to-GLB weapons: barrel from +X to -Z.
        Vec3::new(0.0, -std::f32::consts::FRAC_PI_2, 0.0)
    }
}

/// The `__root__` node's matrix in the weapon node's space.
#[must_use]
pub fn glb_root_matrix(weapon: &str) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::new(1.0, 1.0, -1.0),
        babylon_euler(glb_root_rotation(weapon)),
        Vec3::ZERO,
    )
}

/// `loadGlbSource`'s material fix-ups for a gun (the blade keeps its
/// authored PBR): `_glow` layers draw as added light, `_emit` layers as a
/// self-lit, translucent teal StandardMaterial, `_glass` at alpha 0.35, and
/// every other layer lit from both sides.
#[must_use]
pub fn view_material_mode(weapon: &str, material: &ModelMaterial) -> ModelMaterialMode {
    if weapon == "blade" {
        return ModelMaterialMode::Lit;
    }
    let name = material.name.to_lowercase();
    if name.ends_with("_glow") {
        ModelMaterialMode::AddedLight
    } else if name.contains("_emit") && material.base_color_image.is_some() {
        ModelMaterialMode::Standard(StandardMaterialParams {
            diffuse_color: Vec3::ZERO,
            specular_color: Vec3::ZERO,
            emissive_color: Vec3::new(0.5, 1.0, 1.0),
            alpha: 0.5,
            disable_lighting: true,
            back_face_culling: false,
            alpha_blend: true,
            texture: TextureUse {
                emissive: true,
                ..TextureUse::default()
            },
            ..StandardMaterialParams::default()
        })
    } else if name.contains("_glass") {
        ModelMaterialMode::Glass(0.35)
    } else {
        ModelMaterialMode::LitTwoSided
    }
}

/// `r_cel_weapon_width`'s default as Babylon takes it (the cvar's 15 / 100).
pub const WEAPON_OUTLINE_WIDTH: f32 = 0.15;

/// `applyWeaponOutlines`: a black outline on every layer but the added
/// light ones, its width divided by the root's scale (GLB weapons are
/// scaled up, so outlines would read thicker).
#[must_use]
pub fn weapon_outline(mode: &ModelMaterialMode, root_scale: f64) -> Option<Outline> {
    if *mode == ModelMaterialMode::AddedLight {
        return None;
    }
    let scale = if root_scale == 0.0 { 1.0 } else { root_scale as f32 };
    Some(Outline {
        color: Vec3::ZERO,
        width: WEAPON_OUTLINE_WIDTH / scale,
    })
}

/// What `presentFire` reads from the weapon's definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotStyle {
    /// `recoilScale`, 1 when unset.
    pub recoil_scale: f64,
    /// `adsRecoilScale`, 0.2 when unset.
    pub ads_recoil_scale: f64,
    /// `fireIntervalTicks / tick rate`.
    pub refire_seconds: Option<f64>,
    /// `meleeRadius` is set: the shot is a swing.
    pub melee: bool,
}

impl Default for ShotStyle {
    fn default() -> Self {
        Self {
            recoil_scale: 1.0,
            ads_recoil_scale: 0.2,
            refire_seconds: None,
            melee: false,
        }
    }
}

/// What the view model reads about the player each frame
/// (`ViewportEffectsManager`'s state, or a followed player's).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewModelInput {
    pub delta_time: f64,
    pub grounded: bool,
    pub horizontal_speed: f64,
    pub velocity_y: f64,
    pub landing_velocity: f64,
    pub bob_cycle: f64,
    pub aiming: bool,
    pub reloading: bool,
}

/// The view model's state (`WeaponViewManager`, the parts ported).
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponView {
    pub weapon: String,
    pub def: Option<ViewModelDef>,
    pub current_position: WeaponPosition,
    /// The base position with handedness, which the bob moves about.
    pub effective_base_position: DVec3,
    /// 0 hip, 1 fully aimed.
    pub ads_progress: f64,
    pub bob: WeaponBob,
    pub recoil: WeaponRecoil,
    pub bob_vars: GunBobVars,
    pub handedness: Handedness,
    /// `cg_weaponfov`, degrees.
    pub fov: f64,
    /// The root's position, Euler rotation and scaling this frame.
    pub position: DVec3,
    pub rotation: DVec3,
    pub scale: f64,
    /// This view's clock, seconds: the animations' `performance.now()`.
    pub clock: f64,
    pub slash: WeaponSlash,
    pub reload_anim: WeaponReloadAnim,
    /// The holder's switch slide this frame.
    pub switch_offsets: SwitchAnimOffsets,
}

impl WeaponView {
    /// `loadWeapon` and `setupWeapon` for a GLB view model: the hip
    /// position from its definition, applied (`applyPosition`).
    #[must_use]
    pub fn new(weapon: &str, def: Option<ViewModelDef>) -> Self {
        let current_position = def.map_or(
            WeaponPosition {
                offset: DVec3::new(12.0, -25.0, 19.0),
                rotation: DVec3::ZERO,
                scale: 2.0,
            },
            |d| WeaponPosition {
                offset: d.hip_offset,
                rotation: d.hip_rotation,
                scale: d.scale,
            },
        );
        let mut view = Self {
            weapon: weapon.to_owned(),
            def,
            current_position,
            effective_base_position: DVec3::ZERO,
            ads_progress: 0.0,
            bob: WeaponBob::default(),
            recoil: WeaponRecoil::default(),
            bob_vars: GunBobVars::default(),
            handedness: Handedness::Right,
            fov: DEFAULT_WEAPON_FOV,
            position: DVec3::ZERO,
            rotation: DVec3::ZERO,
            scale: 1.0,
            clock: 0.0,
            slash: WeaponSlash::default(),
            reload_anim: WeaponReloadAnim::default(),
            switch_offsets: SwitchAnimOffsets::default(),
        };
        view.apply_position();
        view
    }

    /// `applyPosition`: X is the hand's offset, Y and Z the position's.
    pub fn apply_position(&mut self) {
        let pos = self.current_position;
        let base = DVec3::new(self.handedness.offset(), pos.offset.y, pos.offset.z);
        self.position = base;
        self.rotation = pos.rotation;
        self.scale = pos.scale;
        self.effective_base_position = base;
    }

    /// `presentFire`'s motion: the recoil, timed to the fire rate, and a
    /// melee weapon's swing (not for a charged throw).
    pub fn present_fire(&mut self, shot: &ShotStyle, projectile_fire: bool) {
        self.recoil.fire(
            shot.recoil_scale,
            self.ads_progress,
            shot.ads_recoil_scale,
            shot.refire_seconds,
        );
        if shot.melee && !projectile_fire {
            self.slash.trigger(self.clock * 1000.0);
        }
    }

    /// `startReload`.
    pub fn start_reload(&mut self, duration: f64) {
        self.reload_anim.start(duration, self.clock);
    }

    /// `cancelReload`.
    pub fn cancel_reload(&mut self) {
        self.reload_anim.cancel();
    }

    /// `updateWeaponBob` (with `WeaponRecoil.update`, which
    /// `updateMuzzleFlash` steps): bob, aim, recoil, reload, switch and
    /// swing on the base position.
    pub fn update(&mut self, input: &ViewModelInput) {
        let dt = input.delta_time;
        self.clock += dt;
        self.recoil.update(dt);
        let reload = self.reload_anim.offsets(self.clock);
        let reloading = input.reloading || self.reload_anim.is_reloading();
        let switch = self.switch_offsets;
        let slash = self.slash.offsets(self.clock * 1000.0);
        let base = self.effective_base_position;
        self.bob.update(
            &BobInput {
                delta_time: dt,
                grounded: input.grounded,
                horizontal_speed: input.horizontal_speed,
                velocity_y: input.velocity_y,
                landing_velocity: input.landing_velocity,
                bob_cycle: input.bob_cycle,
                reloading,
                ads_progress: self.ads_progress,
                hand_offset_x: base.x,
            },
            &self.bob_vars,
        );
        let bob = self.bob.state();

        // Aiming down sights.
        let ads_time = self
            .def
            .and_then(|d| d.ads_time)
            .filter(|t| *t != 0.0)
            .unwrap_or(DEFAULT_ADS_TIME);
        let ads_speed = 1.0 / ads_time.max(0.05);
        let target_ads = if input.aiming && !reloading { 1.0 } else { 0.0 };
        if self.def.is_some_and(|d| d.ads_require_ready) {
            let step = dt * ads_speed;
            self.ads_progress = if target_ads > self.ads_progress {
                target_ads.min(self.ads_progress + step)
            } else {
                target_ads.max(self.ads_progress - step)
            };
        } else {
            let ads_lerp = (dt * ads_speed).min(1.0);
            self.ads_progress += (target_ads - self.ads_progress) * ads_lerp;
        }
        let ads_offset = self.def.and_then(|d| d.ads_offset);
        let ads_target = ads_offset.unwrap_or(DVec3::new(0.0, base.y + DEFAULT_ADS_OFFSET_Y, base.z));
        let ads_x = base.x + (ads_target.x - base.x) * self.ads_progress;
        let ads_y = (ads_target.y - base.y) * self.ads_progress;
        let ads_z = (ads_target.z - base.z) * self.ads_progress;

        self.position = DVec3::new(
            ads_x + bob.bob_x + reload.x_offset + switch.x_offset + slash.x_offset,
            base.y + bob.bob_y + bob.jump_land_bob + ads_y + reload.y_offset + switch.y_offset + slash.y_offset,
            base.z + ads_z + reload.z_offset - self.recoil.offset + switch.z_offset + slash.z_offset,
        );
        // Aimed, the sprint tilt drops by 80%.
        let sprint_factor = 1.0 - self.ads_progress * 0.8;
        let base_rot = self.current_position.rotation;
        let degrees = std::f64::consts::PI / 180.0;
        self.rotation = DVec3::new(
            base_rot.x + bob.sprint_tilt * sprint_factor * degrees + reload.pitch + slash.pitch
                - self.recoil.pitch_offset,
            base_rot.y + bob.sprint_yaw * sprint_factor * degrees + slash.yaw,
            base_rot.z + bob.sprint_roll * sprint_factor * degrees + reload.roll + switch.roll + slash.roll,
        );
    }

    /// The weapon root's matrix in the camera's space.
    #[must_use]
    pub fn root_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(self.scale as f32),
            babylon_euler(self.rotation.as_vec3()),
            self.position.as_vec3(),
        )
    }

    /// The glTF scene's matrix in world space, under a camera whose world
    /// matrix (the inverse of its view) is `camera_world`.
    #[must_use]
    pub fn model_matrix(&self, camera_world: Mat4) -> Mat4 {
        camera_world * self.root_matrix() * glb_root_matrix(&self.weapon)
    }

    /// The weapon camera's vertical field of view, radians.
    #[must_use]
    pub fn fov_radians(&self) -> f32 {
        (self.fov * std::f64::consts::PI / 180.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plasmagun() -> ViewModelDef {
        ViewModelDef {
            hip_offset: DVec3::new(0.0, -25.0, -1.0),
            hip_rotation: DVec3::ZERO,
            ads_offset: Some(DVec3::new(0.0, -22.0, -5.0)),
            scale: 2.0,
            ads_time: None,
            ads_require_ready: false,
        }
    }

    #[test]
    fn the_hip_takes_the_hand_offset() {
        let view = WeaponView::new("plasmagun", Some(plasmagun()));
        assert_eq!(view.position, DVec3::new(12.0, -25.0, -1.0));
        assert_eq!(view.scale, 2.0);
    }

    #[test]
    fn aiming_moves_to_the_sights() {
        let mut view = WeaponView::new("plasmagun", Some(plasmagun()));
        for _ in 0..120 {
            view.update(&ViewModelInput {
                delta_time: 1.0 / 60.0,
                grounded: true,
                aiming: true,
                ..ViewModelInput::default()
            });
        }
        assert!((view.position - DVec3::new(0.0, -22.0, -5.0)).length() < 1e-6);
    }

    #[test]
    fn a_shot_kicks_and_a_blade_swings() {
        let still = ViewModelInput {
            delta_time: 0.01,
            grounded: true,
            ..ViewModelInput::default()
        };
        let mut gun = WeaponView::new("plasmagun", Some(plasmagun()));
        gun.update(&still);
        let rest = gun.position;
        gun.present_fire(&ShotStyle::default(), false);
        for _ in 0..3 {
            gun.update(&still);
        }
        assert!(gun.position.z < rest.z - 1.0, "{} {}", gun.position.z, rest.z);
        assert!(!gun.slash.is_active());

        let mut blade = WeaponView::new("blade", Some(plasmagun()));
        blade.update(&still);
        let rest = blade.position;
        let melee = ShotStyle {
            melee: true,
            ..ShotStyle::default()
        };
        blade.present_fire(&melee, false);
        for _ in 0..6 {
            blade.update(&still);
        }
        // Up in the windup.
        assert!(blade.position.y > rest.y + 20.0, "{} {}", blade.position.y, rest.y);
        for _ in 0..60 {
            blade.update(&still);
        }
        assert!(!blade.slash.is_active());
        assert!((blade.position - rest).length() < 1e-6);
    }

    #[test]
    fn a_reload_tilts_the_gun_and_drops_the_aim() {
        let mut gun = WeaponView::new("plasmagun", Some(plasmagun()));
        gun.start_reload(1.0);
        let input = ViewModelInput {
            delta_time: 0.05,
            grounded: true,
            aiming: true,
            ..ViewModelInput::default()
        };
        for _ in 0..8 {
            gun.update(&input);
        }
        assert!(gun.rotation.z > 0.7, "{}", gun.rotation.z);
        assert_eq!(gun.ads_progress, 0.0);
    }

    #[test]
    fn a_gun_points_its_barrel_forward() {
        // The converted barrel runs along +X; the root turns it to the
        // camera's forward (+Z), the handedness flip included.
        let m = glb_root_matrix("plasmagun");
        let tip = m.transform_vector3(Vec3::X);
        assert!(tip.z.abs() > 0.99, "{tip}");
    }
}
