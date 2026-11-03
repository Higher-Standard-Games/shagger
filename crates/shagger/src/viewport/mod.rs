//! SHAG `src/engine/viewport`: first-person view effects. Each effect
//! reads the player's state and returns a small camera delta (an offset
//! in camera space, a rotation in degrees, a field-of-view multiplier);
//! [`first_person_effects::FirstPersonEffectStack`] sums them and
//! [`apply_fps`] puts the sum on the camera.
//!
//! The browser reads each effect's cvars from the global `Cvar`; here a
//! frame's values are read once into [`ViewCvars`] and handed to every
//! effect.

pub mod easing;
pub mod effects;
pub mod first_person_effects;
pub mod player_state;
pub mod speed_constants;
pub mod viewport_delta;

use crate::core::cvar::{CvarOptions, CvarType, Cvars};
use glam::DVec3;

pub use first_person_effects::FirstPersonEffectStack;
pub use player_state::{PlayerState, WallJumpState};
pub use viewport_delta::ViewportDelta;

/// The effects' cvars, read once a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewCvars {
    pub view_effects: bool,
    pub bob: bool,
    pub bob_amplitude: f64,
    pub bob_frequency: f64,
    pub bob_roll: f64,
    pub bob_lean: f64,
    pub landing: bool,
    pub landing_amplitude: f64,
    pub landing_speed: f64,
    pub strafe_tilt: f64,
    pub strafe_tilt_speed: f64,
    pub speed_fov: f64,
    pub speed_fov_speed: f64,
    pub idle: bool,
    pub idle_amplitude: f64,
    pub idle_frequency: f64,
    /// `cl_ads_time` and `cl_ads_fov_reduction` (MovementVars).
    pub ads_time: f64,
    pub ads_fov_reduction: f64,
}

impl Default for ViewCvars {
    /// The registered defaults.
    fn default() -> Self {
        Self {
            view_effects: true,
            bob: true,
            bob_amplitude: 1.0,
            bob_frequency: 0.03,
            bob_roll: 0.35,
            bob_lean: 1.0,
            landing: true,
            landing_amplitude: 8.0,
            landing_speed: 4.0,
            strafe_tilt: 1.5,
            strafe_tilt_speed: 8.0,
            speed_fov: 0.38,
            speed_fov_speed: 5.0,
            idle: true,
            idle_amplitude: 0.02,
            idle_frequency: 0.6,
            ads_time: 0.08,
            ads_fov_reduction: 15.0,
        }
    }
}

/// (name, default, min, max, int, description), as each effect registers.
const CVARS: [(&str, f64, f64, f64, bool, &str); 16] = [
    (
        "cl_vieweffects",
        1.0,
        0.0,
        1.0,
        true,
        "Master toggle for all viewport effects",
    ),
    ("cg_bob", 1.0, 0.0, 1.0, true, "Enable view bobbing while walking"),
    ("cg_bobamplitude", 1.0, 0.0, 5.0, false, "Vertical bob strength (units)"),
    (
        "cg_bobfrequency",
        0.03,
        0.01,
        0.06,
        false,
        "Bob cycle speed - higher = faster footsteps",
    ),
    ("cg_bobroll", 0.35, 0.0, 2.0, false, "Roll tilt during bob (degrees)"),
    (
        "cg_boblean",
        1.0,
        0.0,
        5.0,
        false,
        "Forward lean when running fast (degrees)",
    ),
    ("cg_landing", 1.0, 0.0, 1.0, true, "Enable landing impact effect"),
    ("cg_landingamplitude", 8.0, 0.0, 20.0, false, "Max landing dip (units)"),
    ("cg_landingspeed", 4.0, 1.0, 10.0, false, "Landing animation speed"),
    ("cg_strafetilt", 1.5, 0.0, 5.0, false, "Max strafe lean (degrees)"),
    ("cg_strafetiltspeed", 8.0, 1.0, 20.0, false, "Lean interpolation speed"),
    (
        "cl_speedfov",
        0.38,
        0.0,
        0.5,
        false,
        "Max FOV increase at speed (multiplier)",
    ),
    ("cl_speedfovspeed", 5.0, 1.0, 15.0, false, "FOV interpolation speed"),
    (
        "cg_idle",
        1.0,
        0.0,
        1.0,
        true,
        "Enable idle breathing effect (0=off for competitive, 1=on for immersion)",
    ),
    (
        "cg_idleamplitude",
        0.02,
        0.0,
        0.5,
        false,
        "Idle breathing amplitude (units) - should be barely perceptible",
    ),
    (
        "cg_idlefrequency",
        0.6,
        0.3,
        2.0,
        false,
        "Idle breathing cycle speed (slower = more natural)",
    ),
];

/// Registers the effects' cvars (each effect's `registerCvars`).
pub fn register_view_cvars(cvars: &mut Cvars) {
    for (name, default, min, max, int, description) in CVARS {
        if cvars.exists(name) {
            continue;
        }
        let mut options = CvarOptions::describe(description).range(min, max);
        if int {
            options = options.kind(CvarType::Int);
        }
        cvars.register(name, default, options);
    }
}

impl ViewCvars {
    /// This frame's values; unregistered ones keep their defaults.
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
        let on = |name: &str, fallback: bool| {
            if cvars.exists(name) {
                cvars.get_number(name) != 0.0
            } else {
                fallback
            }
        };
        Self {
            view_effects: on("cl_vieweffects", d.view_effects),
            bob: on("cg_bob", d.bob),
            bob_amplitude: num("cg_bobamplitude", d.bob_amplitude),
            bob_frequency: num("cg_bobfrequency", d.bob_frequency),
            bob_roll: num("cg_bobroll", d.bob_roll),
            bob_lean: num("cg_boblean", d.bob_lean),
            landing: on("cg_landing", d.landing),
            landing_amplitude: num("cg_landingamplitude", d.landing_amplitude),
            landing_speed: num("cg_landingspeed", d.landing_speed),
            strafe_tilt: num("cg_strafetilt", d.strafe_tilt),
            strafe_tilt_speed: num("cg_strafetiltspeed", d.strafe_tilt_speed),
            speed_fov: num("cl_speedfov", d.speed_fov),
            speed_fov_speed: num("cl_speedfovspeed", d.speed_fov_speed),
            idle: on("cg_idle", d.idle),
            idle_amplitude: num("cg_idleamplitude", d.idle_amplitude),
            idle_frequency: num("cg_idlefrequency", d.idle_frequency),
            ads_time: num("cl_ads_time", d.ads_time),
            ads_fov_reduction: num("cl_ads_fov_reduction", d.ads_fov_reduction),
        }
    }
}

/// `FPSApplicator.apply`: the offset along the camera's right, the world's
/// up and the camera's forward; the rotation (degrees) onto pitch, yaw and
/// roll; the field of view scaled. Returns (position offset, [pitch, yaw,
/// roll] in radians, field-of-view multiplier).
#[must_use]
pub fn apply_fps(delta: &ViewportDelta, right: DVec3, forward: DVec3) -> (DVec3, [f64; 3], f64) {
    let offset =
        right * delta.position_offset.x + DVec3::Y * delta.position_offset.y + forward * delta.position_offset.z;
    let r = delta.rotation_offset * (std::f64::consts::PI / 180.0);
    (offset, [r.x, r.y, r.z], delta.fov_multiplier)
}
