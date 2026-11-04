//! SHAG `src/engine/weapon/ExplosionEffect.ts`: the explosion presets, and
//! which projectile takes which (`ProjectileManager.spawnImpact`).
//! and the camera shake one sets off near the player (`triggerCameraShake`).

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

/// `triggerCameraShake`: how hard an explosion `distance` units from the
/// player shakes the view. Full within 42% of the look's range, falling off
/// with the square of the rest, scaled by its size (capped at 1.5 times a
/// scale of 10) and its own multiplier; `None` when out of reach or too
/// weak to feel.
#[must_use]
pub fn explosion_shake(look: &ExplosionLook, distance: f64) -> Option<f64> {
    let max_range = look.shake_max_range;
    let full_range = max_range * 0.42;
    if distance >= max_range {
        return None;
    }
    let intensity = if distance <= full_range {
        1.0
    } else {
        let t = (distance - full_range) / (max_range - full_range);
        (1.0 - t) * (1.0 - t)
    };
    let size = (look.scale / 10.0).min(1.5);
    let shake = intensity * size * look.shake_intensity;
    (shake >= 0.01).then_some(shake)
}

/// `ROCKET_EXPLOSION`.
pub const ROCKET_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 13.0,
    speed: 0.85,
    core_color: Vec3::new(1.0, 0.95, 0.6),
    outer_color: Vec3::new(1.0, 0.4, 0.05),
    smoke_color: Vec3::new(0.3, 0.25, 0.2),
    rays: DEFAULT_RAYS,
    shake_intensity: 0.7,
    shake_max_range: 194.0,
};

/// `GRENADE_EXPLOSION`.
pub const GRENADE_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 9.0,
    speed: 0.95,
    core_color: Vec3::new(1.0, 0.9, 0.5),
    outer_color: Vec3::new(1.0, 0.5, 0.1),
    smoke_color: Vec3::new(0.3, 0.25, 0.2),
    rays: DEFAULT_RAYS,
    shake_intensity: 0.7,
    shake_max_range: 194.0,
};

/// `PLASMA_EXPLOSION`: small, fast, green.
pub const PLASMA_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 3.0,
    speed: 1.5,
    core_color: Vec3::new(0.8, 1.0, 0.6),
    outer_color: Vec3::new(0.1, 0.9, 0.2),
    smoke_color: Vec3::new(0.1, 0.2, 0.1),
    rays: (4, 6),
    shake_intensity: 0.0525,
    shake_max_range: 216.0,
};

/// `BLADE_EXPLOSION`: half a rocket's, teal.
pub const BLADE_EXPLOSION: ExplosionLook = ExplosionLook {
    scale: 6.5,
    speed: 1.1,
    core_color: Vec3::new(0.7, 1.0, 0.95),
    outer_color: Vec3::new(0.05, 1.0, 0.9),
    smoke_color: Vec3::new(0.08, 0.2, 0.18),
    rays: DEFAULT_RAYS,
    shake_intensity: 0.35,
    shake_max_range: 162.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_rocket_shakes_hardest_and_a_far_one_not_at_all() {
        // Full inside 42% of 194, a 1.3 size factor and its 0.7.
        let close = explosion_shake(&ROCKET_EXPLOSION, 50.0).unwrap();
        assert!((close - 1.3 * 0.7).abs() < 1e-12, "{close}");
        let edge = explosion_shake(&ROCKET_EXPLOSION, 150.0).unwrap();
        assert!(edge < close && edge > 0.0);
        assert_eq!(explosion_shake(&ROCKET_EXPLOSION, 194.0), None);
        // A plasma pop is barely felt up close and not at all further out.
        let pop = explosion_shake(&PLASMA_EXPLOSION, 0.0).unwrap();
        assert!((pop - 0.3 * 0.0525).abs() < 1e-12, "{pop}");
        assert_eq!(explosion_shake(&PLASMA_EXPLOSION, 150.0), None);
    }
}
