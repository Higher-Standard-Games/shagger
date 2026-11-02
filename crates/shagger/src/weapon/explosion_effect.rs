//! SHAG `src/engine/weapon/ExplosionEffect.ts`: the explosion presets, and
//! which projectile takes which (`ProjectileManager.spawnImpact`).
//!
//! Not yet: the camera shake.

use glam::Vec3;

use super::explosion::explosion_batch_material::EXPLOSION_FIRE_TIME;
use super::explosion::explosion_batch_renderer::{EXPLOSION_LIFETIME, ExplosionLook};

/// The flash light's intensity per unit of scale.
const FLASH_LIGHT_GAIN: f64 = 0.1;

/// The flash light `seconds` after an explosion went off, as `spawn`
/// acquires it and `update` fades it: intensity and range, or `None` once
/// the explosion is over (its light released).
#[must_use]
pub fn explosion_flash(look: &ExplosionLook, seconds: f64) -> Option<(f32, f32)> {
    let age = seconds * look.speed;
    if age >= EXPLOSION_LIFETIME {
        return None;
    }
    // The flash fades with the fire, over the first 0.6 of explosion time.
    let t = (age / EXPLOSION_FIRE_TIME).min(1.0);
    let intensity = FLASH_LIGHT_GAIN * look.scale * (1.0 - t * t);
    let range = 40.0 * look.scale * (1.0 - t * 0.5);
    Some((intensity as f32, range as f32))
}

const DEFAULT_RAYS: (u32, u32) = (8, 12);

/// `ROCKET_EXPLOSION`.
pub const ROCKET_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 13.0,
    speed: 0.85,
    core_color: Vec3::new(1.0, 0.95, 0.6),
    outer_color: Vec3::new(1.0, 0.4, 0.05),
    smoke_color: Vec3::new(0.3, 0.25, 0.2),
    rays: DEFAULT_RAYS,
};

/// `GRENADE_EXPLOSION`.
pub const GRENADE_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 9.0,
    speed: 0.95,
    core_color: Vec3::new(1.0, 0.9, 0.5),
    outer_color: Vec3::new(1.0, 0.5, 0.1),
    smoke_color: Vec3::new(0.3, 0.25, 0.2),
    rays: DEFAULT_RAYS,
};

/// `PLASMA_EXPLOSION`: small, fast, green.
pub const PLASMA_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 3.0,
    speed: 1.5,
    core_color: Vec3::new(0.8, 1.0, 0.6),
    outer_color: Vec3::new(0.1, 0.9, 0.2),
    smoke_color: Vec3::new(0.1, 0.2, 0.1),
    rays: (4, 6),
};

/// `BLADE_EXPLOSION`: half a rocket's, teal.
pub const BLADE_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 6.5,
    speed: 1.1,
    core_color: Vec3::new(0.7, 1.0, 0.95),
    outer_color: Vec3::new(0.05, 1.0, 0.9),
    smoke_color: Vec3::new(0.08, 0.2, 0.18),
    rays: DEFAULT_RAYS,
};

/// `spawnImpact`'s explosion for a projectile, if it explodes.
#[must_use]
pub fn explosion_for(projectile: &str) -> Option<&'static ExplosionLook> {
    match projectile {
        "rocket" => Some(&ROCKET_EXPLOSION),
        "grenade" => Some(&GRENADE_EXPLOSION),
        "plasma_bolt" => Some(&PLASMA_EXPLOSION),
        "blade_blast" => Some(&BLADE_EXPLOSION),
        _ => None,
    }
}
