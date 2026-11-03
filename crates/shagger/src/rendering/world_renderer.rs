//! Draws [`WorldGeometry`] with wgpu: one vertex and index buffer for the map,
//! the lightmap pages as one array texture (and the deluxe pages as another),
//! and per batch its material passes, each with its base image and the
//! blend, alpha test, cull and depth state its shader gives
//! ([`super::world_materials`]). Opaque passes draw first, then blended ones
//! in batch and pass order (Babylon's transparent queue, unsorted).
//!
//! A pass whose primary stage animates (`tcMod`, `rgbGen`/`alphaGen` waves,
//! `animMap`) is stepped by [`WorldRenderer::animate`] as SHAG's
//! `ShaderAnimationSystem` steps it ([`q3_stage_evaluation`]), its UVs moved
//! by Babylon's texture matrix. `rgbGen const`, `alphaGen const` and
//! distance ramps (`Q3StageColorPlugin`, `Q3DistanceRampPlugin`) apply to
//! every pass.
//!
//! A lightmapped pass finds its companion images as `loadTextures` does:
//! a normal map (read through [`build_bsp_tangents`](crate::loader::bsp::mesh::tangents::build_bsp_tangents)'s
//! frame) and a gloss map, which with deluxe pages adds Qfusion's baked
//! gloss; an opaque pass with both draws as SHAG's generated PBR material.
//! A glow image adds as the emissive, or, with an animated glow stage,
//! draws as an additive shine pass after it (`registerGlowAnimation`).
//!
//! Not yet drawn as SHAG draws them: environment and `$cubemap` reflections
//! (a reflection pass is left out, an environment material draws its base
//! image), and the sky.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::dynamic_lights::{DynamicLight, DynamicLightBlock};
use super::gpu::Gpu;
use super::world_fog_controller::FogUniforms;
use super::world_geometry::{WorldGeometry, WorldVertex};
use super::world_materials::{
    Blend, GLOSS_SUFFIXES, MissingImage, NORMAL_SUFFIXES, PassState, add_texture_prefixes, build_diffuse_candidates,
    gloss_exponent, gloss_intensity, missing_image, optional_texture_candidates, pbr_micro_surface,
    should_use_generated_pbr,
};
use crate::babylon::materials::texture::get_texture_matrix;
use crate::loader::bsp::bsp_lighting_profile::bsp_lighting_profile;
use crate::loader::bsp::bsp_loader::BspFile;
use crate::loader::bsp::mesh::lightmaps::bsp_lightmap_array_rgba;
use crate::loader::shader::q3_shader_types::{
    AlphaGenType, DistanceRamp, Q3ShaderStage, RgbGen, RgbGenType, TcMod, WaveFunc,
};
use crate::loader::shader::q3_stage_evaluation::{TcState, create_tc_state, evaluate_wave_func, step_tc_mods_into};
use crate::loader::texture::texture_extensions::strip_texture_extension;

/// The depth buffer format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    lighting: [f32; 4],
    ambient: [f32; 4],
    /// The camera position (distance ramps).
    eye: [f32; 4],
    /// World fog (`AtmosphericFogPlugin`).
    fog: FogUniforms,
    dynamic: DynamicLightBlock,
}

/// One pass's material uniform.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    /// x: alpha cutoff (negative: no test), y: 1 to take alpha from the
    /// image, z: lighting (0 baked, 1 full bright, 2 vertex colour),
    /// w: 1 to multiply alpha by the vertex alpha.
    params: [f32; 4],
    /// The texture matrix: U row (xy) and V row (zw).
    uv_rows: [f32; 4],
    /// The texture matrix origin (xy).
    uv_origin: [f32; 4],
    /// Colour and alpha multipliers (`rgbGen`/`alphaGen` const and waves).
    color: [f32; 4],
    /// Distance ramps (from, to, near, far); from = to = 1 when absent.
    ramp_rgb: [f32; 4],
    ramp_alpha: [f32; 4],
    /// Companion surfaces (`tuneCompanionSurfaceMaterial`): x 1 with a
    /// normal map, y 1 with a gloss map, z 1 for the generated PBR material.
    surface: [f32; 4],
    /// x: baked gloss strength (`glossIntensity`), y: gloss exponent,
    /// z: PBR roughness (`1 - microSurface`), w: PBR reflectivity colour.
    gloss: [f32; 4],
    /// x: how world fog meets the pass (`fogPassKind`: 0 mix, 1 additive,
    /// 2 filter).
    fog: [f32; 4],
}

