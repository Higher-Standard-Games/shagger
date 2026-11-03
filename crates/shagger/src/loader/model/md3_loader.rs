//! SHAG `src/engine/loader/model/MD3Loader.ts`: Quake 3 MD3 models, parsed
//! (`parseMD3`) and turned into Babylon-axis surface meshes
//! (`createSurfaceMesh`).
//!
//! An MD3 stores every frame's vertex positions. Quake's axes (X right, Y
//! forward, Z up) become Babylon's by swapping Y and Z, with the winding
//! reversed. Each surface's material comes from its shader name
//! (`createMaterialFromShader`), as a Babylon `StandardMaterial` with the
//! stage animation `ShaderAnimationSystem` would run on it.
//!
//! Not ported: alpha test, glow textures, `animMap` frames, distance ramps
//! and soft particles, and the autosprite turn (none of the projectiles use
//! them).

use std::fmt;

use glam::{Mat3, Vec3};

use crate::babylon::materials::standard_material::{StandardAlphaMode, StandardMaterialParams, TextureUse};
use crate::babylon::materials::texture::{TextureMatrix, get_texture_matrix};
use crate::loader::pk3::pk3_loader::Pk3Loader;
use crate::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use crate::loader::shader::q3_shader_types::{
    AlphaGen, AlphaGenType, AlphaMode, BlendFactor, ResolveMaterialOptions, RgbGen, RgbGenType, TcMod,
};
use crate::loader::shader::q3_stage_evaluation::{create_tc_state, evaluate_wave_func, step_tc_mods_into};

/// `MD3_IDENT`: "IDP3".
const MD3_IDENT: i32 = 0x3350_4449;
const MD3_VERSION: i32 = 15;
const MD3_XYZ_SCALE: f32 = 1.0 / 64.0;
const MAX_QPATH: usize = 64;

/// Why an MD3 did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Md3Error(pub String);

impl fmt::Display for Md3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Md3Error {}

/// `MD3Header`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Md3Header {
    pub ident: i32,
    pub version: i32,
    pub name: String,
    pub flags: i32,
    pub num_frames: i32,
    pub num_tags: i32,
    pub num_surfaces: i32,
    pub num_skins: i32,
    pub ofs_frames: i32,
    pub ofs_tags: i32,
    pub ofs_surfaces: i32,
    pub ofs_end: i32,
}

/// `MD3Frame`: a frame's bounds, in Quake axes.
#[derive(Debug, Clone, PartialEq)]
pub struct Md3Frame {
    pub min_bounds: Vec3,
    pub max_bounds: Vec3,
    pub local_origin: Vec3,
    pub radius: f32,
    pub name: String,
}

/// `MD3Tag`: an attachment point, in Babylon axes.
#[derive(Debug, Clone, PartialEq)]
pub struct Md3Tag {
    pub name: String,
    pub origin: Vec3,
    /// Columns: Babylon right (Quake left, negated), up, forward.
    pub axis: Mat3,
}

/// `MD3SurfaceHeader`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Md3SurfaceHeader {
    pub ident: i32,
    pub name: String,
    pub flags: i32,
    pub num_frames: i32,
    pub num_shaders: i32,
    pub num_verts: i32,
    pub num_triangles: i32,
    pub ofs_triangles: i32,
    pub ofs_shaders: i32,
    pub ofs_st: i32,
    pub ofs_xyz_normal: i32,
    pub ofs_end: i32,
}

/// `MD3Shader`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Md3Shader {
    pub name: String,
    pub shader_index: i32,
}

/// `MD3Surface`: Quake axes, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Md3Surface {
    pub header: Md3SurfaceHeader,
    pub shaders: Vec<Md3Shader>,
    pub triangles: Vec<[i32; 3]>,
    /// `[u, v]` per vertex, the same for every frame.
    pub tex_coords: Vec<[f32; 2]>,
    /// Per frame, per vertex.
    pub vertices: Vec<Vec<[f32; 3]>>,
    pub normals: Vec<Vec<[f32; 3]>>,
}

