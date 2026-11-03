//! SHAG `src/engine/weapon/AmmoCounterDisplay.ts`: the live ammo count on
//! the gun's counter screen, mounted at the model's `tag_ammo`. The `.cfg`
//! beside the model sizes it (`ammoCounter <font> <fontSize> <digitW>
//! <digitH> <digitAlpha> <iconSize> <iconAlpha>`).
//!
//! The browser paints the three digits into a canvas each time the count
//! changes; here the ten digits are painted once into a strip, the same way
//! (Orbitron at 900, near black, in a yellow glow), and each of the three
//! places is its own quad showing one cell of it.
//!
//! The browser draws the counter in its own rendering group with the depth
//! cleared, so the gun never covers it; here its material passes the depth
//! test always and draws after the gun's blended layers.

use std::io::Cursor;

use glam::{DVec3, Mat4, Quat, Vec3};

use crate::babylon::materials::standard_material::{StandardMaterialParams, TextureUse};
use crate::babylon::meshes::vertex_data::VertexData;
use crate::core::cvar::{CvarOptions, CvarType, Cvars};
use crate::loader::model::gltf_model::ModelImage;
use crate::rendering::gpu::Gpu;
use crate::rendering::model_renderer::{ModelDraw, ModelHandle, ModelRenderer};
use crate::ui::font::Font;
use crate::weapon::projectile_manager::upload_textured_standard_mesh;
use crate::weapon::weapon_barrels::babylon_euler;

/// Places the counter shows (hundreds).
pub const MAX_DIGITS: usize = 3;
/// A digit's cell in the strip: a third of the browser's 256 by 96
/// canvas.
pub const CELL_WIDTH: u32 = 85;
pub const CELL_HEIGHT: u32 = 96;

/// The parsed `ammoCounter` directive.
#[derive(Debug, Clone, PartialEq)]
pub struct AmmoCounterConfig {
    pub font_family: String,
    pub font_size: f64,
    pub digit_width: f64,
    pub digit_height: f64,
    pub digit_alpha: f64,
    pub icon_size: f64,
    pub icon_alpha: f64,
}

/// `parseAmmoCounter`: the `ammoCounter` line of a weapon `.cfg`; `None`
/// when it declares none.
#[must_use]
pub fn parse_ammo_counter(cfg: &str) -> Option<AmmoCounterConfig> {
    for raw in cfg.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        let Some(rest) = line.get(..11).filter(|head| head.eq_ignore_ascii_case("ammocounter")) else {
            continue;
        };
        let rest = &line[rest.len()..];
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let (font, numbers) = if let Some(quoted) = rest.strip_prefix('"') {
            let Some(end) = quoted.find('"') else { continue };
            (&quoted[..end], &quoted[end + 1..])
        } else {
            match rest.split_once(char::is_whitespace) {
                Some(pair) => pair,
                None => continue,
            }
        };
        let nums: Vec<f64> = numbers
            .split_whitespace()
            .map(|n| n.parse::<f64>().unwrap_or(f64::NAN))
            .collect();
        if nums.len() < 6 || nums.iter().any(|n| !n.is_finite()) || font.is_empty() {
            continue;
        }
        return Some(AmmoCounterConfig {
            font_family: font.to_owned(),
            font_size: nums[0],
            digit_width: nums[1],
            digit_height: nums[2],
            digit_alpha: nums[3],
            icon_size: nums[4],
            icon_alpha: nums[5],
        });
    }
    None
}

/// `formatAmmoDigits`: fixed width, leading zeros blank, clamped to what
/// the places hold.
#[must_use]
pub fn format_ammo_digits(ammo: i64, max_digits: usize) -> String {
    let max = 10i64.pow(max_digits as u32) - 1;
    format!("{:>max_digits$}", ammo.clamp(0, max))
}

/// The live tuning cvars (`cg_ammocounter_*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmmoCounterTuning {
    pub scale: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub fwd: f64,
}

impl Default for AmmoCounterTuning {
    fn default() -> Self {
        Self {
            scale: 1.0,
            yaw: 90.0,
            pitch: 0.0,
            roll: 180.0,
            fwd: 0.0,
        }
    }
}

