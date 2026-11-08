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
//! The muzzle (`getMuzzleWorldPosition`): the model's `tag_flash` seen
//! through the weapon camera, then put on the main camera's matching
//! screen ray, or the camera fallback for a model without one.
//!
//! Barrel sub-models turn and slide with the shots and reloads
//! (`WeaponBarrels`); the caller places and draws them.
//!
//! The blade's charge (`WeaponCharge`): aiming a melee weapon shakes it
//! and puts the charge's outline on it.
//!
//! The cvars it reads each frame ([`ViewModelCvars`]: `cg_handedness`,
//! `cg_weaponfov`, `cl_gunbob*`, `cl_ads_time`, `cl_ads_offset_y` and the
//! `cl_wep_*` overrides), the ladder's holster (`WeaponHolsterAnim`,
//! `setMovementHidden`), the light grid tint (`WeaponLighting`, which shows
//! on the blade's light) and the underwater drift of the tagless muzzle.
//!
//! One `WeaponViewManager` holds every weapon in the TypeScript, so the bob,
//! the aim, the recoil and the swing carry over a switch; a holder of one
//! [`WeaponView`] per weapon hands them on with [`WeaponView::carry_from`].
//!
//! The muzzle flash's light on the gun (`muzzleFlashLight`): lit at a
//! shot, fading at `MUZZLE_FLASH_DECAY`; the caller puts it at the socket.
//! A socket's screen ray, jittered (`getBladeTipWorldPosition`).
//!
//! The blade reflects `BladeEnvironment`'s fill ([`blade_reflection`]).
//!
//! Not yet: MD3 view models.

use glam::{DVec3, Mat4, Vec3};

use super::weapon_barrels::{BarrelSpin, babylon_euler};
use super::weapon_bob::{BobInput, GunBobVars, WeaponBob};
use super::weapon_charge::WeaponCharge;
use super::weapon_holster_anim::WeaponHolsterAnim;
use super::weapon_lighting::WeaponLighting;
use super::weapon_recoil::WeaponRecoil;
use super::weapon_reload_anim::WeaponReloadAnim;
use super::weapon_slash::WeaponSlash;
use super::weapon_switch_anim::SwitchAnimOffsets;
use crate::babylon::materials::standard_material::{StandardMaterialParams, TextureUse};
use crate::core::cvar::Cvars;
use crate::loader::bsp::light_grid_sampling::LightSample;
use crate::loader::model::gltf_model::ModelMaterial;
use crate::rendering::model_renderer::{ModelLight, ModelMaterialMode, Outline, ReflectionFill};

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
/// Underwater drift: slow enough to read as buoyancy, not as sway.
const WEAPON_FLOAT_HZ: f64 = 0.35;
/// Peak drift in world units.
const WEAPON_FLOAT_AMPLITUDE: f64 = 0.55;
/// Constant droop: the weapon hangs a little heavier in the water.
const WEAPON_FLOAT_SAG: f64 = 0.35;

/// The `MovementVars` the view model reads. A `cl_wep_*` override of 0
/// leaves the weapon's own value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewModelCvars {
    pub handedness: Handedness,
    /// `cg_weaponfov`, degrees.
    pub fov: f64,
    pub bob: GunBobVars,
    /// `cl_ads_time`.
    pub ads_time: f64,
    /// `cl_ads_offset_y`.
    pub ads_offset_y: f64,
    /// `cl_wep_hip_x`, `_y`, `_z`.
    pub hip: DVec3,
    /// `cl_wep_ads_x`, `_y`, `_z`.
    pub ads: DVec3,
    /// `cl_wep_scale`.
    pub scale: f64,
}

impl Default for ViewModelCvars {
    fn default() -> Self {
        Self {
            handedness: Handedness::Right,
            fov: DEFAULT_WEAPON_FOV,
            bob: GunBobVars::default(),
            ads_time: DEFAULT_ADS_TIME,
            ads_offset_y: DEFAULT_ADS_OFFSET_Y,
            hip: DVec3::ZERO,
            ads: DVec3::ZERO,
            scale: 0.0,
        }
    }
}