/// `MD3Model`.
#[derive(Debug, Clone, PartialEq)]
pub struct Md3Model {
    pub header: Md3Header,
    pub frames: Vec<Md3Frame>,
    /// `[frame][tag]`.
    pub tags: Vec<Vec<Md3Tag>>,
    pub surfaces: Vec<Md3Surface>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N], Md3Error> {
        self.0
            .get(at..at + N)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| Md3Error(format!("MD3 read past the end at {at}")))
    }
    fn i32(&self, at: usize) -> Result<i32, Md3Error> {
        Ok(i32::from_le_bytes(self.bytes(at)?))
    }
    fn i16(&self, at: usize) -> Result<i16, Md3Error> {
        Ok(i16::from_le_bytes(self.bytes(at)?))
    }
    fn f32(&self, at: usize) -> Result<f32, Md3Error> {
        Ok(f32::from_le_bytes(self.bytes(at)?))
    }
    fn vec3(&self, at: usize) -> Result<Vec3, Md3Error> {
        Ok(Vec3::new(self.f32(at)?, self.f32(at + 4)?, self.f32(at + 8)?))
    }
    /// `readString`: bytes up to the first zero, as Latin-1.
    fn string(&self, at: usize, max: usize) -> Result<String, Md3Error> {
        let mut out = String::new();
        for i in 0..max {
            let [c] = self.bytes::<1>(at + i)?;
            if c == 0 {
                break;
            }
            out.push(char::from(c));
        }
        Ok(out)
    }
}

fn offset(base: usize, delta: i32) -> Result<usize, Md3Error> {
    usize::try_from(i64::try_from(base).unwrap_or(i64::MAX) + i64::from(delta))
        .map_err(|_| Md3Error(format!("MD3 offset {delta} before the start")))
}