impl AmmoCounterTuning {
    /// This frame's values.
    #[must_use]
    pub fn read(cvars: &Cvars) -> Self {
        let d = Self::default();
        let num = |name: &str, fallback: f64| {
            if cvars.exists(name) {
                cvars.get_number(name)
            } else {
                fallback
            }
        };
        Self {
            scale: num("cg_ammocounter_scale", d.scale),
            yaw: num("cg_ammocounter_yaw", d.yaw),
            pitch: num("cg_ammocounter_pitch", d.pitch),
            roll: num("cg_ammocounter_roll", d.roll),
            fwd: num("cg_ammocounter_fwd", d.fwd),
        }
    }
}

/// Registers `cg_ammocounter_*`.
pub fn register_ammo_counter_cvars(cvars: &mut Cvars) {
    let float = |description: &str, min: f64, max: f64| {
        CvarOptions::describe(description).range(min, max).kind(CvarType::Float)
    };
    let entries: [(&str, f64, CvarOptions); 5] = [
        (
            "cg_ammocounter_scale",
            1.0,
            float("Ammo counter size multiplier", 0.05, 20.0),
        ),
        (
            "cg_ammocounter_yaw",
            90.0,
            float("Ammo counter yaw (deg)", -180.0, 180.0),
        ),
        (
            "cg_ammocounter_pitch",
            0.0,
            float("Ammo counter pitch (deg)", -180.0, 180.0),
        ),
        (
            "cg_ammocounter_roll",
            180.0,
            float("Ammo counter roll (deg)", -180.0, 180.0),
        ),
        (
            "cg_ammocounter_fwd",
            0.0,
            float("Ammo counter forward offset", -10.0, 10.0),
        ),
    ];
    for (name, default, options) in entries {
        if !cvars.exists(name) {
            cvars.register(name, default, options);
        }
    }
}

/// A separable Gaussian blur of a coverage mask (the canvas shadow's
/// `shadowBlur`, whose sigma is half of it).
fn blur(mask: &[f32], width: usize, height: usize, sigma: f64) -> Vec<f32> {
    let radius = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp() as f32)
        .collect();
    let total: f32 = kernel.iter().sum();
    let pass = |src: &[f32], horizontal: bool| {
        let mut out = vec![0.0f32; src.len()];
        for y in 0..height {
            for x in 0..width {
                let mut sum = 0.0;
                for (k, w) in kernel.iter().enumerate() {
                    let o = k as isize - radius;
                    let (sx, sy) = if horizontal {
                        (x as isize + o, y as isize)
                    } else {
                        (x as isize, y as isize + o)
                    };
                    if sx >= 0 && sy >= 0 && (sx as usize) < width && (sy as usize) < height {
                        sum += src[sy as usize * width + sx as usize] * w;
                    }
                }
                out[y * width + x] = sum / total;
            }
        }
        out
    };
    pass(&pass(mask, true), false)
}

/// The ten digits, 0 to 9 left to right, each painted as `draw` paints a
/// place: Orbitron filling 90% of the height, centred, near black, in a
/// yellow glow, twice over. RGBA, `10 * CELL_WIDTH` by `CELL_HEIGHT`.
#[must_use]
pub fn digit_strip(font: &Font) -> (u32, u32, Vec<u8>) {
    let (cw, ch) = (CELL_WIDTH as usize, CELL_HEIGHT as usize);
    let width = cw * 10;
    let font_px = (f64::from(CELL_HEIGHT) * 0.9).round() as f32;
    let (ascent, descent) = font.vertical(font_px);
    // `textBaseline = "middle"`: the em box's middle on the cell's.
    let baseline = ch as f32 / 2.0 + (ascent - descent.abs()) / 2.0;
    let blur_px = (f64::from(CELL_HEIGHT) * 0.09).max(3.0);
    let mut rgba = vec![0.0f32; width * ch * 4];
    for digit in 0..10u8 {
        let c = char::from(b'0' + digit);
        let glyph = font.glyph(c);
        let mut mask = vec![0.0f32; cw * ch];
        if let Some(bitmap) = font.rasterize_char(c, font_px) {
            let advance = font.advance(glyph, font_px);
            let x0 = (cw as f32 / 2.0 - advance / 2.0).round() as i32 + bitmap.left;
            let y0 = baseline.round() as i32 - bitmap.top;
            for by in 0..bitmap.height as i32 {
                for bx in 0..bitmap.width as i32 {
                    let (x, y) = (x0 + bx, y0 + by);
                    if x >= 0 && y >= 0 && (x as usize) < cw && (y as usize) < ch {
                        let coverage = bitmap.coverage[(by * bitmap.width as i32 + bx) as usize];
                        mask[y as usize * cw + x as usize] = f32::from(coverage) / 255.0;
                    }
                }
            }
        }
        let glow = blur(&mask, cw, ch, blur_px / 2.0);
        let shadow = [1.0, 205.0 / 255.0, 70.0 / 255.0];
        let ink = [12.0 / 255.0; 3];
        for y in 0..ch {
            for x in 0..cw {
                let i = y * cw + x;
                // shadow, glyph, shadow, glyph: source-over each.
                let (mut rgb, mut a) = ([0.0f32; 3], 0.0f32);
                for _ in 0..2 {
                    for (color, alpha) in [(shadow, glow[i] * 0.95), (ink, mask[i])] {
                        let out = alpha + a * (1.0 - alpha);
                        if out > 0.0 {
                            for k in 0..3 {
                                rgb[k] = (color[k] * alpha + rgb[k] * a * (1.0 - alpha)) / out;
                            }
                        }
                        a = out;
                    }
                }
                let o = (y * width + usize::from(digit) * cw + x) * 4;
                rgba[o..o + 3].copy_from_slice(&rgb);
                rgba[o + 3] = a;
            }
        }
    }
    let bytes = rgba.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    (width as u32, CELL_HEIGHT, bytes)
}