impl ViewModelCvars {
    /// Reads them, each at its `MovementVars` default when unset.
    #[must_use]
    pub fn read(cvars: &Cvars) -> Self {
        let d = Self::default();
        let num = |name: &str, fallback: f64| {
            if cvars.exists(name) {
                cvars.get_number(name)
            } else {
                fallback
            }
        };
        let vec = |prefix: &str| {
            DVec3::new(
                num(&format!("{prefix}_x"), 0.0),
                num(&format!("{prefix}_y"), 0.0),
                num(&format!("{prefix}_z"), 0.0),
            )
        };
        #[allow(clippy::cast_possible_truncation)]
        let hand = num("cg_handedness", 1.0) as i32;
        Self {
            handedness: Handedness::from_cvar(hand),
            fov: num("cg_weaponfov", d.fov),
            bob: GunBobVars {
                enabled: num("cl_gunbob", 1.0) > 0.0,
                amplitude: num("cl_gunbob_amplitude", d.bob.amplitude),
                speed: num("cl_gunbob_speed", d.bob.speed),
                sprint_tilt: num("cl_gunbob_sprint_tilt", d.bob.sprint_tilt),
                sprint_inward: num("cl_gunbob_sprint_inward", d.bob.sprint_inward),
                jump: num("cl_gunbob_jump", d.bob.jump),
                land: num("cl_gunbob_land", d.bob.land),
            },
            ads_time: num("cl_ads_time", d.ads_time),
            ads_offset_y: num("cl_ads_offset_y", d.ads_offset_y),
            hip: vec("cl_wep_hip"),
            ads: vec("cl_wep_ads"),
            scale: num("cl_wep_scale", 0.0),
        }
    }
}

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
    /// `meleeRadius` is set: aiming charges the weapon instead.
    pub melee: bool,
    /// `chargeTime`, seconds.
    pub charge_time: Option<f64>,
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
/// `color` is `WeaponLighting`'s, its diffuse and specular.
#[must_use]
pub fn blade_light(camera_rotation: glam::Quat, color: Vec3) -> ModelLight {
    ModelLight::Directional {
        direction: camera_rotation * Vec3::new(0.3, -0.4, 1.0),
        diffuse: color,
        specular: color,
        intensity: 3.0,
    }
}

/// `loadGlbSource`'s blade materials: `BladeEnvironment` as the reflection
/// texture at `environmentIntensity` 1.5; `WeaponLighting` sets its
/// `reflectionColor` to `color`. `None` for the guns.
#[must_use]
pub fn blade_reflection(weapon: &str, color: Vec3) -> Option<ReflectionFill> {
    (weapon == "blade").then_some(ReflectionFill { color, intensity: 1.5 })
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
    /// `muzzleFlash`'s light on the gun: colour, intensity and range.
    pub flash: Option<FlashLight>,
}

impl Default for ShotStyle {
    fn default() -> Self {
        Self {
            recoil_scale: 1.0,
            ads_recoil_scale: 0.2,
            refire_seconds: None,
            melee: false,
            flash: None,
        }
    }
}

/// A weapon definition's `muzzleFlash`, as its light on the gun reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlashLight {
    pub color: Vec3,
    /// `MUZZLE_FLASH_MAX_INTENSITY` when unset.
    pub intensity: Option<f64>,
    /// `MUZZLE_FLASH_RANGE` when unset.
    pub range: Option<f64>,
}

/// `MUZZLE_FLASH_MAX_INTENSITY`: the flash light's peak, and what its
/// intensity is measured against.
pub const MUZZLE_FLASH_MAX_INTENSITY: f64 = 15.0;
/// `MUZZLE_FLASH_DECAY`: intensity lost a second (half for the blade).
pub const MUZZLE_FLASH_DECAY: f64 = 150.0;
/// `MUZZLE_FLASH_RANGE`, units.
pub const MUZZLE_FLASH_RANGE: f64 = 30.0;

