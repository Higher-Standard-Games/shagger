//! SHAG `src/engine/weapon/muzzle/MuzzleFlashRenderer.ts` and
//! `MuzzleFlashMaterial.ts` (`muzzle_flash.wgsl`): pooled one-shot
//! procedural muzzle flashes, one camera-facing quad each.
//!
//! Not yet: anchors, which move live flashes with the barrel.

use glam::{DVec3, Vec3};

use crate::rendering::effect_quad_batch::{EffectFrame, EffectQuadBatch, EffectSlots, instance};
use crate::rendering::gpu::Gpu;

/// Base world size of a flash before the weapon's scale.
const BASE_FLASH_SIZE: f64 = 8.5;

/// `MuzzleFlashStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MuzzleFlashStyle {
    #[default]
    Burst,
    Plume,
    Arcs,
}

impl MuzzleFlashStyle {
    /// `STYLE_TO_FLOAT`.
    #[must_use]
    pub fn as_float(self) -> f64 {
        match self {
            Self::Burst => 0.0,
            Self::Plume => 1.0,
            Self::Arcs => 2.0,
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "burst" => Some(Self::Burst),
            "plume" => Some(Self::Plume),
            "arcs" => Some(Self::Arcs),
            _ => None,
        }
    }
}

/// One muzzle flash.
#[derive(Debug, Clone, Copy)]
pub struct MuzzleFlashSpawn {
    pub position: DVec3,
    pub color: Vec3,
    pub style: MuzzleFlashStyle,
    /// The weapon's size multiplier (1 by default).
    pub scale: Option<f64>,
    /// The weapon's light intensity (20 by default).
    pub intensity: Option<f64>,
    /// Lifetime in seconds (0.06 by default).
    pub duration: Option<f64>,
}

pub struct MuzzleFlashRenderer {
    slots: EffectSlots,
    batch: EffectQuadBatch,
    seed_counter: u32,
}

impl MuzzleFlashRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let capacity = 24;
        Self {
            slots: EffectSlots::new(capacity),
            batch: EffectQuadBatch::new(
                gpu,
                color_format,
                "muzzle_flash",
                include_str!("muzzle_flash.wgsl"),
                capacity,
            ),
            seed_counter: 0,
        }
    }

    /// `spawn` at `now`.
    pub fn spawn(&mut self, now: f64, flash: &MuzzleFlashSpawn) {
        let duration = flash.duration.unwrap_or(0.06);
        self.seed_counter = (self.seed_counter + 1) % 8191;
        let seed = f64::from(self.seed_counter + 1);
        let size = BASE_FLASH_SIZE * flash.scale.unwrap_or(1.0);
        let (p, c) = (flash.position, flash.color.as_dvec3());
        self.slots.push(
            now,
            duration,
            instance([
                [p.x, p.y, p.z, seed],
                [c.x, c.y, c.z, now],
                [size, flash.intensity.unwrap_or(20.0), duration, flash.style.as_float()],
                [0.0; 4],
            ]),
        );
    }

    /// Drops the flashes that have faded.
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