fn count(n: i32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// `parseMD3`.
pub fn parse_md3(bytes: &[u8]) -> Result<Md3Model, Md3Error> {
    let r = Reader(bytes);
    let header = Md3Header {
        ident: r.i32(0)?,
        version: r.i32(4)?,
        name: r.string(8, MAX_QPATH)?,
        flags: r.i32(72)?,
        num_frames: r.i32(76)?,
        num_tags: r.i32(80)?,
        num_surfaces: r.i32(84)?,
        num_skins: r.i32(88)?,
        ofs_frames: r.i32(92)?,
        ofs_tags: r.i32(96)?,
        ofs_surfaces: r.i32(100)?,
        ofs_end: r.i32(104)?,
    };
    if header.ident != MD3_IDENT {
        return Err(Md3Error(format!(
            "Invalid MD3 file: bad magic number (got {:x})",
            header.ident
        )));
    }
    if header.version != MD3_VERSION {
        eprintln!("MD3 version mismatch: expected {MD3_VERSION}, got {}", header.version);
    }
    let mut frames = Vec::new();
    let mut at = offset(0, header.ofs_frames)?;
    for _ in 0..count(header.num_frames) {
        frames.push(Md3Frame {
            min_bounds: r.vec3(at)?,
            max_bounds: r.vec3(at + 12)?,
            local_origin: r.vec3(at + 24)?,
            radius: r.f32(at + 36)?,
            name: r.string(at + 40, 16)?,
        });
        at += 56;
    }
    let mut tags = Vec::new();
    at = offset(0, header.ofs_tags)?;
    for _ in 0..count(header.num_frames) {
        let mut frame_tags = Vec::new();
        for _ in 0..count(header.num_tags) {
            frame_tags.push(read_tag(&r, at)?);
            at += 112;
        }
        tags.push(frame_tags);
    }
    let mut surfaces = Vec::new();
    at = offset(0, header.ofs_surfaces)?;
    for _ in 0..count(header.num_surfaces) {
        let surface = read_surface(&r, at, count(header.num_frames))?;
        // Each surface ends past its own header: an end of 0 would read the
        // same one two billion times over (fuzzing).
        if surface.header.ofs_end < 108 {
            return Err(Md3Error(format!("MD3 surface ends at {}", surface.header.ofs_end)));
        }
        at = offset(at, surface.header.ofs_end)?;
        surfaces.push(surface);
    }
    Ok(Md3Model {
        header,
        frames,
        tags,
        surfaces,
    })
}

/// `readTag`: the origin and axes turned to Babylon's.
fn read_tag(r: &Reader<'_>, at: usize) -> Result<Md3Tag, Md3Error> {
    let name = r.string(at, MAX_QPATH)?;
    let q = r.vec3(at + 64)?;
    let mut axes = [Vec3::ZERO; 3];
    for (col, axis) in axes.iter_mut().enumerate() {
        let v = r.vec3(at + 76 + col * 12)?;
        // Babylon axes: swap Y and Z.
        *axis = Vec3::new(v.x, v.z, v.y);
    }
    Ok(Md3Tag {
        name,
        origin: Vec3::new(q.x, q.z, q.y),
        axis: Mat3::from_cols(-axes[1], axes[2], axes[0]),
    })
}

fn read_surface(r: &Reader<'_>, start: usize, num_frames: usize) -> Result<Md3Surface, Md3Error> {
    let header = Md3SurfaceHeader {
        ident: r.i32(start)?,
        name: r.string(start + 4, MAX_QPATH)?,
        flags: r.i32(start + 68)?,
        num_frames: r.i32(start + 72)?,
        num_shaders: r.i32(start + 76)?,
        num_verts: r.i32(start + 80)?,
        num_triangles: r.i32(start + 84)?,
        ofs_triangles: r.i32(start + 88)?,
        ofs_shaders: r.i32(start + 92)?,
        ofs_st: r.i32(start + 96)?,
        ofs_xyz_normal: r.i32(start + 100)?,
        ofs_end: r.i32(start + 104)?,
    };
    let mut shaders = Vec::new();
    let mut at = offset(start, header.ofs_shaders)?;
    for _ in 0..count(header.num_shaders) {
        shaders.push(Md3Shader {
            name: r.string(at, MAX_QPATH)?,
            shader_index: r.i32(at + 64)?,
        });
        at += 68;
    }
    let mut triangles = Vec::new();
    at = offset(start, header.ofs_triangles)?;
    for _ in 0..count(header.num_triangles) {
        triangles.push([r.i32(at)?, r.i32(at + 4)?, r.i32(at + 8)?]);
        at += 12;
    }
    let verts = count(header.num_verts);
    // The coordinates and every frame's vertices must lie in the file
    // before anything is allocated for them: a damaged count asked for
    // 14 GB (fuzzing). The reads below would fail on the same files.
    let fits = |ofs: i32, size: Option<usize>| -> Result<(), Md3Error> {
        if size == Some(0) {
            return Ok(());
        }
        offset(start, ofs)?
            .checked_add(size.ok_or_else(|| Md3Error("MD3 surface too large".into()))?)
            .filter(|&end| end <= r.0.len())
            .map(|_| ())
            .ok_or_else(|| Md3Error("MD3 surface runs past the end".into()))
    };
    fits(header.ofs_st, verts.checked_mul(8))?;
    fits(
        header.ofs_xyz_normal,
        verts.checked_mul(8).and_then(|v| v.checked_mul(num_frames)),
    )?;
    let mut tex_coords = Vec::with_capacity(verts);
    at = offset(start, header.ofs_st)?;
    for _ in 0..verts {
        tex_coords.push([r.f32(at)?, r.f32(at + 4)?]);
        at += 8;
    }
    let mut vertices = Vec::with_capacity(num_frames);
    let mut normals = Vec::with_capacity(num_frames);
    for f in 0..num_frames {
        at = offset(start, header.ofs_xyz_normal)? + f * verts * 8;
        let mut frame_verts = Vec::with_capacity(verts);
        let mut frame_normals = Vec::with_capacity(verts);
        for _ in 0..verts {
            frame_verts.push([
                f32::from(r.i16(at)?) * MD3_XYZ_SCALE,
                f32::from(r.i16(at + 2)?) * MD3_XYZ_SCALE,
                f32::from(r.i16(at + 4)?) * MD3_XYZ_SCALE,
            ]);
            frame_normals.push(decode_normal(r.i16(at + 6)?));
            at += 8;
        }
        vertices.push(frame_verts);
        normals.push(frame_normals);
    }
    Ok(Md3Surface {
        header,
        shaders,
        triangles,
        tex_coords,
        vertices,
        normals,
    })
}

/// `decodeNormal`: latitude and longitude bytes.
fn decode_normal(encoded: i16) -> [f32; 3] {
    let encoded = i32::from(encoded);
    let lat = f64::from((encoded >> 8) & 0xff) * (2.0 * std::f64::consts::PI) / 255.0;
    let lng = f64::from(encoded & 0xff) * (2.0 * std::f64::consts::PI) / 255.0;
    [
        (lat.cos() * lng.sin()) as f32,
        (lat.sin() * lng.sin()) as f32,
        lng.cos() as f32,
    ]
}

/// One surface's mesh at a frame, in Babylon axes (`createSurfaceMesh`).
#[derive(Debug, Clone, PartialEq)]
pub struct Md3SurfaceMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Babylon's winding: reversed after the axis swap.
    pub indices: Vec<u32>,
}