/// The muzzle flash's point light on the view model this frame
/// (`muzzleFlashLight`, which lights only the weapon layer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MuzzleFlashLight {
    pub diffuse: Vec3,
    pub specular: Vec3,
    pub intensity: f64,
    pub range: f64,
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
    pub cvars: ViewModelCvars,
    /// The root's position, Euler rotation and scaling this frame.
    pub position: DVec3,
    pub rotation: DVec3,
    pub scale: f64,
    /// This view's clock, seconds: the animations' `performance.now()`.
    pub clock: f64,
    pub slash: WeaponSlash,
    pub charge: WeaponCharge,
    /// The barrel sub-models' motion (`WeaponBarrels`), in the order the
    /// caller loaded them.
    pub barrels: Vec<BarrelSpin>,
    /// The charge's outline this frame (its width before the root's
    /// scale), worn in place of the black one.
    pub charge_outline: Option<Outline>,
    pub reload_anim: WeaponReloadAnim,
    /// The holder's switch slide this frame.
    pub switch_offsets: SwitchAnimOffsets,
    /// `WeaponHolsterAnim`: stowed while climbing.
    pub holster: WeaponHolsterAnim,
    /// `WeaponLighting`: the light grid's colour at the camera, eased.
    pub lighting: WeaponLighting,
    /// `submersionAmount`: how deep the camera is under liquid, 0 to 1.
    pub submersion_amount: f64,
    /// `muzzleFlashIntensity`: from the flash's intensity at a shot down to
    /// 0 at `MUZZLE_FLASH_DECAY` a second.
    pub muzzle_flash_intensity: f64,
    /// The last shot's flash light: colour and range.
    flash_light: Option<(Vec3, f64)>,
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
            cvars: ViewModelCvars::default(),
            position: DVec3::ZERO,
            rotation: DVec3::ZERO,
            scale: 1.0,
            clock: 0.0,
            slash: WeaponSlash::default(),
            charge: WeaponCharge::default(),
            barrels: Vec::new(),
            charge_outline: None,
            reload_anim: WeaponReloadAnim::default(),
            switch_offsets: SwitchAnimOffsets::default(),
            holster: WeaponHolsterAnim::default(),
            lighting: WeaponLighting::default(),
            submersion_amount: 0.0,
            muzzle_flash_intensity: 0.0,
            flash_light: None,
        };
        view.apply_position();
        view
    }

    /// `applyPosition`: X is the hand's offset, Y and Z the position's,
    /// each unless its `cl_wep_*` override is set.
    pub fn apply_position(&mut self) {
        let pos = self.current_position;
        let c = &self.cvars;
        let over = |value: f64, fallback: f64| if value != 0.0 { value } else { fallback };
        let base = DVec3::new(
            over(c.hip.x, c.handedness.offset()),
            over(c.hip.y, pos.offset.y),
            over(c.hip.z, pos.offset.z),
        );
        self.position = base;
        self.rotation = pos.rotation;
        self.scale = over(c.scale, pos.scale);
        self.effective_base_position = base;
    }

    /// `checkCVarChanges` and the cvars `updateWeaponBob` reads: a new
    /// hand or hip override places the weapon again.
    pub fn apply_cvars(&mut self, cvars: &ViewModelCvars) {
        let placed = (self.cvars.handedness, self.cvars.hip, self.cvars.scale);
        self.cvars = *cvars;
        if placed != (cvars.handedness, cvars.hip, cvars.scale) {
            self.apply_position();
        }
    }

    /// `setMovementHidden`: stow the weapon for a climb, or bring it back.
    pub fn set_movement_hidden(&mut self, hidden: bool) {
        self.holster.set_holstered(hidden);
    }

    /// The mesh draws until the holster has fully stowed it.
    #[must_use]
    pub fn is_drawn(&self) -> bool {
        !self.holster.is_fully_holstered()
    }

    /// `updateLightingFromBsp`: the light grid's sample at the camera.
    pub fn update_lighting(&mut self, delta_time: f64, sample: Option<&LightSample>) {
        self.lighting.update(delta_time, sample);
    }

    /// The one `WeaponViewManager`'s state a weapon put in hand takes from
    /// the one put away: bob, aim, recoil, swing and charge, the holster,
    /// the light, the cvars and the clock. Its barrels and its reload
    /// (cancelled by the switch) stay its own.
    pub fn carry_from(&mut self, old: &WeaponView) {
        self.bob = old.bob.clone();
        self.recoil = old.recoil.clone();
        self.ads_progress = old.ads_progress;
        self.slash = old.slash.clone();
        self.charge = old.charge;
        self.charge_outline = old.charge_outline;
        self.holster = old.holster;
        self.lighting = old.lighting;
        self.submersion_amount = old.submersion_amount;
        self.clock = old.clock;
        self.reload_anim.cancel();
        self.apply_cvars(&old.cvars);
    }

    /// `presentFire`'s motion: the recoil, timed to the fire rate, and a
    /// melee weapon's swing (not for a charged throw).
    pub fn present_fire(&mut self, shot: &ShotStyle, projectile_fire: bool) {
        // The light: lit at the flash's intensity, its colour and range.
        self.muzzle_flash_intensity = shot
            .flash
            .and_then(|f| f.intensity)
            .unwrap_or(MUZZLE_FLASH_MAX_INTENSITY);
        let color = shot.flash.map_or(Vec3::new(1.0, 0.8, 0.3), |f| f.color);
        let range = shot.flash.and_then(|f| f.range).unwrap_or(MUZZLE_FLASH_RANGE);
        self.flash_light = Some((color, range));
        self.recoil.fire(
            shot.recoil_scale,
            self.ads_progress,
            shot.ads_recoil_scale,
            shot.refire_seconds,
        );
        for barrel in &mut self.barrels {
            barrel.fire(shot.refire_seconds);
        }
        if shot.melee && !projectile_fire {
            self.slash.trigger(self.clock * 1000.0);
        }
    }

    /// `startReload`.
    pub fn start_reload(&mut self, duration: f64) {
        self.reload_anim.start(duration, self.clock);
        for barrel in &mut self.barrels {
            barrel.reload(duration);
        }
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
        self.holster.update(dt);
        // `updateMuzzleFlash`: the light fades, the blade's at half the rate.
        if self.muzzle_flash_intensity > 0.0 {
            let decay = MUZZLE_FLASH_DECAY * if self.weapon == "blade" { 0.5 } else { 1.0 };
            self.muzzle_flash_intensity = (self.muzzle_flash_intensity - decay * dt).max(0.0);
        }
        let reloading = input.reloading || self.reload_anim.is_reloading();
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
            &self.cvars.bob,
        );
        let bob = self.bob.state();

        // Aiming down sights.
        let ads_time = self
            .def
            .and_then(|d| d.ads_time)
            .filter(|t| *t != 0.0)
            .unwrap_or(self.cvars.ads_time);
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
        // The `cl_wep_ads_*` overrides, else the definition's sights.
        let ads_offset = self.def.and_then(|d| d.ads_offset);
        let over = self.cvars.ads;
        let pick = |o: f64, def: Option<f64>, fallback: f64| if o != 0.0 { o } else { def.unwrap_or(fallback) };
        let ads_target = DVec3::new(
            pick(over.x, ads_offset.map(|a| a.x), 0.0),
            pick(over.y, ads_offset.map(|a| a.y), base.y + self.cvars.ads_offset_y),
            pick(over.z, ads_offset.map(|a| a.z), base.z),
        );
        let ads_x = base.x + (ads_target.x - base.x) * self.ads_progress;
        let ads_y = (ads_target.y - base.y) * self.ads_progress;
        let ads_z = (ads_target.z - base.z) * self.ads_progress;

        // The blade's charge: aiming a melee weapon holds its charge.
        let charging = input.aiming && self.def.is_some_and(|d| d.melee);
        let charge_time = self
            .def
            .and_then(|d| d.charge_time)
            .filter(|t| *t != 0.0)
            .unwrap_or(1.0);
        let charge = self.charge.update(charging, charge_time, self.clock * 1000.0);
        self.charge_outline = charge.outline;

        let reload = self.reload_anim.offsets(self.clock);
        let switch = self.switch_offsets;
        let slash = self.slash.offsets(self.clock * 1000.0);
        let holster = self.holster.offsets();
        self.position = DVec3::new(
            ads_x + bob.bob_x + reload.x_offset + switch.x_offset + charge.shake.x + slash.x_offset + holster.x_offset,
            base.y
                + bob.bob_y
                + bob.jump_land_bob
                + ads_y
                + reload.y_offset
                + switch.y_offset
                + charge.shake.y
                + slash.y_offset
                + holster.y_offset,
            base.z + ads_z + reload.z_offset - self.recoil.offset + switch.z_offset + slash.z_offset + holster.z_offset,
        );
        // Aimed, the sprint tilt drops by 80%.
        let sprint_factor = 1.0 - self.ads_progress * 0.8;
        let base_rot = self.current_position.rotation;
        let degrees = std::f64::consts::PI / 180.0;
        self.rotation = DVec3::new(
            base_rot.x + bob.sprint_tilt * sprint_factor * degrees + reload.pitch + slash.pitch
                - self.recoil.pitch_offset,
            base_rot.y + bob.sprint_yaw * sprint_factor * degrees + slash.yaw,
            base_rot.z
                + bob.sprint_roll * sprint_factor * degrees
                + reload.roll
                + switch.roll
                + slash.roll
                + holster.roll,
        );
        // `updateMuzzleFlash` steps the recoil and the barrels after the
        // weapon is placed, so the kick shows from the next frame.
        self.recoil.update(dt);
        for barrel in &mut self.barrels {
            barrel.update(dt);
        }
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
        (self.cvars.fov * std::f64::consts::PI / 180.0) as f32
    }

    /// `getMuzzleWorldPosition(distance)`: `distance` units from the eye
    /// along the main camera's screen ray through the muzzle. `socket` is
    /// the model's muzzle tag in glTF scene space; `camera_world` the main
    /// camera's world matrix, `main_fov` its vertical FOV in radians.
    #[must_use]
    pub fn muzzle_world_position(
        &self,
        camera_world: Mat4,
        main_fov: f32,
        socket: Option<Vec3>,
        distance: f64,
    ) -> DVec3 {
        let eye = camera_world.w_axis.truncate().as_dvec3();
        if let Some(socket) = socket {
            let local = (self.root_matrix() * glb_root_matrix(&self.weapon)).transform_point3(socket);
            if let Some(ray) = muzzle_screen_ray(local, self.fov_radians(), main_fov) {
                let direction = camera_world.transform_vector3(ray).as_dvec3().normalize();
                if direction.is_finite() {
                    return eye + direction * distance;
                }
            }
        }
        self.fallback_muzzle(camera_world, distance)
    }

    /// `muzzleFlashLight` this frame: the last shot's colour at the faded
    /// intensity (over `MUZZLE_FLASH_MAX_INTENSITY`), its specular half
    /// the colour; `None` once it is out.
    #[must_use]
    pub fn muzzle_flash_light(&self) -> Option<MuzzleFlashLight> {
        let (color, range) = self.flash_light?;
        (self.muzzle_flash_intensity > 0.0).then(|| MuzzleFlashLight {
            diffuse: color,
            specular: color * 0.5,
            intensity: self.muzzle_flash_intensity / MUZZLE_FLASH_MAX_INTENSITY,
            range,
        })
    }

    /// `getBladeTipWorldPosition(jitterX, jitterY)`'s projection, for any
    /// socket: `distance` units from the eye along the main camera's ray
    /// through the pixel where the weapon camera shows `socket` (glTF scene
    /// space), moved `jitter` pixels across a screen `viewport_height`
    /// pixels tall. `None` for a socket behind the eye.
    #[must_use]
    pub fn socket_world_position(
        &self,
        camera_world: Mat4,
        main_fov: f32,
        socket: Vec3,
        distance: f64,
        jitter: (f64, f64),
        viewport_height: f64,
    ) -> Option<DVec3> {
        let eye = camera_world.w_axis.truncate().as_dvec3();
        let local = (self.root_matrix() * glb_root_matrix(&self.weapon)).transform_point3(socket);
        let ray = muzzle_screen_ray(local, self.fov_radians(), main_fov)?.as_dvec3();
        // A pixel is 2 tan(fov / 2) / height of the screen ray's x/z and
        // y/z, across as up (the aspect cancels); screen y runs down.
        let per_pixel = 2.0 * f64::from(main_fov * 0.5).tan() / viewport_height.max(1.0);
        let ray = DVec3::new(
            ray.x / ray.z + jitter.0 * per_pixel,
            ray.y / ray.z - jitter.1 * per_pixel,
            1.0,
        );
        let direction = camera_world.transform_vector3(ray.as_vec3()).as_dvec3().normalize();
        direction.is_finite().then(|| eye + direction * distance)
    }

    /// `getMuzzleWorldPosition`'s fallback for a model without a muzzle
    /// tag: between the hip (right 1.5, down 1.2, forward 5) and the
    /// sights (down 0.8, forward 4), moved with the bob and the recoil.
    /// Under liquid it floats in a slow figure of eight and sags. Not yet:
    /// the beam charge's steadying.
    fn fallback_muzzle(&self, camera_world: Mat4, distance: f64) -> DVec3 {
        let eye = camera_world.w_axis.truncate().as_dvec3();
        let forward = camera_world.transform_vector3(Vec3::Z).as_dvec3().normalize();
        let right = camera_world.transform_vector3(Vec3::X).as_dvec3().normalize();
        let up = camera_world.transform_vector3(Vec3::Y).as_dvec3().normalize();
        let ads = self.ads_progress;
        let right_off = 1.5 * (1.0 - ads);
        let down_off = 1.2 - 0.4 * ads;
        let fwd_off = 5.0 - ads;
        let scale = 0.05;
        let bob = self.bob.state();
        let bob_x = bob.bob_x * scale;
        let bob_y = (bob.bob_y + bob.jump_land_bob) * scale;
        let recoil_z = -self.recoil.offset * scale;
        let (mut float_right, mut float_up) = (0.0, 0.0);
        let submersion = self.submersion_amount;
        if submersion > 0.0 {
            let t = self.clock;
            let tau = std::f64::consts::PI * 2.0;
            float_right = (t * WEAPON_FLOAT_HZ * tau).sin() * WEAPON_FLOAT_AMPLITUDE * submersion;
            // Twice the rate upright: a lazy figure of eight, not a circle.
            let vertical = (t * WEAPON_FLOAT_HZ * tau * 2.0).sin() * WEAPON_FLOAT_AMPLITUDE;
            float_up = (vertical - WEAPON_FLOAT_SAG) * submersion;
        }
        let origin = eye
            + forward * (fwd_off + recoil_z)
            + right * (right_off + bob_x + float_right)
            + up * (-down_off + bob_y + float_up);
        if distance == 5.0 {
            return origin;
        }
        eye + (origin - eye).normalize() * distance
    }
}