impl MaterialUniform {
    fn set_tc_state(&mut self, state: &TcState) {
        let m = get_texture_matrix(
            state.u_offset,
            state.v_offset,
            state.u_scale,
            state.v_scale,
            state.rotation,
        );
        self.uv_rows = [
            m.u_row[0] as f32,
            m.u_row[1] as f32,
            m.v_row[0] as f32,
            m.v_row[1] as f32,
        ];
        self.uv_origin = [m.origin[0] as f32, m.origin[1] as f32, 0.0, 0.0];
    }
}

/// `fogPassKind`: how a pass's blend meets world fog (0 mix, 1 additive,
/// 2 filter).
fn fog_pass_kind(blend: Blend) -> f32 {
    match blend {
        Blend::Opaque | Blend::Combine => 0.0,
        Blend::OneOne | Blend::Add => 1.0,
        Blend::Multiply => 2.0,
    }
}

fn ramp(ramp: Option<DistanceRamp>) -> [f32; 4] {
    ramp.map_or([1.0, 1.0, 0.0, 1.0], |r| {
        [r.from as f32, r.to as f32, r.near as f32, r.far as f32]
    })
}

/// What `ShaderAnimationSystem.registerMaterial` steps for a pass.
struct PassAnimation {
    pass: usize,
    tc_mods: Vec<TcMod>,
    tc_state: TcState,
    rgb_wave: Option<WaveFunc>,
    alpha_wave: Option<WaveFunc>,
    /// `animMap`: the frames' bind groups and their rate.
    frames: Vec<usize>,
    frequency: f64,
    /// The colour the waves scale (`baseColor`, with any `rgbGen const`).
    base_color: [f32; 4],
}

/// A decoded RGBA8 image.
#[derive(Debug, Clone)]
pub struct Rgba8Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba8Image {
    /// One white pixel: what a surface whose texture is missing draws with.
    #[must_use]
    pub fn white() -> Self {
        Self::solid([255, 255, 255, 255])
    }

    /// One pixel of a colour.
    #[must_use]
    pub fn solid(rgba: [u8; 4]) -> Self {
        Self {
            width: 1,
            height: 1,
            pixels: rgba.to_vec(),
        }
    }

    /// `createRadialGradientTexture`: white at the centre fading to clear
    /// at the edge, for additive halos with no image.
    #[must_use]
    pub fn radial_gradient() -> Self {
        const SIZE: u32 = 64;
        let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
        let half = SIZE as f32 / 2.0;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = ((x as f32 + 0.5 - half).hypot(y as f32 + 0.5 - half) / half).min(1.0);
                let v = ((1.0 - d) * 255.0) as u8;
                pixels.extend_from_slice(&[v, v, v, v]);
            }
        }
        Self {
            width: SIZE,
            height: SIZE,
            pixels,
        }
    }
}

/// The fixed-function state a pipeline is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    two_sided: bool,
    blend: Blend,
    depth_write: bool,
    polygon_offset: bool,
}

/// One draw a material pass makes: the surface itself, then any shine
/// overlay.
struct Layer<'a> {
    base_view: String,
    frame_views: Vec<String>,
    /// The stage the animation system steps, if any.
    stage: Option<&'a Q3ShaderStage>,
    params: [f32; 4],
    rgb_gen: Option<RgbGen>,
    alpha: f32,
    ramps: ([f32; 4], [f32; 4]),
    surface: [f32; 4],
    gloss: [f32; 4],
    key: PipelineKey,
}

struct DrawPass {
    pipeline: usize,
    bind_group: usize,
    uniform: MaterialUniform,
    buffer: wgpu::Buffer,
    first_index: u32,
    index_count: u32,
    /// The batch it draws (an index into `WorldGeometry::batches`).
    batch: usize,
}

pub struct WorldRenderer {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_bind_groups: Vec<wgpu::BindGroup>,
    texture_count: usize,
    missing: Vec<String>,
    passes: Vec<DrawPass>,
    /// The opaque passes lead `passes`; this many of them.
    opaque_count: usize,
    animations: Vec<PassAnimation>,
    /// `ShaderAnimationSystem.totalTime`.
    total_time: f64,
    /// `Math.random` for `noise` waves (xorshift).
    noise_seed: u64,
    pipelines: Vec<wgpu::RenderPipeline>,
    lighting: [f32; 4],
    ambient: [f32; 4],
    dynamic: DynamicLightBlock,
    /// World fog, from the next `prepare`.
    fog: FogUniforms,
}

