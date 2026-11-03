//! SHAG `src/engine/weapon/WeaponSlash.ts`: the melee swing (windup, swing,
//! return), cycling through three presets, and the faint ribbon its tip
//! leaves in the world.
//!
//! The browser reads `performance.now()`; here the caller passes its clock
//! in milliseconds. The browser builds a mesh per trail and grows it; here
//! [`SlashTrailRenderer`] keeps a few ribbons of a fixed size and rewrites
//! them, as the browser does between growths.

use std::io::Cursor;

use glam::{DVec3, Mat4};

use crate::babylon::materials::standard_material::{StandardMaterialParams, TextureUse};
use crate::babylon::meshes::ribbon_builder::create_ribbon_double_side;
use crate::loader::model::gltf_model::ModelImage;
use crate::rendering::gpu::Gpu;
use crate::rendering::model_renderer::{ModelDraw, ModelHandle, ModelRenderer};
use crate::weapon::projectile_manager::upload_textured_standard_mesh;

/// `TRAIL_FADE_MS`.
pub const TRAIL_FADE_MS: f64 = 800.0;
/// `TRAIL_ALPHA`.
pub const TRAIL_ALPHA: f32 = 0.22;
/// Points a ribbon has room for; a swing stops adding once it is full
/// (450 ms of swing at 500 frames a second).
pub const TRAIL_CAPACITY: usize = 256;
/// Ribbons kept: one swing drawing while the last ones fade.
const TRAIL_POOL: usize = 4;

/// A trail: the tip's and the handle's paths.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlashTrail {
    pub tip_points: Vec<DVec3>,
    pub handle_points: Vec<DVec3>,
    /// When it began to fade, ms.
    pub fade_start: f64,
    pub active: bool,
}

impl SlashTrail {
    /// The material's alpha at `now_ms`; `None` once faded.
    #[must_use]
    pub fn alpha(&self, now_ms: f64) -> Option<f32> {
        if self.active {
            return Some(TRAIL_ALPHA);
        }
        let fade = (now_ms - self.fade_start) / TRAIL_FADE_MS;
        (fade < 1.0).then_some(TRAIL_ALPHA * (1.0 - fade as f32))
    }
}

/// `writeSlashRibbon`: the tip path then the handle path, each `capacity`
/// long (the spare points repeat the newest), then the same again for the
/// back face. U runs along the path as a share of its length, V is 0 on
/// the tip and 1 on the handle (0 where the two meet).
#[must_use]
pub fn slash_ribbon(trail: &SlashTrail, capacity: usize) -> (Vec<[f32; 3]>, Vec<[f32; 2]>) {
    let count = trail.tip_points.len();
    let mut positions = vec![[0.0f32; 3]; capacity * 4];
    let mut uvs = vec![[0.0f32; 2]; capacity * 4];
    if count == 0 {
        return (positions, uvs);
    }
    let paths = [&trail.tip_points, &trail.handle_points];
    let half = capacity * 2;
    for (p, path) in paths.iter().enumerate() {
        let total: f64 = (1..count).map(|j| path[j].distance(path[j - 1])).sum();
        let mut along = 0.0;
        for j in 0..capacity {
            let at = j.min(count - 1);
            let point = path[at];
            if j > 0 && j < count {
                along += point.distance(path[j - 1]);
            }
            let vertex = p * capacity + j;
            let width = trail.tip_points[at].distance(trail.handle_points[at]);
            let u = if total == 0.0 { 0.0 } else { along / total };
            let v = if p == 0 || width == 0.0 { 0.0 } else { 1.0 };
            for side in [0, half] {
                positions[side + vertex] = point.as_vec3().to_array();
                uvs[side + vertex] = [u as f32, v as f32];
            }
        }
    }
    (positions, uvs)
}

/// The feather: a 64 by 32 white texture whose alpha is a smooth envelope,
/// so every ribbon edge fades out.
#[must_use]
pub fn feather_pixels() -> Vec<u8> {
    let (width, height) = (64usize, 32usize);
    let mut pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let envelope = (std::f64::consts::PI * x as f64 / (width - 1) as f64).sin()
                * (std::f64::consts::PI * y as f64 / (height - 1) as f64).sin();
            let alpha = crate::shared::js::js_round(255.0 * envelope * envelope) as u8;
            pixels.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    pixels
}

