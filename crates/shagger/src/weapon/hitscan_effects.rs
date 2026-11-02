//! SHAG `src/engine/weapon/HitscanEffects.ts`: the hitscan weapons' effects,
//! one owner for the bullet streaks, pellets, rails, Voltaic beams, muzzle
//! flashes and impact sparks, aged by one clock.
//!
//! TypeScript reaches the scene, the collision manager and the light pool
//! through singletons; here the caller passes the world to trace against
//! and the light pool. Not yet: wall marks, the rendered-mesh pick for a
//! shot that starts inside a brush, `fireBolt`, `fireSlash`, and the local
//! Voltaic hero beam's truth bookkeeping. A traveling pellet, a Babylon cone
//! mesh in TypeScript, is drawn as a short streak that crosses its ray in
//! the same time.

use glam::{DVec3, Vec3};
use shagger_shared::spread_utils::{generate_spread_directions, mulberry32, yaw_pitch_from_direction};

use crate::babylon::maths::math_vector::Vector3;
use crate::physics::collision_backend::TraceOptions;
use crate::physics::content_flags::{content, surface};
use crate::physics::sweep_collision_backend::SweepCollisionBackend;
use crate::rendering::gpu::Gpu;
use crate::weapon::impact::impact_spark_renderer::{ImpactSparkBurst, ImpactSparkRenderer};
use crate::weapon::muzzle::muzzle_flash_renderer::{MuzzleFlashRenderer, MuzzleFlashSpawn};
use crate::weapon::point_light_pool::{PointLightPool, PooledLight};
use crate::weapon::rail::rail_effect_renderer::RailEffectRenderer;
use crate::weapon::rail::rail_profiles::{RailEffectSample, RailImpactKind};
use crate::weapon::tracer::streak_tracer_renderer::{StreakTracerRenderer, StreakTracerShot};
use crate::weapon::voltaic::voltaic_effect_renderer::VoltaicEffectRenderer;

const TRACER_IMPACT_LIGHT_INTENSITY: f32 = 0.25;
/// Embers per contact, per weapon class.
const TRACER_SPARKS: f64 = 6.0;
/// Per pellet: a full riot gun blast is this times the pellet count.
const PELLET_SPARKS: f64 = 3.0;
const RAIL_SPARKS: f64 = 16.0;

/// Where a shot stopped: `marks` is false on surfaces that take no marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotTrace {
    pub point: DVec3,
    pub normal: DVec3,
    pub marks: bool,
}

/// What a shot's ray stops on.
pub trait ShotWorld {
    /// `traceRay`: where a shot from `origin` along `dir` stops within
    /// `max_dist`, or `None` when it flies on.
    fn trace_shot(&mut self, origin: DVec3, dir: DVec3, max_dist: f64) -> Option<ShotTrace>;
}

/// No world: every shot flies its full range.
impl ShotWorld for () {
    fn trace_shot(&mut self, _: DVec3, _: DVec3, _: f64) -> Option<ShotTrace> {
        None
    }
}

impl ShotWorld for SweepCollisionBackend {
    /// A ray against solid brushes: what the server's shots stop on.
    fn trace_shot(&mut self, origin: DVec3, dir: DVec3, max_dist: f64) -> Option<ShotTrace> {
        let end = origin + dir * max_dist;
        let result = self.trace(
            Vector3::new(origin.x, origin.y, origin.z),
            Vector3::new(end.x, end.y, end.z),
            TraceOptions {
                radius: Some(0.0),
                height: Some(0.0),
                content_mask: Some(content::SOLID),
            },
        );
        // A start inside a brush falls to the rendered-mesh pick in
        // TypeScript, which is not ported: the shot flies on.
        if result.start_solid {
            return None;
        }
        let flags = result.hit_surface_flags;
        if result.fraction >= 1.0 || flags & surface::NOIMPACT != 0 {
            return None;
        }
        let p = result.end_pos;
        Some(ShotTrace {
            point: DVec3::new(p.x, p.y, p.z),
            normal: result.normal.map_or(-dir, |n| DVec3::new(n.x, n.y, n.z)),
            marks: flags & surface::NOMARKS == 0,
        })
    }
}

