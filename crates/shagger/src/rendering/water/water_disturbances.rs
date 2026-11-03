//! Port of SHAG `src/engine/rendering/water/WaterDisturbances.ts`: cosmetic
//! impulses, shared by the surface renderer and game interactions. The
//! TypeScript keeps one per scene (`waterDisturbancesFor`); here the map's
//! [`super::liquid_surface_system::LiquidSurfaceSystem`] owns it and
//! resolves each impulse to a pool's surface.

use glam::DVec3;
use shagger_shared::js::js_hypot;

pub use super::unified_water_material::MAX_WATER_IMPULSES;

/// `WATER_IMPULSE_LIFETIME`, seconds.
pub const WATER_IMPULSE_LIFETIME: f64 = 2.0;
/// `WATER_IMPULSE_RADIUS`.
pub const WATER_IMPULSE_RADIUS: f64 = 96.0;

/// `WaterImpulse`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterImpulse {
    pub position: DVec3,
    pub strength: f64,
    pub age: f64,
    pub radius: f64,
    pub direction_x: f64,
    pub direction_z: f64,
}

/// `WaterDisturbances`.
#[derive(Debug, Clone, Default)]
pub struct WaterDisturbances {
    pub impulses: Vec<WaterImpulse>,
    /// Monotonic diagnostics: an expired impulse must not erase evidence of
    /// delivery.
    pub accepted: u64,
    pub rejected: u64,
    pub last_result: String,
}

impl WaterDisturbances {
    /// `add`: `resolve_surface` snaps the impulse to a supported surface
    /// (`resolveSurface`), or `None` when nothing resolves (no water system).
    pub fn add(
        &mut self,
        position: DVec3,
        strength: f64,
        radius: f64,
        direction: Option<(f64, f64)>,
        resolve_surface: Option<&dyn Fn(DVec3) -> Option<DVec3>>,
    ) -> bool {
        let (dx, dz) = direction.unwrap_or((0.0, 0.0));
        if ![position.x, position.y, position.z, strength, radius, dx, dz]
            .iter()
            .all(|v| v.is_finite())
            || strength <= 0.0
            || radius <= 0.0
        {
            self.rejected += 1;
            "Rejected: invalid ripple parameters".clone_into(&mut self.last_result);
            return false;
        }
        let mut position = position;
        if let Some(resolve) = resolve_surface {
            let Some(surface) = resolve(position) else {
                self.rejected += 1;
                "Rejected: no supported water surface near this position".clone_into(&mut self.last_result);
                return false;
            };
            position = surface;
        }
        if self.impulses.len() == MAX_WATER_IMPULSES {
            self.impulses.remove(0);
        }
        let length = js_hypot(&[dx, dz]);
        self.impulses.push(WaterImpulse {
            position,
            strength: strength.min(1.0),
            radius: radius.min(128.0),
            age: 0.0,
            direction_x: if length > 0.001 { dx / length } else { 0.0 },
            direction_z: if length > 0.001 { dz / length } else { 0.0 },
        });
        self.accepted += 1;
        self.last_result = format!("Accepted at ({:.1}, {:.1}, {:.1})", position.x, position.y, position.z);
        true
    }

    /// `update`: ages the impulses and drops the expired.
    pub fn update(&mut self, dt: f64) {
        for impulse in &mut self.impulses {
            impulse.age += dt.max(0.0);
        }
        self.impulses.retain(|i| i.age < WATER_IMPULSE_LIFETIME);
    }

    /// `pack`: two vec4s per impulse the surface `contains` (position and
    /// age; strength, radius and direction XZ, zero for a splash).
    #[must_use]
    pub fn pack(&self, contains: impl Fn(DVec3) -> bool) -> [[f32; 4]; MAX_WATER_IMPULSES * 2] {
        let mut uniforms = [[0.0f32; 4]; MAX_WATER_IMPULSES * 2];
        let mut i = 0;
        for impulse in &self.impulses {
            if !contains(impulse.position) {
                continue;
            }
            let p = impulse.position;
            uniforms[i * 2] = [p.x as f32, p.y as f32, p.z as f32, impulse.age as f32];
            uniforms[i * 2 + 1] = [
                impulse.strength as f32,
                impulse.radius as f32,
                impulse.direction_x as f32,
                impulse.direction_z as f32,
            ];
            i += 1;
        }
        uniforms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_ages_and_rejects() {
        let mut waves = WaterDisturbances::default();
        assert!(!waves.add(DVec3::ZERO, 0.0, 96.0, None, None));
        assert!(!waves.add(DVec3::new(f64::NAN, 0.0, 0.0), 1.0, 96.0, None, None));
        for i in 0..14 {
            assert!(waves.add(DVec3::new(f64::from(i), 0.0, 0.0), 2.0, 400.0, Some((3.0, 4.0)), None));
        }
        assert_eq!(waves.impulses.len(), MAX_WATER_IMPULSES);
        assert_eq!(waves.impulses[0].position.x, 2.0);
        assert_eq!((waves.impulses[0].strength, waves.impulses[0].radius), (1.0, 128.0));
        assert_eq!(
            (waves.impulses[0].direction_x, waves.impulses[0].direction_z),
            (0.6, 0.8)
        );
        assert_eq!((waves.accepted, waves.rejected), (14, 2));
        let packed = waves.pack(|p| p.x < 5.0);
        assert_eq!(packed[0], [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(packed[5], [1.0, 128.0, 0.6, 0.8]);
        assert_eq!(packed[6], [0.0; 4]);
        let none: &dyn Fn(DVec3) -> Option<DVec3> = &|_| None;
        assert!(!waves.add(DVec3::ZERO, 1.0, 96.0, None, Some(none)));
        let snap: &dyn Fn(DVec3) -> Option<DVec3> = &|p| Some(DVec3::new(p.x, -10.0, p.z));
        assert!(waves.add(DVec3::new(1.0, 5.0, 2.0), 1.0, 96.0, None, Some(snap)));
        assert_eq!(waves.impulses.last().unwrap().position.y, -10.0);
        waves.update(1.5);
        waves.update(0.6);
        assert!(waves.impulses.is_empty());
    }
}