/// `slashTrailMat`: a pale self-lit ribbon, both sides, feathered by its
/// texture's alpha, at [`TRAIL_ALPHA`].
#[must_use]
pub fn slash_trail_material() -> StandardMaterialParams {
    StandardMaterialParams {
        emissive_color: glam::Vec3::new(0.75, 0.8, 0.8),
        diffuse_color: glam::Vec3::ZERO,
        disable_lighting: true,
        back_face_culling: false,
        alpha: TRAIL_ALPHA,
        alpha_blend: true,
        texture: TextureUse {
            diffuse: true,
            alpha_from_diffuse: true,
            ..TextureUse::default()
        },
        ..StandardMaterialParams::default()
    }
}

/// The trails' ribbons on the GPU.
#[derive(Debug, Default)]
pub struct SlashTrailRenderer {
    ribbons: Vec<(ModelHandle, Vec<[f32; 3]>)>,
}

impl SlashTrailRenderer {
    /// Writes each visible trail into a ribbon and adds its draw.
    pub fn draws(
        &mut self,
        gpu: &Gpu,
        models: &mut ModelRenderer,
        slash: &WeaponSlash,
        now_ms: f64,
        out: &mut Vec<ModelDraw>,
    ) {
        let visible = slash
            .trails()
            .iter()
            .filter(|t| t.tip_points.len() >= 2)
            .filter_map(|t| t.alpha(now_ms).map(|a| (t, a)))
            .take(TRAIL_POOL);
        for (i, (trail, alpha)) in visible.enumerate() {
            if self.ribbons.len() <= i {
                let rest = vec![DVec3::ZERO; TRAIL_CAPACITY];
                let data = create_ribbon_double_side(&rest, &rest);
                let normals = data.normals.as_chunks::<3>().0.to_vec();
                let mut png = Cursor::new(Vec::new());
                image::write_buffer_with_format(
                    &mut png,
                    &feather_pixels(),
                    64,
                    32,
                    image::ExtendedColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .expect("encoding a PNG in memory");
                let feather = ModelImage {
                    name: "slashTrailFeather.png".to_owned(),
                    mime_type: Some("image/png".to_owned()),
                    bytes: png.into_inner(),
                };
                let handle = upload_textured_standard_mesh(
                    gpu,
                    models,
                    &[("slashTrail", &data)],
                    slash_trail_material(),
                    Some(feather),
                );
                self.ribbons.push((handle, normals));
            }
            let (handle, normals) = &self.ribbons[i];
            let (positions, uvs) = slash_ribbon(trail, TRAIL_CAPACITY);
            models.update_vertices(gpu, *handle, 0, &positions, normals, &uvs);
            out.push(ModelDraw {
                alpha: Some(alpha),
                ..ModelDraw::new(*handle, 0, Mat4::IDENTITY)
            });
        }
    }
}

/// One swing's shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlashParams {
    pub start_pitch: f64,
    pub start_roll: f64,
    pub start_yaw: f64,
    pub end_pitch: f64,
    pub end_roll: f64,
    pub end_yaw: f64,
    pub start_offset: [f64; 3],
    pub end_offset: [f64; 3],
    pub windup_duration: f64,
    pub swing_duration: f64,
    pub return_duration: f64,
}

/// `SLASH_PRESETS`, cycled on each swing.
pub const SLASH_PRESETS: [SlashParams; 3] = [
    // Big vertical down: raised high, sweeps down with a lunge.
    SlashParams {
        start_pitch: 0.0,
        start_roll: -2.0,
        start_yaw: 0.0,
        end_pitch: 0.0,
        end_roll: 1.4,
        end_yaw: 0.0,
        start_offset: [5.0, 30.0, -15.0],
        end_offset: [-5.0, -25.0, 18.0],
        windup_duration: 60.0,
        swing_duration: 160.0,
        return_duration: 230.0,
    },
    // Diagonal, upper right to lower left.
    SlashParams {
        start_pitch: 0.7,
        start_roll: -1.8,
        start_yaw: -0.4,
        end_pitch: -0.7,
        end_roll: 1.2,
        end_yaw: 0.4,
        start_offset: [-12.0, 28.0, -12.0],
        end_offset: [12.0, -22.0, 15.0],
        windup_duration: 55.0,
        swing_duration: 165.0,
        return_duration: 230.0,
    },
    // Diagonal, upper left to lower right.
    SlashParams {
        start_pitch: -0.7,
        start_roll: -1.8,
        start_yaw: 0.4,
        end_pitch: 0.7,
        end_roll: 1.2,
        end_yaw: -0.4,
        start_offset: [12.0, 28.0, -12.0],
        end_offset: [-12.0, -22.0, 15.0],
        windup_duration: 55.0,
        swing_duration: 165.0,
        return_duration: 230.0,
    },
];