/// `TracerConfig`: a bullet streak. `origin` and `direction` are the shot's
/// own ray, the one the server judged; `start` is where it is drawn from.
#[derive(Debug, Clone, Copy)]
pub struct TracerConfig {
    pub origin: DVec3,
    pub direction: DVec3,
    /// The muzzle (`origin` by default).
    pub start: Option<DVec3>,
    pub color: Option<Vec3>,
    /// 8192 by default.
    pub max_range: Option<f64>,
    pub intensity: Option<f64>,
}

/// `PelletConfig`: a shotgun blast.
#[derive(Debug, Clone)]
pub struct PelletConfig {
    pub origin: DVec3,
    pub direction: DVec3,
    pub start: Option<DVec3>,
    pub color: Option<Vec3>,
    pub max_range: Option<f64>,
    /// Pellets drawn from the cone when `directions` is absent (20).
    pub pellet_count: Option<usize>,
    /// Cone half-angle in degrees (12).
    pub spread_angle: Option<f64>,
    /// The server's own pellets.
    pub directions: Option<Vec<DVec3>>,
    /// Seeds the cone when `directions` is absent; otherwise random.
    pub seed: Option<u32>,
    /// Seconds a traveling pellet takes (0.25).
    pub duration: Option<f64>,
    /// Pellets drawn traveling (5).
    pub visible_tracers: Option<usize>,
}

/// `serverPelletDirections`: the server's pellets of a shot, rebuilt from
/// its direction and seed.
#[must_use]
pub fn server_pellet_directions(direction: DVec3, seed: i32, count: usize, spread_deg: f64) -> Vec<DVec3> {
    let (yaw, pitch) = yaw_pitch_from_direction(direction.to_array());
    generate_spread_directions(seed, count, spread_deg, yaw, pitch)
        .into_iter()
        .map(DVec3::from_array)
        .collect()
}

/// A light-holding effect (`ActiveEffect` with a light and no meshes).
struct ActiveFlash {
    light: PooledLight,
    intensity: f32,
    range: f32,
    spawn_time: f64,
    duration: f64,
}

/// A tracer's impact, waiting for its streak to arrive.
struct PendingImpact {
    due: f64,
    trace: ShotTrace,
    direction: DVec3,
    color: Vec3,
}

pub struct HitscanEffects {
    pub voltaic: VoltaicEffectRenderer,
    pub rails: RailEffectRenderer,
    pub muzzle_flashes: MuzzleFlashRenderer,
    pub impact_sparks: ImpactSparkRenderer,
    pub streaks: StreakTracerRenderer,
    effects: Vec<ActiveFlash>,
    pending_impacts: Vec<PendingImpact>,
    /// Random pellets when a blast has no seed (`Math.random`).
    random_seed: u32,
}