/// Whether `createMeshes` leaves a surface out: `_fx` effect surfaces.
#[must_use]
pub fn is_effect_surface(surface: &Md3Surface) -> bool {
    let name = surface.header.name.to_lowercase();
    name.ends_with("_fx") || name.contains("_fx_")
}

/// `createSurfaceMesh`: positions and normals with Y and Z swapped, the
/// triangles reversed. `name` is the model's name.
#[must_use]
pub fn create_surface_mesh(surface: &Md3Surface, name: &str, frame: usize) -> Md3SurfaceMesh {
    let swap = |v: &[f32; 3]| [v[0], v[2], v[1]];
    Md3SurfaceMesh {
        name: format!("{name}_{}", surface.header.name),
        positions: surface
            .vertices
            .get(frame)
            .map_or_else(Vec::new, |f| f.iter().map(swap).collect()),
        normals: surface
            .normals
            .get(frame)
            .map_or_else(Vec::new, |f| f.iter().map(swap).collect()),
        uvs: surface.tex_coords.clone(),
        indices: surface
            .triangles
            .iter()
            .flat_map(|t| [t[2] as u32, t[1] as u32, t[0] as u32])
            .collect(),
    }
}

/// The animation `ShaderAnimationSystem.registerMaterial` runs on a
/// surface's material: its primary stage's `tcMod`s, `rgbGen` and
/// `alphaGen` waves.
#[derive(Debug, Clone, PartialEq)]
pub struct Md3StageAnimation {
    pub tc_mods: Vec<TcMod>,
    pub rgb_gen: Option<RgbGen>,
    pub alpha_gen: Option<AlphaGen>,
    /// The stage colours the emissive texture (additive and fullbright
    /// surfaces), not the diffuse colour.
    pub emissive_target: bool,
    /// The colour a wave scales (`baseColor`).
    pub base_color: Vec3,
}

/// What an animated stage shows at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Md3StageFrame {
    pub texture_matrix: TextureMatrix,
    /// An emissive stage's wave: the texture's `level`.
    pub texture_level: Option<f32>,
    /// A diffuse stage's wave: the diffuse colour.
    pub diffuse_color: Option<Vec3>,
    /// An `alphaGen` wave: the material's alpha.
    pub alpha: Option<f32>,
}

impl Md3StageAnimation {
    /// The stage `seconds` into the animation clock (`totalTime`). The
    /// texture state is stepped once over the whole time, which is where
    /// SHAG's per-frame steps add up to.
    #[must_use]
    pub fn at(&self, seconds: f64) -> Md3StageFrame {
        let mut no_noise = || 0.5;
        let mut state = create_tc_state(&self.tc_mods);
        if !self.tc_mods.is_empty() {
            step_tc_mods_into(&mut state, &self.tc_mods, seconds, seconds, &mut no_noise);
        }
        let texture_matrix = if self.tc_mods.is_empty() {
            TextureMatrix::IDENTITY
        } else {
            get_texture_matrix(
                state.u_offset,
                state.v_offset,
                state.u_scale,
                state.v_scale,
                state.rotation,
            )
        };
        let mut frame = Md3StageFrame {
            texture_matrix,
            texture_level: None,
            diffuse_color: None,
            alpha: None,
        };
        if let Some(RgbGen {
            kind: RgbGenType::Wave,
            wave: Some(wave),
            ..
        }) = &self.rgb_gen
        {
            let value = evaluate_wave_func(wave, seconds, &mut no_noise).clamp(0.0, 1.0) as f32;
            if self.emissive_target {
                frame.texture_level = Some(value);
            } else {
                frame.diffuse_color = Some(self.base_color * value);
            }
        }
        if let Some(AlphaGen {
            kind: AlphaGenType::Wave,
            wave: Some(wave),
            ..
        }) = &self.alpha_gen
        {
            frame.alpha = Some(evaluate_wave_func(wave, seconds, &mut no_noise).clamp(0.0, 1.0) as f32);
        }
        frame
    }
}