/// `SlashAnimOffsets`: position and radians on the weapon root.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SlashAnimOffsets {
    pub x_offset: f64,
    pub y_offset: f64,
    pub z_offset: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
}

/// `WeaponSlash`'s animation.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponSlash {
    active: bool,
    start_ms: f64,
    params: SlashParams,
    preset_index: usize,
    trails: Vec<SlashTrail>,
    trail_was_active: bool,
}

impl Default for WeaponSlash {
    fn default() -> Self {
        Self {
            active: false,
            start_ms: 0.0,
            params: SLASH_PRESETS[0],
            preset_index: 0,
            trails: Vec::new(),
            trail_was_active: false,
        }
    }
}

impl WeaponSlash {
    /// A swing is animating.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// `trigger`: the next preset, from `now_ms`.
    pub fn trigger(&mut self, now_ms: f64) {
        self.params = SLASH_PRESETS[self.preset_index % SLASH_PRESETS.len()];
        self.preset_index += 1;
        self.active = true;
        self.start_ms = now_ms;
    }

    /// The trails drawing or fading.
    #[must_use]
    pub fn trails(&self) -> &[SlashTrail] {
        &self.trails
    }

    /// `updateTrail` at `now_ms`, for a camera at `eye` with these axes:
    /// a swing starts a trail and adds a tip and a handle point a frame;
    /// its end starts the fade.
    pub fn update_trail(&mut self, now_ms: f64, eye: DVec3, forward: DVec3, up: DVec3, right: DVec3) {
        let slashing = self.active;
        if slashing && !self.trail_was_active {
            for trail in &mut self.trails {
                if trail.active {
                    trail.active = false;
                    trail.fade_start = now_ms;
                }
            }
            self.trails.push(SlashTrail {
                active: true,
                ..SlashTrail::default()
            });
        }
        self.trail_was_active = slashing;
        if slashing {
            let off = self.offsets(now_ms);
            if let Some(trail) = self.trails.iter_mut().find(|t| t.active)
                && trail.tip_points.len() < TRAIL_CAPACITY
            {
                let handle = eye
                    + forward * 15.0
                    + up * -4.0
                    + right * 2.0
                    + up * (off.y_offset * 0.04)
                    + right * (off.x_offset * 0.04);
                let tip = eye
                    + forward * 28.0
                    + up * 5.0
                    + right * -2.0
                    + up * (off.y_offset * 0.25)
                    + right * (off.x_offset * 0.25)
                    + forward * (off.z_offset * 0.1);
                trail.tip_points.push(tip);
                trail.handle_points.push(handle);
            }
        } else if let Some(trail) = self.trails.iter_mut().find(|t| t.active) {
            trail.active = false;
            trail.fade_start = now_ms;
        }
        self.trails
            .retain(|t| t.active || (now_ms - t.fade_start) / TRAIL_FADE_MS < 1.0);
    }

