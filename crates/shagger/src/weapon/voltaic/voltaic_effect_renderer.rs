//! SHAG `src/engine/weapon/voltaic/VoltaicEffectRenderer.ts` and
//! `VoltaicShaderMaterial.ts` (`voltaic_beam.wgsl`, `voltaic_impact.wgsl`):
//! the local hero beam, remote players' beams and chain arcs.

use std::collections::HashMap;

use glam::DVec3;

use super::voltaic_beam_geometry::{VoltaicBeamProfile, build_voltaic_beam_template, build_voltaic_impact_template};
use super::voltaic_profiles::{
    VOLTAIC_HERO_RELEASE_TIME, VOLTAIC_REMOTE_REMOVE_TIME, VoltaicEffectSample, impact_kind_to_float, make_voltaic_seed,
};
use crate::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch, instance5};
use crate::rendering::gpu::Gpu;
use crate::shared::js::js_round;

const BEAM_WGSL: &str = include_str!("voltaic_beam.wgsl");
const IMPACT_WGSL: &str = include_str!("voltaic_impact.wgsl");
const POSITION_EPSILON_SQUARED: f64 = 0.0001;

/// `quantizeArcCoordinate`.
fn quantize_arc_coordinate(value: f64) -> i64 {
    js_round(value * 0.125) as i64
}

/// The shared vec4s: `iStartSeed`, `iEndImpact`, `iColorLife`, `iParams`,
/// `iNormal`.
fn sample_vectors(sample: &VoltaicEffectSample, seed: f64, life: f64, params: [f64; 4]) -> [[f64; 4]; 5] {
    let fallback = sample.start - sample.end;
    let fallback_length = match fallback.dot(fallback).sqrt() {
        0.0 => 1.0,
        length => length,
    };
    let normal = sample.impact_normal.unwrap_or(fallback / fallback_length);
    let (s, e, c) = (sample.start, sample.end, sample.color);
    [
        [s.x, s.y, s.z, seed],
        [e.x, e.y, e.z, impact_kind_to_float(sample.impact_kind)],
        [f64::from(c.x), f64::from(c.y), f64::from(c.z), life],
        params,
        [normal.x, normal.y, normal.z, 0.0],
    ]
}

/// What a remote start resolver answers: the muzzle for an entity aiming
/// along a direction, or `None` to keep the last one.
pub type VoltaicRemoteStartResolver = dyn FnMut(&str, DVec3) -> Option<DVec3>;

struct BatchSlot {
    key: String,
    seed: f64,
    touched_at: f64,
    duration: f64,
    /// 0 remote, 1 chain.
    mode: u8,
    direction: DVec3,
    dynamic_start: bool,
}

/// `VoltaicInstanceBatch`: keyed slots over one beam and one impact batch.
struct VoltaicInstanceBatch {
    beam: EffectQuadBatch,
    impact: EffectQuadBatch,
    capacity: usize,
    slots: Vec<BatchSlot>,
    data: Vec<EffectInstance>,
    slot_by_key: HashMap<String, usize>,
    dirty: bool,
    activation_counter: u32,
}