/// One place's quad: a third of `CreatePlane(width, height)` (in XY,
/// facing -Z), the left place first, with the strip cell `digit`.
fn place_quad(config: &AmmoCounterConfig, place: usize, digit: u8) -> VertexData {
    let width = config.digit_width * MAX_DIGITS as f64;
    let (h2, w3) = (config.digit_height / 2.0, width / MAX_DIGITS as f64);
    let x0 = -width / 2.0 + w3 * place as f64;
    let x1 = x0 + w3;
    let [u0, u1] = cell_us(digit);
    // Babylon's plane shows its texture's top row at its top edge.
    let corners = [
        (x0, -h2, u0, 1.0),
        (x1, -h2, u1, 1.0),
        (x1, h2, u1, 0.0),
        (x0, h2, u0, 0.0),
    ];
    let mut data = VertexData::default();
    for (x, y, u, v) in corners {
        data.positions.extend_from_slice(&[x as f32, y as f32, 0.0]);
        data.normals.extend_from_slice(&[0.0, 0.0, -1.0]);
        data.uvs.extend_from_slice(&[u, v]);
    }
    data.indices = vec![0, 1, 2, 0, 2, 3];
    data
}

/// A cell's U range in the strip.
fn cell_us(digit: u8) -> [f32; 2] {
    let w = f32::from(digit) / 10.0;
    [w, w + 0.1]
}

/// `ammoCounterMat`: self-lit, both faces, blended by the digits' alpha.
#[must_use]
pub fn ammo_counter_material() -> StandardMaterialParams {
    StandardMaterialParams {
        disable_lighting: true,
        emissive_color: Vec3::ONE,
        back_face_culling: false,
        alpha_blend: true,
        depth_always: true,
        texture: TextureUse {
            diffuse: true,
            alpha_from_diffuse: true,
            emissive: true,
            opacity_rgb: false,
        },
        ..StandardMaterialParams::default()
    }
}

/// The counter on a gun.
#[derive(Debug)]
pub struct AmmoCounterDisplay {
    config: AmmoCounterConfig,
    handle: ModelHandle,
    digits: String,
}