    /// `getOffsets` at `now_ms`; the swing ends once its time is up.
    pub fn offsets(&mut self, now_ms: f64) -> SlashAnimOffsets {
        if !self.active {
            return SlashAnimOffsets::default();
        }
        let elapsed = now_ms - self.start_ms;
        let p = self.params;
        let total = p.windup_duration + p.swing_duration + p.return_duration;
        if elapsed >= total {
            self.active = false;
            return SlashAnimOffsets::default();
        }
        let zero = [0.0; 3];
        let (t, from, to, from_off, to_off) = if elapsed < p.windup_duration {
            let t = if p.windup_duration > 0.0 {
                elapsed / p.windup_duration
            } else {
                1.0
            };
            (
                t * t,
                [0.0; 3],
                [p.start_pitch, p.start_roll, p.start_yaw],
                zero,
                p.start_offset,
            )
        } else if elapsed < p.windup_duration + p.swing_duration {
            let t = (elapsed - p.windup_duration) / p.swing_duration;
            (
                1.0 - (1.0 - t) * (1.0 - t),
                [p.start_pitch, p.start_roll, p.start_yaw],
                [p.end_pitch, p.end_roll, p.end_yaw],
                p.start_offset,
                p.end_offset,
            )
        } else {
            let t = (elapsed - p.windup_duration - p.swing_duration) / p.return_duration;
            let eased = if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            };
            (
                eased,
                [p.end_pitch, p.end_roll, p.end_yaw],
                [0.0; 3],
                p.end_offset,
                zero,
            )
        };
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        SlashAnimOffsets {
            x_offset: lerp(from_off[0], to_off[0]),
            y_offset: lerp(from_off[1], to_off[1]),
            z_offset: lerp(from_off[2], to_off[2]),
            pitch: lerp(from[0], to[0]),
            roll: lerp(from[1], to[1]),
            yaw: lerp(from[2], to[2]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swing_leaves_a_trail_that_fades() {
        let mut s = WeaponSlash::default();
        s.trigger(0.0);
        let mut now = 0.0;
        while s.is_active() {
            s.update_trail(now, DVec3::ZERO, DVec3::Z, DVec3::Y, DVec3::X);
            now += 16.0;
        }
        s.update_trail(now, DVec3::ZERO, DVec3::Z, DVec3::Y, DVec3::X);
        let trail = &s.trails()[0];
        assert!(!trail.active && trail.tip_points.len() > 20);
        // The first point: the windup's start, close to the rest pose.
        assert!((trail.tip_points[0] - DVec3::new(-2.0, 5.0, 28.0)).length() < 1.0);
        assert_eq!(trail.alpha(now), Some(TRAIL_ALPHA));
        assert!((trail.alpha(now + 400.0).unwrap() - TRAIL_ALPHA / 2.0).abs() < 1e-6);
        s.update_trail(now + 800.0, DVec3::ZERO, DVec3::Z, DVec3::Y, DVec3::X);
        assert!(s.trails().is_empty());
    }

    #[test]
    fn the_ribbon_runs_tip_then_handle_and_pads_with_the_newest() {
        let trail = SlashTrail {
            tip_points: vec![DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0)],
            handle_points: vec![DVec3::Y, DVec3::new(2.0, 1.0, 0.0)],
            ..SlashTrail::default()
        };
        let (p, uv) = slash_ribbon(&trail, 4);
        assert_eq!(p.len(), 16);
        assert_eq!(p[1], [2.0, 0.0, 0.0]);
        assert_eq!(p[3], [2.0, 0.0, 0.0]);
        assert_eq!(p[4], [0.0, 1.0, 0.0]);
        assert_eq!(uv[1], [1.0, 0.0]);
        assert_eq!(uv[5], [1.0, 1.0]);
        // The back face repeats the front.
        assert_eq!(p[9], p[1]);
        let f = feather_pixels();
        assert_eq!(f[3], 0);
        assert!(f[(16 * 64 + 32) * 4 + 3] > 240);
    }

    #[test]
    fn a_swing_winds_up_sweeps_and_returns() {
        let mut s = WeaponSlash::default();
        assert_eq!(s.offsets(0.0), SlashAnimOffsets::default());
        s.trigger(1000.0);
        // End of the windup: the first preset's start pose.
        let o = s.offsets(1060.0);
        assert!((o.y_offset - 30.0).abs() < 1e-9 && (o.roll + 2.0).abs() < 1e-9);
        // End of the swing.
        let o = s.offsets(1220.0);
        assert!((o.y_offset + 25.0).abs() < 1e-9 && (o.z_offset - 18.0).abs() < 1e-9);
        assert!(s.is_active());
        assert_eq!(s.offsets(1450.0), SlashAnimOffsets::default());
        assert!(!s.is_active());
        // The next swing is the second preset.
        s.trigger(2000.0);
        let o = s.offsets(2055.0);
        assert!((o.x_offset + 12.0).abs() < 1e-9 && (o.yaw + 0.4).abs() < 1e-9);
    }
}