impl VoltaicInstanceBatch {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat, name: &str, profile: VoltaicBeamProfile, capacity: usize) -> Self {
        Self {
            beam: EffectQuadBatch::with_template(
                gpu,
                format,
                &format!("{name}_beam"),
                BEAM_WGSL,
                capacity,
                &build_voltaic_beam_template(profile),
            ),
            impact: EffectQuadBatch::with_template(
                gpu,
                format,
                &format!("{name}_impact"),
                IMPACT_WGSL,
                capacity,
                &build_voltaic_impact_template(),
            ),
            capacity,
            slots: Vec::new(),
            data: Vec::new(),
            slot_by_key: HashMap::new(),
            dirty: false,
            activation_counter: 0,
        }
    }

    fn remove_at(&mut self, index: usize) {
        let removed = self.slots.swap_remove(index);
        self.data.swap_remove(index);
        self.slot_by_key.remove(&removed.key);
        if let Some(moved) = self.slots.get(index) {
            self.slot_by_key.insert(moved.key.clone(), index);
        }
        self.dirty = true;
    }

    fn least_recently_touched_index(&self) -> usize {
        let mut oldest = 0;
        for i in 1..self.slots.len() {
            if self.slots[i].touched_at < self.slots[oldest].touched_at {
                oldest = i;
            }
        }
        oldest
    }

    fn upsert(
        &mut self,
        key: &str,
        sample: &VoltaicEffectSample,
        mode: u8,
        duration: f64,
        now: f64,
        anchor_direction: Option<DVec3>,
    ) {
        let index = if let Some(&index) = self.slot_by_key.get(key) {
            index
        } else {
            if self.slots.len() >= self.capacity {
                self.remove_at(self.least_recently_touched_index());
            }
            self.activation_counter += 1;
            self.slots.push(BatchSlot {
                key: key.to_owned(),
                seed: make_voltaic_seed(key, self.activation_counter),
                touched_at: now,
                duration,
                mode,
                direction: DVec3::Z,
                dynamic_start: false,
            });
            self.data.push([0.0; 20]);
            self.slot_by_key.insert(key.to_owned(), self.slots.len() - 1);
            self.slots.len() - 1
        };
        let slot = &mut self.slots[index];
        slot.touched_at = now;
        slot.duration = duration;
        slot.mode = mode;
        if let Some(direction) = anchor_direction {
            let length = direction.length_squared().sqrt();
            if length > 0.0001 {
                slot.direction = direction / length;
            }
            slot.dynamic_start = true;
        } else {
            slot.dynamic_start = false;
        }
        let amplitude = if mode == 1 { 13.0 } else { 8.0 };
        let params = [sample.intensity.unwrap_or(1.0), amplitude, f64::from(mode), duration];
        self.data[index] = instance5(sample_vectors(sample, slot.seed, now, params));
        self.dirty = true;
    }

    fn refresh_starts(&mut self, resolver: &mut VoltaicRemoteStartResolver) {
        for (slot, data) in self.slots.iter().zip(self.data.iter_mut()) {
            if !slot.dynamic_start {
                continue;
            }
            let Some(start) = resolver(&slot.key, slot.direction) else {
                continue;
            };
            if !(start.x + start.y + start.z).is_finite() {
                continue;
            }
            let current = DVec3::new(f64::from(data[0]), f64::from(data[1]), f64::from(data[2]));
            if (current - start).length_squared() <= POSITION_EPSILON_SQUARED {
                continue;
            }
            data[0] = start.x as f32;
            data[1] = start.y as f32;
            data[2] = start.z as f32;
            self.dirty = true;
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(&index) = self.slot_by_key.get(key) {
            self.remove_at(index);
        }
    }

    fn update(&mut self, now: f64) {
        for i in (0..self.slots.len()).rev() {
            let slot = &self.slots[i];
            let lifetime = if slot.mode == 1 {
                slot.duration
            } else {
                VOLTAIC_REMOTE_REMOVE_TIME
            };
            if now - slot.touched_at >= lifetime {
                self.remove_at(i);
            }
        }
    }

    fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame) {
        self.beam.prepare(gpu, frame, &self.data, self.dirty);
        self.impact.prepare(gpu, frame, &self.data, self.dirty);
        self.dirty = false;
    }

    fn clear(&mut self) {
        self.slots.clear();
        self.data.clear();
        self.slot_by_key.clear();
        self.dirty = false;
    }
}

/// `VoltaicEffectRenderer`.
pub struct VoltaicEffectRenderer {
    hero_beam: EffectQuadBatch,
    hero_impact: EffectQuadBatch,
    /// The hero's uniforms, as one instance.
    hero: [[f64; 4]; 5],
    remote: VoltaicInstanceBatch,
    chain: VoltaicInstanceBatch,
    remote_start_resolver: Option<Box<VoltaicRemoteStartResolver>>,
    hero_firing: bool,
    hero_visible: bool,
    hero_impact_enabled: bool,
    hero_release_started: f64,
    hero_activation: u32,
}