fn mip_chain(image: &Rgba8Image) -> Vec<Rgba8Image> {
    let mut levels = vec![image.clone()];
    let mut current = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
        .expect("the pixel count matches the size");
    while current.width() > 1 || current.height() > 1 {
        let (w, h) = ((current.width() / 2).max(1), (current.height() / 2).max(1));
        current = image::imageops::resize(&current, w, h, image::imageops::FilterType::Triangle);
        levels.push(Rgba8Image {
            width: w,
            height: h,
            pixels: current.as_raw().clone(),
        });
    }
    levels
}

pub(crate) fn upload_texture(gpu: &Gpu, label: &str, image: &Rgba8Image) -> wgpu::Texture {
    let levels = mip_chain(image);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Babylon samples and writes these bytes as they are (no sRGB decode).
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, data) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * data.width),
                rows_per_image: Some(data.height),
            },
            wgpu::Extent3d {
                width: data.width,
                height: data.height,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

fn upload_layers(gpu: &Gpu, label: &str, size: u32, layers: u32, rgba: &[u8]) -> wgpu::Texture {
    gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba,
    )
}

fn array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

impl WorldRenderer {
    /// Uploads the map. `textures` loads an image by base path (as
    /// `resolveAndLoadTexture` does, extensions its own business); each
    /// pass's candidates are tried in order. `color_format` is the target's
    /// format.
    pub fn new(
        gpu: &Gpu,
        bsp: &BspFile,
        geometry: &WorldGeometry,
        mut textures: impl FnMut(&str) -> Option<Rgba8Image>,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        let device = &gpu.device;
        let profile = bsp_lighting_profile(bsp);

        // Lightmap pages as layers; deluxe pages, when present, as a second array.
        let size = bsp.lightmap_size;
        let stride = geometry.lightmap_stride;
        let raw = bsp.common.header.is_fbsp;
        let pages: Vec<&[u8]> = bsp.lightmaps.iter().step_by(stride).map(Vec::as_slice).collect();
        let white_page = vec![255u8; (size * size * 3) as usize];
        let pages = if pages.is_empty() {
            vec![white_page.as_slice()]
        } else {
            pages
        };
        let lightmap = upload_layers(
            gpu,
            "lightmap_layers",
            size,
            pages.len() as u32,
            &bsp_lightmap_array_rgba(&pages, size as usize, raw, false),
        );
        let directions: Vec<&[u8]> = if stride == 2 {
            bsp.lightmaps.iter().skip(1).step_by(2).map(Vec::as_slice).collect()
        } else {
            vec![white_page.as_slice()]
        };
        let direction = upload_layers(
            gpu,
            "lightmap_direction_layers",
            size,
            directions.len() as u32,
            &bsp_lightmap_array_rgba(&directions, size as usize, true, true),
        );

        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let base_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("base"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, wgpu::TextureViewDimension::D2Array),
                texture_entry(2, wgpu::TextureViewDimension::D2Array),
                sampler_entry(3),
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world base texture"),
            entries: &[
                texture_entry(0, wgpu::TextureViewDimension::D2),
                sampler_entry(1),
                texture_entry(3, wgpu::TextureViewDimension::D2),
                texture_entry(4, wgpu::TextureViewDimension::D2),
                texture_entry(5, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&array_view(&lightmap)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&array_view(&direction)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
            ],
        });

