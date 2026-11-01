//! SHAG `src/engine/weapon/ExplosionEffect.ts`: the explosion presets, and
//! which projectile takes which (`ProjectileManager.spawnImpact`).
//!
//! Not yet: the flash light (with dynamic lights) and the camera shake.

use glam::Vec3;

use super::explosion::explosion_batch_renderer::ExplosionLook;

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