impl HitscanEffects {
    /// `init`: the renderers, for a scene drawn into `color_format`.
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        Self {
            voltaic: VoltaicEffectRenderer::new(gpu, color_format),
            rails: RailEffectRenderer::new(gpu, color_format),
            muzzle_flashes: MuzzleFlashRenderer::new(gpu, color_format),
            impact_sparks: ImpactSparkRenderer::new(gpu, color_format),
            streaks: StreakTracerRenderer::new(gpu, color_format),
            effects: Vec::new(),
            pending_impacts: Vec::new(),
            random_seed: 0x5EED,
        }
    }

    /// `fireTracer` at `now`: a streak from the muzzle to where the shot's
    /// ray hits; the impact flash and sparks play when the streak gets
    /// there. Returns the hit.
    pub fn fire_tracer(
        &mut self,
        now: f64,
        world: &mut dyn ShotWorld,
        lights: &mut PointLightPool,
        cfg: &TracerConfig,
    ) -> Option<DVec3> {
        let range = cfg.max_range.unwrap_or(8192.0);
        let color = cfg.color.unwrap_or(Vec3::new(1.0, 0.85, 0.3));
        let hit = world.trace_shot(cfg.origin, cfg.direction, range);
        let endpoint = hit.map_or(cfg.origin + cfg.direction * range, |h| h.point);
        let travel = self.streaks.fire(
            now,
            &StreakTracerShot {
                start: cfg.start.unwrap_or(cfg.origin),
                end: endpoint,
                color,
                intensity: cfg.intensity,
            },
        );
        if let Some(trace) = hit {
            let impact = PendingImpact {
                due: now + travel,
                trace,
                direction: cfg.direction,
                color,
            };
            // `afterDelay`: at once when the streak takes no time.
            if travel <= 0.0 {
                self.play_tracer_impact(now, lights, &impact);
            } else {
                self.pending_impacts.push(impact);
            }
        }
        hit.map(|h| h.point)
    }

    fn play_tracer_impact(&mut self, now: f64, lights: &mut PointLightPool, impact: &PendingImpact) {
        let ShotTrace { point, normal, .. } = impact.trace;
        self.spawn_impact_flash(
            now,
            lights,
            point,
            impact.color,
            Some(normal),
            TRACER_IMPACT_LIGHT_INTENSITY,
            60.0,
            0.16,
        );
        self.spawn_impact_sparks(now, point, normal, impact.direction, impact.color, TRACER_SPARKS, false);
    }

    /// `firePellets` at `now`: every pellet traced and sparking, a few drawn
    /// traveling, and one light at the hits' centroid. Returns the hits.
    pub fn fire_pellets(
        &mut self,
        now: f64,
        world: &mut dyn ShotWorld,
        lights: &mut PointLightPool,
        cfg: &PelletConfig,
    ) -> Vec<DVec3> {
        let count = cfg.directions.as_ref().map_or(cfg.pellet_count.unwrap_or(20), Vec::len);
        let spread_rad = cfg.spread_angle.unwrap_or(12.0) * std::f64::consts::PI / 180.0;
        let duration = cfg.duration.unwrap_or(0.25);
        let color = cfg.color.unwrap_or(Vec3::new(1.0, 0.9, 0.4));
        let range = cfg.max_range.unwrap_or(8192.0);
        let visible = cfg.visible_tracers.unwrap_or(5);
        let seed = cfg.seed.unwrap_or_else(|| {
            self.random_seed = self.random_seed.wrapping_mul(0x9E37_79B9).wrapping_add(1);
            self.random_seed
        });
        let mut rng = mulberry32(seed);
        let stride = (count / visible.max(1)).max(1);
        let start = cfg.start.unwrap_or(cfg.origin);
        let mut hits = Vec::new();
        for i in 0..count {
            let dir = match &cfg.directions {
                Some(d) => d[i],
                None => spread_direction_seeded(cfg.direction, spread_rad, &mut rng),
            };
            let hit = world.trace_shot(cfg.origin, dir, range);
            let endpoint = hit.map_or(cfg.origin + dir * range, |h| h.point);
            if let Some(h) = hit {
                hits.push(h.point);
                self.spawn_impact_sparks(now, h.point, h.normal, dir, color, PELLET_SPARKS, false);
            }
            if i % stride == 0 && i / stride < visible {
                self.spawn_traveling_pellet(now, start, endpoint, color, duration);
            }
        }
        if !hits.is_empty() {
            let centroid = hits.iter().sum::<DVec3>() / hits.len() as f64;
            // Boosted intensity and range for the spread.
            self.spawn_impact_flash(now, lights, centroid, color, None, 40.0, 80.0, duration * 2.0);
        }
        hits
    }

    /// `spawnTravelingPellet`: a segment 15% of the ray long that crosses it
    /// in `duration`.
    fn spawn_traveling_pellet(&mut self, now: f64, origin: DVec3, endpoint: DVec3, color: Vec3, duration: f64) {
        let length = (endpoint - origin).length();
        self.streaks.fire_segment(
            now,
            &StreakTracerShot {
                start: origin,
                end: endpoint,
                color,
                intensity: None,
            },
            length / duration.max(1e-3),
            length * 0.15,
            1.25,
        );
    }

    /// `fireRail`: one Electrobolt rail, with sparks on a world contact.
    pub fn fire_rail(&mut self, now: f64, sample: &RailEffectSample) {
        self.rails.fire_rail(now, sample, None);
        if sample.impact_kind == RailImpactKind::World
            && let Some(normal) = sample.impact_normal
        {
            self.spawn_impact_sparks(
                now,
                sample.end,
                normal,
                sample.end - sample.start,
                sample.color,
                RAIL_SPARKS,
                true,
            );
        }
    }

    /// `updateRemoteLightningBeam`: a remote player's Voltaic beam to the
    /// contact the server traced.
    pub fn update_remote_lightning_beam(
        &mut self,
        now: f64,
        entity_id: &str,
        sample: &RailEffectSample,
        direction: DVec3,
    ) {
        self.voltaic.update_remote(now, entity_id, sample, direction);
    }

    /// `updateRemoteLightningFallback`: a remote Voltaic beam traced here,
    /// for demos and packets without the server's contact.
    #[allow(clippy::too_many_arguments)]
    pub fn update_remote_lightning_fallback(
        &mut self,
        now: f64,
        world: &mut dyn ShotWorld,
        entity_id: &str,
        origin: DVec3,
        direction: DVec3,
        color: Option<Vec3>,
        max_range: Option<f64>,
    ) {
        let range = max_range.unwrap_or(850.0);
        let hit = world.trace_shot(origin, direction, range);
        let sample = RailEffectSample {
            start: origin,
            end: hit.map_or(origin + direction * range, |h| h.point),
            color: color.unwrap_or(Vec3::new(1.0, 0.7, 0.1)),
            impact_kind: if hit.is_some() {
                RailImpactKind::World
            } else {
                RailImpactKind::None
            },
            impact_normal: hit.map(|h| h.normal),
            intensity: None,
        };
        self.voltaic.update_remote(now, entity_id, &sample, direction);
    }

    /// `fireMuzzleFlash`.
    pub fn fire_muzzle_flash(&mut self, now: f64, spawn: &MuzzleFlashSpawn) {
        self.muzzle_flashes.spawn(now, spawn);
    }

    /// `update`, once a frame: fades the flash lights, plays the impacts
    /// whose streaks have arrived, and ages the renderers.
    pub fn update(&mut self, now: f64, lights: &mut PointLightPool) {
        // Exponential decay, per frame as in TypeScript.
        self.effects.retain_mut(|fx| {
            if now - fx.spawn_time >= fx.duration {
                lights.release(fx.light);
                return false;
            }
            fx.intensity *= 0.85;
            lights.set_intensity_range(fx.light, fx.intensity, fx.range);
            true
        });
        if self.pending_impacts.iter().any(|i| i.due <= now) {
            let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_impacts)
                .into_iter()
                .partition(|i| i.due <= now);
            self.pending_impacts = waiting;
            for impact in &due {
                self.play_tracer_impact(now, lights, impact);
            }
        }
        self.voltaic.update(now);
        self.rails.update(now);
        self.muzzle_flashes.update(now);
        self.impact_sparks.update(now);
        self.streaks.update(now);
    }

    /// `clear`: every effect gone and its light given back.
    pub fn clear(&mut self, lights: &mut PointLightPool) {
        for fx in self.effects.drain(..) {
            lights.release(fx.light);
        }
        self.pending_impacts.clear();
        self.voltaic.clear();
        self.rails.clear();
        self.muzzle_flashes.clear();
        self.impact_sparks.clear();
        self.streaks.clear();
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_impact_sparks(
        &mut self,
        now: f64,
        position: DVec3,
        normal: DVec3,
        incoming: DVec3,
        color: Vec3,
        count: f64,
        heavy: bool,
    ) {
        self.impact_sparks.burst(
            now,
            &ImpactSparkBurst {
                position,
                normal,
                incoming,
                color,
                count: Some(count),
                heavy,
                speed: None,
                intensity: None,
            },
        );
    }

    /// `spawnImpactFlash`: a pooled light a little off the surface, held by
    /// an effect that lasts `duration`.
    #[allow(clippy::too_many_arguments)]
    fn spawn_impact_flash(
        &mut self,
        now: f64,
        lights: &mut PointLightPool,
        position: DVec3,
        color: Vec3,
        normal: Option<DVec3>,
        intensity: f32,
        range: f32,
        duration: f64,
    ) {
        let offset = normal.map_or(DVec3::ZERO, |n| n * 0.05);
        let light = lights.acquire(now, (position + offset).as_vec3(), color, intensity, range);
        self.effects.push(ActiveFlash {
            light,
            intensity,
            range,
            spawn_time: now,
            duration,
        });
    }

    /// Live streaks, sparks and flash lights, for tests and debugging.
    #[must_use]
    pub fn debug_counts(&self) -> (usize, usize, usize) {
        (
            self.streaks.debug_count(),
            self.impact_sparks.debug_count(),
            self.effects.len(),
        )
    }
}

