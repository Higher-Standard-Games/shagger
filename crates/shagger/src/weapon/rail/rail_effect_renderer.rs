//! SHAG `src/engine/weapon/rail/RailEffectRenderer.ts` and
//! `RailShaderMaterial.ts` (`rail_beam.wgsl`, `rail_impact.wgsl`): a pool of
//! one-shot rails, each one instance of the beam template and one of the
//! impact template.

use super::rail_beam_geometry::{build_rail_beam_template, build_rail_impact_template};
use super::rail_profiles::{RAIL_DURATION, RAIL_HELIX_RADIUS, RailEffectSample, impact_kind_to_float, make_rail_seed};
use crate::rendering::effect_quad_batch::{EffectFrame, EffectQuadBatch, EffectSlots, instance5};
use crate::rendering::gpu::Gpu;

pub struct RailEffectRenderer {
    slots: EffectSlots,
    beam: EffectQuadBatch,
    impact: EffectQuadBatch,
    activation_counter: u32,
}

impl RailEffectRenderer {
    /// The browser's pool of 24.
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let capacity = 24;
        Self {
            slots: EffectSlots::new(capacity),
            beam: EffectQuadBatch::with_template(
                gpu,
                color_format,
                "rail_beam",
                include_str!("rail_beam.wgsl"),
                capacity,
                &build_rail_beam_template(),
            ),
            impact: EffectQuadBatch::with_template(
                gpu,
                color_format,
                "rail_impact",
                include_str!("rail_impact.wgsl"),
                capacity,
                &build_rail_impact_template(),
            ),
            activation_counter: 0,
        }
    }

    /// `fireRail`: one rail from the muzzle (`start`) to the impact (`end`)
    /// at `now`, lasting `duration` (`RAIL_DURATION` by default).
    pub fn fire_rail(&mut self, now: f64, sample: &RailEffectSample, duration: Option<f64>) {
        let duration = duration.unwrap_or(RAIL_DURATION);
        self.activation_counter += 1;
        let seed = make_rail_seed("rail", self.activation_counter);
        let fallback = sample.start - sample.end;
        let fallback_length = match fallback.dot(fallback).sqrt() {
            0.0 => 1.0,
            length => length,
        };
        let normal = sample.impact_normal.unwrap_or(fallback / fallback_length);
        let (s, e, c) = (sample.start, sample.end, sample.color);
        self.slots.push(
            now,
            duration,
            instance5([
                [s.x, s.y, s.z, seed],
                [e.x, e.y, e.z, impact_kind_to_float(sample.impact_kind)],
                [f64::from(c.x), f64::from(c.y), f64::from(c.z), now],
                [sample.intensity.unwrap_or(1.0), RAIL_HELIX_RADIUS, 1.0, duration],
                [normal.x, normal.y, normal.z, 0.0],
            ]),
        );
    }

    /// `update`: drops the rails that have faded by `now`.
    pub fn update(&mut self, now: f64) {
        self.slots.expire(now);
    }

    /// Writes the frame's uniforms and the live rails.
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame) {
        let dirty = self.slots.dirty;
        self.beam.prepare(gpu, frame, &self.slots.data, dirty);
        self.impact.prepare(gpu, frame, &self.slots.data, dirty);
        self.slots.dirty = false;
    }

    /// The beams (`alphaIndex` 40).
    pub fn draw_beams(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.beam.draw(pass);
    }

    /// The muzzle cracks and terminals (`alphaIndex` 41).
    pub fn draw_impacts(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.impact.draw(pass);
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }

    #[must_use]
    pub fn debug_count(&self) -> usize {
        self.slots.slots.len()
    }
}
