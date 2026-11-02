//! SHAG `src/engine/weapon/tracer/StreakTracerRenderer.ts` and
//! `StreakTracerMaterial.ts` (`streak_tracer.wgsl`): pooled bullet streaks,
//! one instance per shot; the vertex shader slides each from the muzzle to
//! the hit.

use glam::{DVec3, Vec3};

use crate::rendering::effect_quad_batch::{EffectFrame, EffectQuadBatch, EffectSlots, instance};
use crate::rendering::gpu::Gpu;

/// How fast a streak travels.
pub const STREAK_SPEED: f64 = 12000.0;
/// Streak length, shortened on short shots.
const STREAK_LENGTH: f64 = 110.0;
/// Half-width of the glow; the shader keeps at least a pixel and a half.
const STREAK_HALF_WIDTH: f64 = 1.1;

/// One shot's streak.
#[derive(Debug, Clone, Copy)]
pub struct StreakTracerShot {
    /// The muzzle.
    pub start: DVec3,
    /// The hit, or the end of the weapon's range.
    pub end: DVec3,
    pub color: Vec3,
    /// Brightness, the weapon's muzzle flash intensity (18 by default).
    pub intensity: Option<f64>,
}

pub struct StreakTracerRenderer {
    slots: EffectSlots,
    batch: EffectQuadBatch,
}

impl StreakTracerRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let capacity = 128;
        Self {
            slots: EffectSlots::new(capacity),
            batch: EffectQuadBatch::new(
                gpu,
                color_format,
                "tracer_streak",
                include_str!("streak_tracer.wgsl"),
                capacity,
            ),
        }
    }

    /// `fire` at `now`: returns the seconds until the head reaches the end.
    pub fn fire(&mut self, now: f64, shot: &StreakTracerShot) -> f64 {
        let d = shot.end - shot.start;
        let distance = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
        let length = STREAK_LENGTH.min((distance * 0.6).max(8.0));
        let c = shot.color.as_dvec3();
        self.slots.push(
            now,
            (distance + length) / STREAK_SPEED,
            instance([
                [shot.start.x, shot.start.y, shot.start.z, now],
                [shot.end.x, shot.end.y, shot.end.z, STREAK_SPEED],
                [c.x, c.y, c.z, length],
                [STREAK_HALF_WIDTH, shot.intensity.unwrap_or(18.0), 0.0, 0.0],
            ]),
        );
        distance / STREAK_SPEED
    }

    /// Drops the streaks whose tails have reached their ends.
    pub fn update(&mut self, now: f64) {
        self.slots.expire(now);
    }

    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame) {
        self.batch.prepare(gpu, frame, &self.slots.data, self.slots.dirty);
        self.slots.dirty = false;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.batch.draw(pass);
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }

    #[must_use]
    pub fn debug_count(&self) -> usize {
        self.slots.data.len()
    }
}