impl AmmoCounterDisplay {
    /// Uploads the three places into `models`, showing nothing.
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer, config: AmmoCounterConfig, font: &Font) -> Self {
        let (w, h, pixels) = digit_strip(font);
        let mut png = Cursor::new(Vec::new());
        image::write_buffer_with_format(
            &mut png,
            &pixels,
            w,
            h,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .expect("encoding a PNG in memory");
        let quads: Vec<VertexData> = (0..MAX_DIGITS).map(|i| place_quad(&config, i, 0)).collect();
        let parts: Vec<(&str, &VertexData)> = quads.iter().map(|q| ("ammoCounter", q)).collect();
        let handle = upload_textured_standard_mesh(
            gpu,
            models,
            &parts,
            ammo_counter_material(),
            Some(ModelImage {
                name: "ammoCounterTex.png".to_owned(),
                mime_type: Some("image/png".to_owned()),
                bytes: png.into_inner(),
            }),
        );
        Self {
            config,
            handle,
            digits: " ".repeat(MAX_DIGITS),
        }
    }

    /// `setAmmo`: rewrites the places that changed.
    pub fn set_ammo(&mut self, gpu: &Gpu, models: &mut ModelRenderer, ammo: i64) {
        let digits = format_ammo_digits(ammo, MAX_DIGITS);
        if digits == self.digits {
            return;
        }
        for (place, (new, old)) in digits.bytes().zip(self.digits.bytes()).enumerate() {
            if new == old || new == b' ' {
                continue;
            }
            let quad = place_quad(&self.config, place, new - b'0');
            let positions: Vec<[f32; 3]> = quad.positions.as_chunks::<3>().0.to_vec();
            let normals: Vec<[f32; 3]> = quad.normals.as_chunks::<3>().0.to_vec();
            let uvs: Vec<[f32; 2]> = quad.uvs.as_chunks::<2>().0.to_vec();
            models.update_vertices(gpu, self.handle, place, &positions, &normals, &uvs);
        }
        self.digits = digits;
    }

    /// The places' draws, under the anchor's world matrix (`tag_ammo`'s),
    /// turned and sized by the tuning; blank places draw nothing.
    pub fn draws(&self, anchor_world: Mat4, tuning: &AmmoCounterTuning, out: &mut Vec<ModelDraw>) {
        let world = anchor_world * plane_local(anchor_world, tuning);
        for (place, digit) in self.digits.bytes().enumerate() {
            if digit != b' ' {
                out.push(ModelDraw::new(self.handle, place, world));
            }
        }
    }
}

/// `applyTuning`: the plane's scaling (the anchor's mirror cancelled),
/// rotation and forward offset under its anchor. Babylon's decompose puts a
/// mirror on Y.
#[must_use]
pub fn plane_local(anchor_world: Mat4, tuning: &AmmoCounterTuning) -> Mat4 {
    let mirror_y = if anchor_world.determinant() < 0.0 { -1.0 } else { 1.0 };
    let scale = tuning.scale as f32;
    let degrees = std::f64::consts::PI / 180.0;
    let rotation: Quat = babylon_euler((DVec3::new(tuning.pitch, tuning.yaw, tuning.roll) * degrees).as_vec3());
    Mat4::from_scale_rotation_translation(
        Vec3::new(scale, mirror_y * scale, scale),
        rotation,
        Vec3::new(0.0, 0.0, tuning.fwd as f32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_cfg_line() {
        let cfg =
            "//ammoCounter font_family font_size digit_width\nammoCounter \"LCDdigits\" 46 0.18 0.34 0.75 0.22 0.3\n";
        let c = parse_ammo_counter(cfg).unwrap();
        assert_eq!(c.font_family, "LCDdigits");
        assert_eq!(c.digit_width, 0.18);
        assert_eq!(c.icon_alpha, 0.3);
        assert_eq!(parse_ammo_counter("ammoCounter x 1 2"), None);
        assert_eq!(parse_ammo_counter("tag_flash 1"), None);
        assert!(parse_ammo_counter("AMMOCOUNTER LCD 1 2 3 4 5 6 // tuned").is_some());
    }

    #[test]
    fn formats_three_places() {
        assert_eq!(format_ammo_digits(7, 3), "  7");
        assert_eq!(format_ammo_digits(42, 3), " 42");
        assert_eq!(format_ammo_digits(137, 3), "137");
        assert_eq!(format_ammo_digits(1200, 3), "999");
        assert_eq!(format_ammo_digits(-3, 3), "  0");
    }

    #[test]
    fn a_mirrored_anchor_is_cancelled_on_y() {
        let mirrored = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
        let local = plane_local(mirrored, &AmmoCounterTuning::default());
        assert!((mirrored * local).determinant() > 0.0);
        assert!(plane_local(Mat4::IDENTITY, &AmmoCounterTuning::default()).determinant() > 0.0);
    }

    #[test]
    fn places_split_the_plane() {
        let c = parse_ammo_counter("ammoCounter LCD 46 0.18 0.34 0.75 0.22 0.3").unwrap();
        let left = place_quad(&c, 0, 3);
        assert!((left.positions[0] + 0.27).abs() < 1e-6);
        assert_eq!(left.uvs[0], 0.3);
        let right = place_quad(&c, 2, 9);
        assert!((right.positions[3] - 0.27).abs() < 1e-6);
        assert!((right.uvs[2] - 1.0).abs() < 1e-6);
    }
}