impl VoltaicEffectRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        Self {
            hero_beam: EffectQuadBatch::with_template(
                gpu,
                format,
                "voltaic_hero_beam",
                BEAM_WGSL,
                1,
                &build_voltaic_beam_template(VoltaicBeamProfile::Hero),
            ),
            hero_impact: EffectQuadBatch::with_template(
                gpu,
                format,
                "voltaic_hero_impact",
                IMPACT_WGSL,
                1,
                &build_voltaic_impact_template(),
            ),
            hero: [[0.0; 4]; 5],
            remote: VoltaicInstanceBatch::new(gpu, format, "voltaic_world", VoltaicBeamProfile::World, 32),
            chain: VoltaicInstanceBatch::new(gpu, format, "voltaic_chain", VoltaicBeamProfile::Chain, 24),
            remote_start_resolver: None,
            hero_firing: false,
            hero_visible: false,
            hero_impact_enabled: false,
            hero_release_started: 0.0,
            hero_activation: 0,
        }
    }

    /// `bindHeroSample`: the hero's field is broader than the remote one.
    fn bind_hero_sample(&mut self, sample: &VoltaicEffectSample, life: f64) {
        let seed = self.hero[0][3];
        let params = [sample.intensity.unwrap_or(1.0), 23.0, 2.0, VOLTAIC_HERO_RELEASE_TIME];
        self.hero = sample_vectors(sample, seed, life, params);
    }

    /// `updateHero`: call every frame while the local player fires.
    pub fn update_hero(&mut self, sample: &VoltaicEffectSample) {
        if !self.hero_firing {
            self.hero_activation += 1;
            self.hero[0][3] = make_voltaic_seed("hero", self.hero_activation);
            self.hero_visible = true;
        }
        self.hero_firing = true;
        self.hero_impact_enabled = true;
        self.bind_hero_sample(sample, 1.0);
    }

    pub fn stop_hero(&mut self, now: f64) {
        if !self.hero_firing {
            return;
        }
        self.hero_firing = false;
        self.hero_release_started = now;
    }

    /// `updateRemote`: a remote player's beam, keyed by its entity.
    pub fn update_remote(&mut self, now: f64, key: &str, sample: &VoltaicEffectSample, direction: DVec3) {
        self.remote.upsert(key, sample, 0, 0.0, now, Some(direction));
    }

    pub fn remove_remote(&mut self, key: &str) {
        self.remote.remove(key);
    }

    pub fn set_remote_start_resolver(&mut self, resolver: Option<Box<VoltaicRemoteStartResolver>>) {
        self.remote_start_resolver = resolver;
    }

    /// `fireChainArc`: keyed by its quantised endpoints, so a repeat arc
    /// refreshes the same slot.
    pub fn fire_chain_arc(&mut self, now: f64, sample: &VoltaicEffectSample, duration: f64) {
        let q = quantize_arc_coordinate;
        let (s, e) = (sample.start, sample.end);
        let key = format!(
            "chain:{},{},{}:{},{},{}",
            q(s.x),
            q(s.y),
            q(s.z),
            q(e.x),
            q(e.y),
            q(e.z)
        );
        self.chain.upsert(&key, sample, 1, duration, now, None);
    }

    /// `update`: expires remote beams and arcs and fades a released hero.
    pub fn update(&mut self, now: f64) {
        self.remote.update(now);
        self.chain.update(now);
        if !self.hero_firing && self.hero_visible {
            let life = (1.0 - (now - self.hero_release_started) / VOLTAIC_HERO_RELEASE_TIME).max(0.0);
            self.hero[2][3] = life;
            if life <= 0.0 {
                self.hero_visible = false;
                self.hero_impact_enabled = false;
            }
        }
    }

    /// Writes the frame. Remote beams first leave this frame's muzzles
    /// (the browser's before-camera-render observer).
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame) {
        if let Some(resolver) = self.remote_start_resolver.as_mut() {
            self.remote.refresh_starts(resolver.as_mut());
        }
        let hero = [instance5(self.hero)];
        let beam: &[EffectInstance] = if self.hero_visible { &hero } else { &[] };
        let impact: &[EffectInstance] = if self.hero_impact_enabled { &hero } else { &[] };
        self.hero_beam.prepare(gpu, frame, beam, true);
        self.hero_impact.prepare(gpu, frame, impact, true);
        self.remote.prepare(gpu, frame);
        self.chain.prepare(gpu, frame);
    }

    /// The beams (`alphaIndex` 40).
    pub fn draw_beams(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.hero_beam.draw(pass);
        self.remote.beam.draw(pass);
        self.chain.beam.draw(pass);
    }

    /// Endpoints, muzzle coronas and contact sparks (`alphaIndex` 41).
    pub fn draw_impacts(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.hero_impact.draw(pass);
        self.remote.impact.draw(pass);
        self.chain.impact.draw(pass);
    }

    pub fn clear(&mut self) {
        self.hero_firing = false;
        self.hero_visible = false;
        self.hero_impact_enabled = false;
        self.remote.clear();
        self.chain.clear();
    }

    /// `getDebugCounts`: remote beams, chain arcs, whether the hero shows.
    #[must_use]
    pub fn debug_counts(&self) -> (usize, usize, bool) {
        (self.remote.slots.len(), self.chain.slots.len(), self.hero_visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_keys_quantise_as_math_round() {
        // Math.round(-4 * 0.125) is -0, which prints as 0; 12 * 0.125 rounds up.
        assert_eq!(quantize_arc_coordinate(-4.0), 0);
        assert_eq!(quantize_arc_coordinate(12.0), 2);
        assert_eq!(quantize_arc_coordinate(-12.0), -1);
    }
}