/// `spreadDirectionSeeded`: a direction within the cone around `dir`.
fn spread_direction_seeded(dir: DVec3, spread_rad: f64, rng: &mut impl FnMut() -> f64) -> DVec3 {
    let r = rng().sqrt() * spread_rad.tan();
    let theta = rng() * std::f64::consts::PI * 2.0;
    let (offset_x, offset_y) = (r * theta.cos(), r * theta.sin());
    let forward = dir.normalize();
    let right = if forward.y.abs() < 0.999 {
        DVec3::Y.cross(forward).normalize()
    } else {
        DVec3::X.cross(forward).normalize()
    };
    let up = forward.cross(right).normalize();
    (forward + right * offset_x + up * offset_y).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat floor at y = 0.
    struct Floor;
    impl ShotWorld for Floor {
        fn trace_shot(&mut self, origin: DVec3, dir: DVec3, max_dist: f64) -> Option<ShotTrace> {
            if dir.y >= 0.0 {
                return None;
            }
            let t = -origin.y / dir.y;
            (t <= max_dist).then(|| ShotTrace {
                point: origin + dir * t,
                normal: DVec3::Y,
                marks: true,
            })
        }
    }

    #[test]
    fn spread_stays_in_the_cone() {
        let mut rng = mulberry32(7);
        for _ in 0..100 {
            let d = spread_direction_seeded(DVec3::Z, 0.1, &mut rng);
            assert!((d.length() - 1.0).abs() < 1e-12);
            assert!(d.z >= 0.1f64.cos() - 1e-9 || d.angle_between(DVec3::Z) <= 0.1 + 1e-9);
        }
    }

    #[test]
    fn server_pellets_point_along_the_shot() {
        let dir = DVec3::new(0.3, -0.2, 0.9).normalize();
        let pellets = server_pellet_directions(dir, 99, 18, 8.0);
        assert_eq!(pellets.len(), 18);
        for p in pellets {
            assert!(p.angle_between(dir) <= 8f64.to_radians() + 1e-9);
        }
        // Floor exists only to check the trait objects compose.
        assert!(Floor.trace_shot(DVec3::Y, -DVec3::Y, 10.0).is_some());
    }
}