/// A surface's material (`createMaterialFromShader`).
#[derive(Debug, Clone, PartialEq)]
pub struct Md3Material {
    pub params: StandardMaterialParams,
    /// The image to load (`resolveAndLoadTexture`): the shader's texture,
    /// else the shader name itself.
    pub texture_path: String,
    /// Set once the texture is there; `None` when nothing animates.
    pub animation: Option<Md3StageAnimation>,
}

/// `createMaterialFromShader`, for a texture that has loaded: the shader's
/// blending and lighting, and the stage animation it registers.
#[must_use]
pub fn create_material_from_shader(registry: &Q3ShaderRegistry, pk3: &Pk3Loader, shader_name: &str) -> Md3Material {
    let normalized = shader_name.replace('\\', "/");
    let props = registry.resolve_material_props(pk3, &normalized, &ResolveMaterialOptions::default());
    let texture_path = props.diffuse_texture.clone().unwrap_or_else(|| normalized.clone());
    let additive = props.alpha_mode == AlphaMode::Add;
    let emissive_target = additive || props.is_fullbright;
    let mut mat = StandardMaterialParams {
        texture: TextureUse {
            diffuse: true,
            alpha_from_diffuse: props.is_transparent,
            emissive: emissive_target,
            opacity_rgb: false,
        },
        ..StandardMaterialParams::default()
    };
    // configureQ3Alpha (alpha test is not drawn yet).
    let blend = matches!(
        props.alpha_mode,
        AlphaMode::Blend | AlphaMode::Add | AlphaMode::Multiply
    );
    mat.alpha_blend = blend;
    mat.alpha_mode = match props.alpha_mode {
        AlphaMode::Add if props.blend_func.is_some_and(|f| f[0] == BlendFactor::SrcAlpha) => StandardAlphaMode::Add,
        AlphaMode::Add => StandardAlphaMode::OneOne,
        _ => StandardAlphaMode::Combine,
    };
    if props.alpha_cutoff.is_some() || blend {
        mat.texture.alpha_from_diffuse = true;
    }
    mat.disable_depth_write = !props.depth_write;
    if props.is_transparent {
        mat.alpha = 1.0;
        if additive {
            mat.emissive_color = Vec3::ZERO;
            mat.emissive_as_illumination = true;
            mat.disable_lighting = true;
        }
    }
    if props.is_fullbright {
        mat.emissive_color = Vec3::ZERO;
        mat.emissive_as_illumination = true;
        mat.disable_lighting = true;
    }
    mat.back_face_culling = props.back_face_culling;
    mat.specular_color = Vec3::splat(0.1);
    let animation = props.primary_stage.as_ref().and_then(|stage| {
        let tc_mods = stage.tc_mod.clone().unwrap_or_default();
        let waves = |rgb: bool| {
            if rgb {
                stage.rgb_gen.as_ref().is_some_and(|g| g.kind == RgbGenType::Wave)
            } else {
                stage.alpha_gen.as_ref().is_some_and(|g| g.kind == AlphaGenType::Wave)
            }
        };
        if tc_mods.is_empty() && !waves(true) && !waves(false) {
            return None;
        }
        let base_color = if emissive_target {
            mat.emissive_color
        } else {
            mat.diffuse_color
        };
        if emissive_target {
            mat.emissive_color = Vec3::ZERO;
            mat.emissive_as_illumination = true;
        }
        Some(Md3StageAnimation {
            tc_mods,
            rgb_gen: stage.rgb_gen,
            alpha_gen: stage.alpha_gen,
            emissive_target,
            base_color,
        })
    });
    Md3Material {
        params: mat,
        texture_path,
        animation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-frame, one-tag, one-triangle model.
    fn tiny() -> Vec<u8> {
        let mut b = vec![0u8; 108];
        let put_i32 = |b: &mut Vec<u8>, at: usize, v: i32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put_i32(&mut b, 0, MD3_IDENT);
        put_i32(&mut b, 4, 15);
        b[8..12].copy_from_slice(b"test");
        put_i32(&mut b, 76, 1); // frames
        put_i32(&mut b, 80, 1); // tags
        put_i32(&mut b, 84, 1); // surfaces
        put_i32(&mut b, 92, 108);
        put_i32(&mut b, 96, 108 + 56);
        put_i32(&mut b, 100, 108 + 56 + 112);
        b.resize(108 + 56 + 112, 0);
        // Tag: origin (1, 2, 3), identity axes.
        let tag = 108 + 56;
        b[tag..tag + 7].copy_from_slice(b"tag_one");
        for (i, v) in [1.0f32, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .iter()
            .enumerate()
        {
            b[tag + 64 + i * 4..tag + 68 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        // Surface: header 108, one shader (68), one triangle (12), three
        // texcoords (24), three vertices (24).
        let s = b.len();
        b.resize(s + 108 + 68 + 12 + 24 + 24, 0);
        put_i32(&mut b, s, MD3_IDENT);
        b[s + 4..s + 8].copy_from_slice(b"body");
        put_i32(&mut b, s + 72, 1);
        put_i32(&mut b, s + 76, 1);
        put_i32(&mut b, s + 80, 3);
        put_i32(&mut b, s + 84, 1);
        put_i32(&mut b, s + 88, 108 + 68);
        put_i32(&mut b, s + 92, 108);
        put_i32(&mut b, s + 96, 108 + 68 + 12);
        put_i32(&mut b, s + 100, 108 + 68 + 12 + 24);
        put_i32(&mut b, s + 104, 108 + 68 + 12 + 24 + 24);
        b[s + 108..s + 108 + 9].copy_from_slice(b"skin/body");
        let tri = s + 108 + 68;
        for (i, v) in [0, 1, 2].iter().enumerate() {
            put_i32(&mut b, tri + i * 4, *v);
        }
        let xyz = s + 108 + 68 + 12 + 24;
        // Vertex 1 at Quake (64, 128, 192) / 64 = (1, 2, 3); normal lat 0, lng 0 (+Z).
        for (i, v) in [64i16, 128, 192, 0].iter().enumerate() {
            b[xyz + 8 + i * 2..xyz + 10 + i * 2].copy_from_slice(&v.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_and_turns_to_babylon_axes() {
        let model = parse_md3(&tiny()).unwrap();
        assert_eq!(model.header.name, "test");
        assert_eq!(model.tags[0][0].name, "tag_one");
        assert_eq!(model.tags[0][0].origin, Vec3::new(1.0, 3.0, 2.0));
        // Quake forward (X col) is Babylon's forward column.
        assert_eq!(model.tags[0][0].axis.z_axis, Vec3::new(1.0, 0.0, 0.0));
        let surface = &model.surfaces[0];
        assert_eq!(surface.shaders[0].name, "skin/body");
        let mesh = create_surface_mesh(surface, "test", 0);
        assert_eq!(mesh.name, "test_body");
        assert_eq!(mesh.positions[1], [1.0, 3.0, 2.0]);
        // Normal +Z in Quake is +Y in Babylon.
        assert!((mesh.normals[1][1] - 1.0).abs() < 1e-6);
        assert_eq!(mesh.indices, [2, 1, 0]);
        assert!(parse_md3(b"nope").is_err());
    }

    /// Fuzzing: a vertex count of two billion is refused before 14 GB is
    /// asked for.
    #[test]
    fn a_vertex_count_past_the_end_is_refused() {
        let mut b = tiny();
        let s = 108 + 56 + 112;
        b[s + 80..s + 84].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(parse_md3(&b).is_err());
        // Nor a surface that ends where it starts, counted two billion times.
        let mut b = tiny();
        b[84..88].copy_from_slice(&i32::MAX.to_le_bytes());
        b[s + 104..s + 108].copy_from_slice(&0i32.to_le_bytes());
        assert!(parse_md3(&b).is_err());
    }
}
