//! SHAG `src/engine/weapon/impact/ImpactSparkRenderer.ts` and
//! `ImpactSparkMaterial.ts` (`impact_spark.wgsl`): pooled impact sparks,
//! one instance per ember (flying its own ballistic arc in the vertex
//! shader) and one for a heavy weapon's hit star.

use glam::{DVec3, Vec3};

use crate::rendering::effect_quad_batch::{EffectFrame, EffectQuadBatch, EffectSlots, instance};
use crate::rendering::gpu::Gpu;
use crate::shared::js::{js_cos, js_sin};
use crate::shared::spread_utils::mulberry32;

/// Quake gravity.
const SPARK_GRAVITY: f64 = 800.0;
const BASE_SPARK_SPEED: f64 = 265.0;
const BASE_SPARK_SIZE: f64 = 3.4;
const SPARK_MIN_DURATION: f64 = 0.16;
const SPARK_MAX_DURATION: f64 = 0.34;
const STAR_SIZE: f64 = 15.0;
const STAR_DURATION: f64 = 0.17;
/// How far the scatter leans off the mirror direction toward the normal.
const NORMAL_BIAS: f64 = 0.36;
/// Half-angle of the scatter cone, in radians.
const SCATTER_CONE: f64 = 0.62;
/// Smallest component every ember keeps along the normal.
const MIN_NORMAL_COMPONENT: f64 = 0.15;

/// One impact's worth of sparks.
#[derive(Debug, Clone, Copy)]
pub struct ImpactSparkBurst {
    /// The contact point.
    pub position: DVec3,
    /// The surface normal there.
    pub normal: DVec3,
    /// Which way the shot travelled; the scatter mirrors it.
    pub incoming: DVec3,
    pub color: Vec3,
    /// Embers (8 by default).
    pub count: Option<f64>,
    /// Adds the hit star.
    pub heavy: bool,
    /// Launch speed multiplier (1 by default).
    pub speed: Option<f64>,
    /// Brightness (20 by default).
    pub intensity: Option<f64>,
}

pub struct ImpactSparkRenderer {
    slots: EffectSlots,
    batch: EffectQuadBatch,
    seed_counter: u32,
    random: Box<dyn FnMut() -> f64>,
}

/// `1 / Math.sqrt(x)` when `x` is past the threshold, else 0.
fn inverse_length(squared: f64) -> f64 {
    if squared > 1e-6 { 1.0 / squared.sqrt() } else { 0.0 }
}