        // Images by candidate list, shared between passes; a bind group per
        // pass. Surfaces without companions read a flat normal and no gloss.
        let mut views: HashMap<String, wgpu::TextureView> = HashMap::new();
        let mut found: HashMap<String, String> = HashMap::new();
        const FLAT_NORMAL: &str = "@flat_normal";
        const NO_GLOSS: &str = "@no_gloss";
        for (key, rgba) in [(FLAT_NORMAL, [128, 128, 255, 255]), (NO_GLOSS, [0, 0, 0, 255])] {
            let view =
                upload_texture(gpu, key, &Rgba8Image::solid(rgba)).create_view(&wgpu::TextureViewDescriptor::default());
            views.insert(key.to_owned(), view);
        }
        let mut texture_bind_groups = Vec::new();
        let mut keys: Vec<PipelineKey> = Vec::new();
        let mut missing = Vec::new();
        let mut opaque = Vec::new();
        let mut blended = Vec::new();
        for (batch_index, batch) in geometry.batches.iter().enumerate() {
            for props in &batch.passes {
                // Only the reflection captured at load draws here.
                if props.generated_cubemap == Some(true) {
                    continue;
                }
                let state = PassState::of(props);
                let candidates = build_diffuse_candidates(&batch.texture_name, props);
                let fallback = missing_image(props);
                // The view's key and the base path of the image found (none for a fallback).
                let mut view_for =
                    |candidates: &[String], fallback: Option<MissingImage>| -> Option<(String, Option<String>)> {
                        let key = format!("{}|{fallback:?}", candidates.join("|").to_lowercase());
                        if !views.contains_key(&key) {
                            let hit = candidates.iter().find_map(|c| textures(c).map(|image| (c, image)));
                            if let Some((candidate, _)) = &hit {
                                found.insert(key.clone(), strip_texture_extension(candidate).to_owned());
                            }
                            let image = match hit {
                                Some((_, image)) => image,
                                None => match fallback? {
                                    MissingImage::RadialGradient => Rgba8Image::radial_gradient(),
                                    MissingImage::Black => Rgba8Image::solid([0, 0, 0, 255]),
                                    MissingImage::White => Rgba8Image::white(),
                                    MissingImage::Untextured => {
                                        missing.push(batch.texture_name.clone());
                                        Rgba8Image::white()
                                    }
                                },
                            };
                            let view = upload_texture(gpu, &batch.texture_name, &image)
                                .create_view(&wgpu::TextureViewDescriptor::default());
                            views.insert(key.clone(), view);
                        }
                        let path = found.get(&key).cloned();
                        Some((key, path))
                    };
                let (base_view, base_path) =
                    view_for(&candidates, Some(fallback)).expect("a fallback always gives an image");
                // Companion images (`loadOptionalTexture`): the normal and
                // gloss maps of a lit surface, beside its found base image.
                let stage = props.primary_stage.as_ref();
                // animMap frames (missing ones left out, as SHAG filters them).
                let frame_views: Vec<String> = stage
                    .filter(|_| props.needs_animation)
                    .and_then(|s| s.anim_map.as_ref())
                    .map(|a| {
                        a.frames
                            .iter()
                            .filter_map(|f| view_for(&add_texture_prefixes(f), None).map(|(key, _)| key))
                            .collect()
                    })
                    .unwrap_or_default();

                let lighting = if state.unlit {
                    // Q3StageColorPlugin's vertex variant, for additive vertex-coloured stages.
                    if props.uses_vertex_rgb == Some(true) { 2.0 } else { 1.0 }
                } else if props.uses_vertex_rgb == Some(true) {
                    2.0
                } else if props.use_lightmap {
                    0.0
                } else {
                    1.0
                };
                // Only the baked lighting path reads them.
                let base_path = base_path.unwrap_or_default();
                let mut companion = |explicit: Option<&str>, blank: bool, suffixes: &[&str]| {
                    if lighting != 0.0 {
                        return None;
                    }
                    let candidates = optional_texture_candidates(explicit, blank, &base_path, suffixes);
                    if candidates.is_empty() {
                        return None;
                    }
                    view_for(&candidates, None).and_then(|(key, path)| path.map(|_| key))
                };
                let normal_view = companion(
                    props.normal_texture.as_deref(),
                    props.normal_texture_explicitly_blank,
                    &NORMAL_SUFFIXES,
                );
                let gloss_view = companion(
                    props.gloss_texture.as_deref(),
                    props.gloss_texture_explicitly_blank,
                    &GLOSS_SUFFIXES,
                );
                let pbr = should_use_generated_pbr(props, normal_view.is_some(), gloss_view.is_some());
                let exponent = gloss_exponent(props);
                let surface = [
                    f32::from(u8::from(normal_view.is_some())),
                    f32::from(u8::from(gloss_view.is_some())),
                    f32::from(u8::from(pbr)),
                    0.0,
                ];
                let gloss = [
                    gloss_intensity(props) as f32,
                    exponent as f32,
                    (1.0 - pbr_micro_surface(exponent).clamp(0.0, 1.0)) as f32,
                    gloss_intensity(props) as f32,
                ];
                let normal_view = normal_view.unwrap_or_else(|| FLAT_NORMAL.to_owned());
                let gloss_view = gloss_view.unwrap_or_else(|| NO_GLOSS.to_owned());
                // The glow image (`loadOptionalTexture` with no suffixes):
                // the emissive of a lit or fullbright surface, or, with a glow
                // stage, the image of its additive shine overlay.
                let glow_view = if lighting == 0.0 || props.is_fullbright {
                    let candidates = optional_texture_candidates(
                        props.glow_texture.as_deref(),
                        props.glow_texture_explicitly_blank,
                        "",
                        &[],
                    );
                    view_for(&candidates, None).and_then(|(key, path)| path.map(|_| key))
                } else {
                    None
                };
                let shine_stage = props.glow_stage.as_ref().filter(|_| glow_view.is_some());
                let shine_frames: Vec<String> = shine_stage
                    .and_then(|s| s.anim_map.as_ref())
                    .map(|a| {
                        a.frames
                            .iter()
                            .filter_map(|f| view_for(&add_texture_prefixes(f), None).map(|(key, _)| key))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut surface = surface;
                surface[3] = f32::from(u8::from(glow_view.is_some() && shine_stage.is_none()));
                let glow_view = glow_view.unwrap_or_else(|| NO_GLOSS.to_owned());

                let alpha_const = stage
                    .and_then(|s| s.alpha_gen)
                    .filter(|g| g.kind == AlphaGenType::Const)
                    .map_or(1.0, |g| g.value.unwrap_or(1.0) as f32);
                let mut layers = vec![Layer {
                    base_view,
                    frame_views,
                    stage: stage.filter(|_| props.needs_animation),
                    params: [
                        state.alpha_cutoff.unwrap_or(-1.0),
                        if state.texture_alpha { 1.0 } else { 0.0 },
                        lighting,
                        if props.uses_vertex_alpha == Some(true) {
                            1.0
                        } else {
                            0.0
                        },
                    ],
                    rgb_gen: props.rgb_gen,
                    alpha: alpha_const,
                    ramps: (
                        ramp(stage.and_then(|s| s.rgb_gen).and_then(|g| g.distance_ramp)),
                        ramp(stage.and_then(|s| s.alpha_gen).and_then(|g| g.distance_ramp)),
                    ),
                    surface,
                    gloss,
                    key: PipelineKey {
                        two_sided: batch.two_sided,
                        blend: state.blend,
                        depth_write: state.depth_write,
                        polygon_offset: state.polygon_offset,
                    },
                }];
                // registerGlowAnimation's shine: the glow image unlit and
                // added (ALPHA_ONEONE) over the surface, without depth
                // writes, stepped by the glow stage.
                if let Some(glow_stage) = shine_stage {
                    layers.push(Layer {
                        base_view: glow_view.clone(),
                        frame_views: shine_frames,
                        stage: Some(glow_stage),
                        params: [-1.0, 0.0, 1.0, 0.0],
                        rgb_gen: glow_stage.rgb_gen,
                        alpha: 1.0,
                        ramps: (ramp(None), ramp(None)),
                        surface: [0.0; 4],
                        gloss: [0.0; 4],
                        key: PipelineKey {
                            two_sided: batch.two_sided,
                            blend: Blend::OneOne,
                            depth_write: false,
                            polygon_offset: true,
                        },
                    });
                }
                for layer in layers {
                    let rgb_const = layer
                        .rgb_gen
                        .filter(|g| g.kind == RgbGenType::Const)
                        .and_then(|g| g.color)
                        .map_or([1.0; 3], |c| c.map(|v| v as f32));
                    let base_color = [rgb_const[0], rgb_const[1], rgb_const[2], layer.alpha];
                    let mut uniform = MaterialUniform {
                        params: layer.params,
                        uv_rows: [1.0, 0.0, 0.0, 1.0],
                        uv_origin: [0.0; 4],
                        color: base_color,
                        ramp_rgb: layer.ramps.0,
                        ramp_alpha: layer.ramps.1,
                        surface: layer.surface,
                        gloss: layer.gloss,
                        fog: [fog_pass_kind(layer.key.blend), 0.0, 0.0, 0.0],
                    };
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("world material"),
                        size: std::mem::size_of::<MaterialUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let mut bind_group_for = |key: &str| {
                        texture_bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some(&batch.texture_name),
                            layout: &texture_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(&views[key]),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(&base_sampler),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: buffer.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 3,
                                    resource: wgpu::BindingResource::TextureView(&views[&normal_view]),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 4,
                                    resource: wgpu::BindingResource::TextureView(&views[&gloss_view]),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 5,
                                    resource: wgpu::BindingResource::TextureView(&views[&glow_view]),
                                },
                            ],
                        }));
                        texture_bind_groups.len() - 1
                    };
                    let bind_group = bind_group_for(&layer.base_view);
                    let frames: Vec<usize> = layer.frame_views.iter().map(|k| bind_group_for(k)).collect();
                    let animation = layer.stage.map(|stage| {
                        let tc_mods = stage.tc_mod.clone().unwrap_or_default();
                        let tc_state = create_tc_state(&tc_mods);
                        uniform.set_tc_state(&tc_state);
                        PassAnimation {
                            pass: 0,
                            tc_mods,
                            tc_state,
                            rgb_wave: stage
                                .rgb_gen
                                .filter(|g| g.kind == RgbGenType::Wave)
                                .and_then(|g| g.wave),
                            alpha_wave: stage
                                .alpha_gen
                                .filter(|g| g.kind == AlphaGenType::Wave)
                                .and_then(|g| g.wave),
                            frames,
                            frequency: stage.anim_map.as_ref().map_or(0.0, |a| a.frequency),
                            base_color,
                        }
                    });
                    gpu.queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&uniform));
                    let key = layer.key;
                    let pipeline = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                        keys.push(key);
                        keys.len() - 1
                    });
                    let pass = DrawPass {
                        pipeline,
                        bind_group,
                        uniform,
                        buffer,
                        first_index: batch.first_index,
                        index_count: batch.index_count,
                        batch: batch_index,
                    };
                    if layer.key.blend != Blend::Opaque {
                        blended.push((pass, animation));
                    } else {
                        opaque.push((pass, animation));
                    }
                }
            }
        }
        let texture_count = views.len() - 2;
        let opaque_count = opaque.len();
        let mut passes = Vec::new();
        let mut animations = Vec::new();
        for (pass, animation) in opaque.into_iter().chain(blended) {
            if let Some(mut animation) = animation {
                animation.pass = passes.len();
                animations.push(animation);
            }
            passes.push(pass);
        }

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world.wgsl"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("fog.wgsl"), include_str!("world.wgsl")).into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = |key: &PipelineKey| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<WorldVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32,
                            6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // The Q3-to-Babylon swap mirrors the map and the builders
                    // reverse the winding, so a face seen from its front is
                    // counter-clockwise on screen (checked on washoi_dm2:
                    // the other way culls the walls you look at).
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: if key.two_sided { None } else { Some(wgpu::Face::Back) },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(key.depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    // Babylon's zOffset -2: glPolygonOffset(-2, 0).
                    bias: if key.polygon_offset {
                        wgpu::DepthBiasState {
                            constant: 0,
                            slope_scale: -2.0,
                            clamp: 0.0,
                        }
                    } else {
                        wgpu::DepthBiasState::default()
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: key.blend.state(),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = keys.iter().map(pipeline).collect();

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world vertices"),
            contents: bytemuck::cast_slice(&geometry.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world indices"),
            contents: bytemuck::cast_slice(&geometry.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            vertex_buffer,
            index_buffer,
            globals_buffer,
            globals_bind_group,
            texture_bind_groups,
            texture_count,
            missing,
            passes,
            opaque_count,
            animations,
            total_time: 0.0,
            noise_seed: 0x9e37_79b9_7f4a_7c15,
            pipelines,
            lighting: [profile.gain as f32, if stride == 2 { 1.0 } else { 0.0 }, 0.0, 0.0],
            ambient: [
                profile.ambient[0] as f32,
                profile.ambient[1] as f32,
                profile.ambient[2] as f32,
                0.0,
            ],
            dynamic: DynamicLightBlock::default(),
            fog: FogUniforms::default(),
        }
    }

    /// The dynamic lights the next `prepare` hands the lightmapped surfaces
    /// (`diffuseBase` in BspBakedLightingPlugin).
    pub fn set_dynamic_lights(&mut self, lights: &[DynamicLight]) {
        self.dynamic = DynamicLightBlock::pack(lights);
    }

    /// How many base images were uploaded.
    #[must_use]
    pub fn texture_count(&self) -> usize {
        self.texture_count
    }

    /// The textures with a pass that found no image and drew white.
    #[must_use]
    pub fn missing_images(&self) -> &[String] {
        &self.missing
    }

    /// How many material passes are drawn.
    #[must_use]
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    /// How many passes animate.
    #[must_use]
    pub fn animated_pass_count(&self) -> usize {
        self.animations.len()
    }

    /// Advances shader animation by `delta_seconds`, as
    /// `ShaderAnimationSystem.update` does each frame: tcMods stepped,
    /// waves sampled at the total time, `animMap` frames chosen.
    pub fn animate(&mut self, gpu: &Gpu, delta_seconds: f64) {
        self.total_time += delta_seconds;
        let total = self.total_time;
        let seed = &mut self.noise_seed;
        let mut rng = move || {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            (*seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for animation in &mut self.animations {
            let pass = &mut self.passes[animation.pass];
            if !animation.frames.is_empty() {
                let index = ((total * animation.frequency).floor() as usize) % animation.frames.len();
                pass.bind_group = animation.frames[index];
            }
            if !animation.tc_mods.is_empty() {
                step_tc_mods_into(
                    &mut animation.tc_state,
                    &animation.tc_mods,
                    delta_seconds,
                    total,
                    &mut rng,
                );
                pass.uniform.set_tc_state(&animation.tc_state);
            }
            let mut color = animation.base_color;
            if let Some(wave) = &animation.rgb_wave {
                let value = evaluate_wave_func(wave, total, &mut rng).clamp(0.0, 1.0) as f32;
                for c in &mut color[..3] {
                    *c *= value;
                }
            }
            if let Some(wave) = &animation.alpha_wave {
                color[3] = evaluate_wave_func(wave, total, &mut rng).clamp(0.0, 1.0) as f32;
            }
            pass.uniform.color = color;
            gpu.queue
                .write_buffer(&pass.buffer, 0, bytemuck::bytes_of(&pass.uniform));
        }
    }

    /// The world fog to draw, from the next [`prepare`](Self::prepare).
    pub fn set_fog(&mut self, fog: FogUniforms) {
        self.fog = fog;
    }

    /// Writes this frame's camera.
    pub fn prepare(&self, gpu: &Gpu, view_proj: Mat4, eye: Vec3) {
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            lighting: self.lighting,
            ambient: self.ambient,
            eye: [eye.x, eye.y, eye.z, 0.0],
            fog: self.fog,
            dynamic: self.dynamic,
        };
        gpu.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
    }

    /// Records the whole world into an open pass (colour and depth already
    /// bound).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_visible(pass, &[]);
    }

    /// Records the batches `visible` keeps (by batch index; a batch past its
    /// end is drawn), as [`WorldCulling`](super::world_culling::WorldCulling)
    /// gives them.
    pub fn draw_visible(&self, pass: &mut wgpu::RenderPass<'_>, visible: &[bool]) {
        self.draw_passes(pass, visible, 0..self.passes.len());
    }

    /// Records the opaque passes of the batches `visible` keeps.
    pub fn draw_visible_opaque(&self, pass: &mut wgpu::RenderPass<'_>, visible: &[bool]) {
        self.draw_passes(pass, visible, 0..self.opaque_count);
    }

    /// Records the blended passes of the batches `visible` keeps, which
    /// draw after every opaque surface (the world's and the models').
    pub fn draw_visible_blended(&self, pass: &mut wgpu::RenderPass<'_>, visible: &[bool]) {
        self.draw_passes(pass, visible, self.opaque_count..self.passes.len());
    }

    fn draw_passes(&self, pass: &mut wgpu::RenderPass<'_>, visible: &[bool], range: std::ops::Range<usize>) {
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let mut current = None;
        for pass_ in &self.passes[range] {
            if !visible.get(pass_.batch).copied().unwrap_or(true) {
                continue;
            }
            if current != Some(pass_.pipeline) {
                pass.set_pipeline(&self.pipelines[pass_.pipeline]);
                current = Some(pass_.pipeline);
            }
            pass.set_bind_group(1, &self.texture_bind_groups[pass_.bind_group], &[]);
            pass.draw_indexed(pass_.first_index..pass_.first_index + pass_.index_count, 0, 0..1);
        }
    }
}

pub(crate) fn texture_entry(binding: u32, dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: dimension,
            multisampled: false,
        },
        count: None,
    }
}

pub(crate) fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