/// The main camera's ray, in camera space, through the pixel where the
/// weapon camera (same view, its own FOV) shows `socket` (a camera-space
/// point): `None` behind the eye.
#[must_use]
pub fn muzzle_screen_ray(socket: Vec3, weapon_fov: f32, main_fov: f32) -> Option<Vec3> {
    if socket.z <= 0.0 {
        return None;
    }
    // Both cameras share the aspect, so it cancels: the screen point is the
    // socket's x/z and y/z over the weapon camera's tangent.
    let ratio = (main_fov * 0.5).tan() / (weapon_fov * 0.5).tan();
    let ray = Vec3::new(socket.x * ratio, socket.y * ratio, socket.z);
    ray.is_finite().then_some(ray)
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
            melee: false,
            charge_time: None,
        }
    }

    #[test]
    fn aiming_a_blade_charges_it() {
        let blade_def = ViewModelDef {
            melee: true,
            charge_time: Some(1.0),
            ..plasmagun()
        };
        let mut blade = WeaponView::new("blade", Some(blade_def));
        let hold = ViewModelInput {
            delta_time: 0.01,
            grounded: true,
            aiming: true,
            ..ViewModelInput::default()
        };
        blade.update(&hold);
        assert!(blade.charge_outline.is_some());
        for _ in 0..120 {
            blade.update(&hold);
        }
        assert_eq!(blade.charge.progress, 1.0);
        let width = blade.charge_outline.unwrap().width;
        assert!((width - 4.8).abs() < 1e-5, "{width}");
        blade.update(&ViewModelInput { aiming: false, ..hold });
        assert!(blade.charge_outline.is_none());
        assert_eq!(blade.charge.progress, 0.0);

        // A gun aims; it does not charge.
        let mut gun = WeaponView::new("plasmagun", Some(plasmagun()));
        gun.update(&hold);
        assert!(gun.charge_outline.is_none());
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
        // The kick is placed a frame after it is stepped, as in the browser.
        gun.update(&still);
        assert_eq!(gun.position, rest);
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
    fn the_muzzle_ray_lands_on_the_same_pixel() {
        // A socket on the weapon camera's axis stays on the main camera's.
        let on_axis = muzzle_screen_ray(Vec3::new(0.0, 0.0, 30.0), 1.0, 1.4).unwrap();
        assert_eq!(on_axis, Vec3::new(0.0, 0.0, 30.0));
        // Off axis: the same screen point through both projections.
        let socket = Vec3::new(6.0, -4.0, 20.0);
        let (weapon_fov, main_fov) = (60f32.to_radians(), 100f32.to_radians());
        let ray = muzzle_screen_ray(socket, weapon_fov, main_fov).unwrap();
        let aspect = 16.0 / 9.0;
        let ndc = |p: Vec3, fov: f32| {
            let t = (fov * 0.5).tan();
            (p.x / (p.z * t * aspect), p.y / (p.z * t))
        };
        let (a, b) = (ndc(socket, weapon_fov), ndc(ray, main_fov));
        assert!((a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5, "{a:?} {b:?}");
        assert_eq!(muzzle_screen_ray(Vec3::new(1.0, 0.0, -1.0), 1.0, 1.0), None);
    }

    #[test]
    fn the_muzzle_sits_out_along_the_view() {
        let view = WeaponView::new("plasmagun", Some(plasmagun()));
        let camera_world = Mat4::from_translation(Vec3::new(100.0, 50.0, -20.0));
        let eye = DVec3::new(100.0, 50.0, -20.0);
        // Without a tag: the hip fallback, right, down and ahead.
        let hip = view.muzzle_world_position(camera_world, 1.5, None, 5.0) - eye;
        assert!((hip - DVec3::new(1.5, -1.2, 5.0)).length() < 1e-6, "{hip}");
        // With one, `distance` from the eye, in front of the camera.
        let tagged = view.muzzle_world_position(camera_world, 1.5, Some(Vec3::new(10.0, 0.0, 0.0)), 5.0) - eye;
        assert!((tagged.length() - 5.0).abs() < 1e-6, "{tagged}");
        assert!(tagged.z > 0.0, "{tagged}");
    }

    #[test]
    fn a_climb_stows_the_gun_and_hides_it_once_down() {
        let mut gun = WeaponView::new("plasmagun", Some(plasmagun()));
        let still = ViewModelInput {
            delta_time: 0.06,
            grounded: true,
            ..ViewModelInput::default()
        };
        gun.update(&still);
        let rest = gun.position;
        gun.set_movement_hidden(true);
        gun.update(&still);
        assert!(gun.is_drawn());
        assert!(gun.position.y < rest.y - 30.0, "{}", gun.position.y);
        for _ in 0..3 {
            gun.update(&still);
        }
        assert!(!gun.is_drawn());
        assert!((gun.position.y - (rest.y - 90.0)).abs() < 1e-9);
        gun.set_movement_hidden(false);
        gun.update(&still);
        assert!(gun.is_drawn());
    }

    #[test]
    fn the_cvars_move_the_hand_and_override_the_hip() {
        let mut gun = WeaponView::new("plasmagun", Some(plasmagun()));
        let mut cvars = ViewModelCvars {
            handedness: Handedness::Left,
            ..ViewModelCvars::default()
        };
        gun.apply_cvars(&cvars);
        assert_eq!(gun.position, DVec3::new(-12.0, -25.0, -1.0));
        cvars.hip = DVec3::new(3.0, 0.0, 7.0);
        cvars.scale = 1.5;
        gun.apply_cvars(&cvars);
        assert_eq!(gun.position, DVec3::new(3.0, -25.0, 7.0));
        assert_eq!(gun.scale, 1.5);
        cvars.fov = 90.0;
        gun.apply_cvars(&cvars);
        assert!((gun.fov_radians() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn a_switch_carries_the_aim_and_the_kick() {
        let aim = ViewModelInput {
            delta_time: 1.0 / 60.0,
            grounded: true,
            aiming: true,
            ..ViewModelInput::default()
        };
        let mut old = WeaponView::new("plasmagun", Some(plasmagun()));
        for _ in 0..60 {
            old.update(&aim);
        }
        old.present_fire(&ShotStyle::default(), false);
        old.update(&aim);
        let mut new = WeaponView::new("riotgun", Some(plasmagun()));
        new.carry_from(&old);
        assert_eq!(new.ads_progress, old.ads_progress);
        assert_eq!(new.recoil, old.recoil);
        assert_eq!(new.clock, old.clock);
    }

    #[test]
    fn a_tagless_muzzle_floats_under_water() {
        let mut view = WeaponView::new("plasmagun", Some(plasmagun()));
        let camera_world = Mat4::IDENTITY;
        view.submersion_amount = 1.0;
        view.clock = 0.0;
        let still = view.muzzle_world_position(camera_world, 1.5, None, 5.0);
        // At time 0 the drift is all sag.
        assert!(
            (still - DVec3::new(1.5, -1.2 - WEAPON_FLOAT_SAG, 5.0)).length() < 1e-9,
            "{still}"
        );
    }

    #[test]
    fn a_socket_without_jitter_is_the_muzzle_and_a_jitter_moves_it_across() {
        let view = WeaponView::new("plasmagun", Some(plasmagun()));
        let camera_world = Mat4::from_translation(Vec3::new(100.0, 50.0, -20.0));
        let socket = Vec3::new(10.0, 0.0, 0.0);
        let muzzle = view.muzzle_world_position(camera_world, 1.5, Some(socket), 5.0);
        let still = view
            .socket_world_position(camera_world, 1.5, socket, 5.0, (0.0, 0.0), 720.0)
            .unwrap();
        assert!((still - muzzle).length() < 1e-5, "{still} {muzzle}");
        // Four pixels right and down: the ray turns right (+X) and down (-Y)
        // by four pixels' worth of the screen's tangent.
        let eye = DVec3::new(100.0, 50.0, -20.0);
        let moved = view
            .socket_world_position(camera_world, 1.5, socket, 5.0, (4.0, 4.0), 720.0)
            .unwrap();
        let (a, b) = (still - eye, moved - eye);
        let per_pixel = 2.0 * 0.75f64.tan() / 720.0;
        assert!((b.x / b.z - a.x / a.z - 4.0 * per_pixel).abs() < 1e-5, "{a} {b}");
        assert!((b.y / b.z - a.y / a.z + 4.0 * per_pixel).abs() < 1e-5, "{a} {b}");
        assert!((b.length() - 5.0).abs() < 1e-5);
    }

    #[test]
    fn the_flash_light_fades_from_the_shot() {
        let mut view = WeaponView::new("machinegun", Some(plasmagun()));
        assert_eq!(view.muzzle_flash_light(), None);
        let shot = ShotStyle {
            flash: Some(FlashLight {
                color: Vec3::new(1.0, 0.9, 0.3),
                intensity: Some(18.0),
                range: Some(30.0),
            }),
            ..ShotStyle::default()
        };
        view.present_fire(&shot, false);
        let lit = view.muzzle_flash_light().unwrap();
        assert!((lit.intensity - 1.2).abs() < 1e-12);
        assert_eq!(lit.range, 30.0);
        assert_eq!(lit.specular, Vec3::new(0.5, 0.45, 0.15));
        // 150 a second: out after 0.12 s.
        view.update(&ViewModelInput {
            delta_time: 0.06,
            ..ViewModelInput::default()
        });
        assert!((view.muzzle_flash_light().unwrap().intensity - 0.6).abs() < 1e-9);
        view.update(&ViewModelInput {
            delta_time: 0.07,
            ..ViewModelInput::default()
        });
        assert_eq!(view.muzzle_flash_light(), None);
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