impl ImpactSparkRenderer {
    /// The browser's pool: 768 instances, the scatter from mulberry32 seed
    /// `0x5eed`.
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let capacity = 768;
        Self {
            slots: EffectSlots::new(capacity),
            batch: EffectQuadBatch::new(
                gpu,
                color_format,
                "impact_spark",
                include_str!("impact_spark.wgsl"),
                capacity,
            ),
            seed_counter: 0,
            random: Box::new(mulberry32(0x5eed)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        now: f64,
        position: DVec3,
        velocity: DVec3,
        kind: f64,
        color: Vec3,
        size: f64,
        duration: f64,
        gravity: f64,
        intensity: f64,
    ) {
        self.seed_counter = (self.seed_counter + 1) % 8191;
        let c = color.as_dvec3();
        self.slots.push(
            now,
            duration,
            instance([
                [position.x, position.y, position.z, f64::from(self.seed_counter + 1)],
                [velocity.x, velocity.y, velocity.z, kind],
                [c.x, c.y, c.z, now],
                [size, duration, gravity, intensity],
            ]),
        );
    }

    /// `burst` at `now`: the embers, plus the hit star when heavy.
    pub fn burst(&mut self, now: f64, burst: &ImpactSparkBurst) {
        let count = burst.count.unwrap_or(8.0).floor().max(0.0) as usize;
        let intensity = burst.intensity.unwrap_or(20.0);
        let n = burst.normal;
        let inverse_normal = inverse_length(n.x * n.x + n.y * n.y + n.z * n.z);
        let has_normal = inverse_normal != 0.0;
        let un = DVec3::new(
            if has_normal { n.x * inverse_normal } else { 0.0 },
            if has_normal { n.y * inverse_normal } else { 1.0 },
            if has_normal { n.z * inverse_normal } else { 0.0 },
        );
        let i = burst.incoming;
        let inverse_incoming = inverse_length(i.x * i.x + i.y * i.y + i.z * i.z);
        let ui = DVec3::new(i.x * inverse_incoming, i.y * inverse_incoming, i.z * inverse_incoming);
        // Mirror the shot about the surface, then lean it toward the normal.
        let dot = ui.x * un.x + ui.y * un.y + ui.z * un.z;
        let mut base = DVec3::new(
            ui.x - 2.0 * dot * un.x,
            ui.y - 2.0 * dot * un.y,
            ui.z - 2.0 * dot * un.z,
        );
        base = DVec3::new(
            base.x + (un.x - base.x) * NORMAL_BIAS,
            base.y + (un.y - base.y) * NORMAL_BIAS,
            base.z + (un.z - base.z) * NORMAL_BIAS,
        );
        let base_squared = base.x * base.x + base.y * base.y + base.z * base.z;
        if base_squared > 1e-6 {
            let inverse = 1.0 / base_squared.sqrt();
            base = DVec3::new(base.x * inverse, base.y * inverse, base.z * inverse);
        } else {
            base = un;
        }
        // An orthonormal frame around the scatter axis.
        let fallback = base.y.abs() < 0.999;
        let mut right = DVec3::new(
            if fallback { -base.z } else { 0.0 },
            0.0,
            if fallback { base.x } else { 1.0 },
        );
        let right_squared = right.x * right.x + right.y * right.y + right.z * right.z;
        if right_squared > 1e-6 {
            let inverse = 1.0 / right_squared.sqrt();
            right = DVec3::new(right.x * inverse, right.y * inverse, right.z * inverse);
        }
        let up = DVec3::new(
            base.y * right.z - base.z * right.y,
            base.z * right.x - base.x * right.z,
            base.x * right.y - base.y * right.x,
        );
        let speed_scale = burst.speed.unwrap_or(1.0);
        for _ in 0..count {
            let radius = (self.random)().sqrt() * SCATTER_CONE.tan();
            let theta = (self.random)() * std::f64::consts::PI * 2.0;
            let offset_right = radius * js_cos(theta);
            let offset_up = radius * js_sin(theta);
            let mut dir = DVec3::new(
                base.x + right.x * offset_right + up.x * offset_up,
                base.y + right.y * offset_right + up.y * offset_up,
                base.z + right.z * offset_right + up.z * offset_up,
            );
            let inverse = inverse_length(dir.x * dir.x + dir.y * dir.y + dir.z * dir.z);
            dir = DVec3::new(dir.x * inverse, dir.y * inverse, dir.z * inverse);
            // Lift anything aimed at or below the surface back out.
            let along_normal = dir.x * un.x + dir.y * un.y + dir.z * un.z;
            if along_normal < MIN_NORMAL_COMPONENT {
                let lift = MIN_NORMAL_COMPONENT - along_normal;
                dir = DVec3::new(dir.x + un.x * lift, dir.y + un.y * lift, dir.z + un.z * lift);
                let inverse = inverse_length(dir.x * dir.x + dir.y * dir.y + dir.z * dir.z);
                dir = DVec3::new(dir.x * inverse, dir.y * inverse, dir.z * inverse);
            }
            let speed = BASE_SPARK_SPEED * speed_scale * (0.45 + 0.75 * (self.random)());
            let duration = SPARK_MIN_DURATION + (SPARK_MAX_DURATION - SPARK_MIN_DURATION) * (self.random)();
            let size = BASE_SPARK_SIZE * (0.75 + 0.5 * (self.random)());
            self.push(
                now,
                burst.position,
                DVec3::new(dir.x * speed, dir.y * speed, dir.z * speed),
                0.0,
                burst.color,
                size,
                duration,
                SPARK_GRAVITY,
                intensity,
            );
        }
        if burst.heavy {
            // The star holds its place: no velocity, no gravity.
            self.push(
                now,
                burst.position,
                DVec3::ZERO,
                1.0,
                burst.color,
                STAR_SIZE,
                STAR_DURATION,
                0.0,
                intensity,
            );
        }
    }

    /// Drops the sparks that have died.
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
