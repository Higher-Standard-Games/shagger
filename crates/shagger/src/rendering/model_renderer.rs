//! Draws glTF models ([`GltfModel`]) with wgpu, lit as Babylon's
//! `PBRMaterial` lights them with the scene's hemispheric and directional
//! lights and no environment texture (`model.wgsl`).
//!
//! A model is uploaded once ([`ModelRenderer::upload`]): a vertex and index
//! buffer per primitive, a base texture and uniform per material, each
//! material drawn the way [`ModelMaterialMode`] says. A frame lists draws
//! (a primitive and its world matrix), opaque ones first and then the
//! blended ones back to front by their bounding centres, as Babylon's
//! transparent queue sorts them; a tie keeps Babylon's mesh creation order
//! (V8's sort is stable), which decides how a weapon's glass and glow
//! layers, sharing one centre, stack.
//! Blended draws do not write depth: Babylon's `setAlphaMode` turns the
//! depth mask off for any blending mode.
//! A mirroring world matrix flips which winding faces front, as Babylon
//! flips the side orientation when the world matrix's determinant is
//! negative.
//!
//! A rigged primitive ([`ModelPrimitive::skin`]) is skinned on the GPU from
//! the frame's joint palette ([`ModelDraw::skin`]), a draw may replace its
//! albedo colour ([`ModelDraw::tint`], a player's team colour) and may carry
//! Babylon's outline ([`ModelDraw::outline`]): drawn just before the mesh
//! without depth write and pulled forward by the renderer's z offset, then
//! again after it into depth only, as `OutlineRenderer` does.
//!
//! Not yet drawn as SHAG draws them: normal and metallic-roughness maps,
//! specular light, the clustered dynamic lights.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::gpu::Gpu;
use super::world_renderer::{DEPTH_FORMAT, Rgba8Image, sampler_entry, texture_entry, upload_texture};
use crate::babylon::materials::STANDARD_WGSL;
use crate::babylon::materials::standard_material::StandardAlphaMode;
use crate::babylon::materials::standard_material::StandardMaterialParams;
use crate::babylon::materials::texture::TextureMatrix;
use crate::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, ModelPrimitive};
use crate::loader::texture::decode_image;

/// How a material draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelMaterialMode {
    /// Lit; opaque unless its glTF alpha mode blends.
    Lit,
    /// Babylon's `drawAsAddedLight`: unlit, added over what is behind it
    /// (`ALPHA_ADD`), both sides.
    AddedLight,
    /// Lit, alpha blended at this alpha, both sides (a `_glass` layer).
    Glass(f32),
    /// A Babylon `StandardMaterial` (`standard.wgsl`), with the material's
    /// base colour image as its one texture.
    Standard(StandardMaterialParams),
}

/// A light as Babylon hands it to the shader.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelLight {
    Hemispheric {
        /// Up, as set; normalized when used.
        direction: Vec3,
        diffuse: Vec3,
        /// What `StandardMaterial` takes for highlights; PBR models do not
        /// use it yet.
        specular: Vec3,
        ground: Vec3,
        intensity: f32,
    },
    Directional {
        /// The way the light travels.
        direction: Vec3,
        diffuse: Vec3,
        specular: Vec3,
        intensity: f32,
    },
}

/// An uploaded model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelHandle(usize);

/// Babylon's mesh outline (`renderOutline`, `outlineColor`, `outlineWidth`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub color: Vec3,
    /// How far the outline stands off the mesh, in the mesh's bind space.
    pub width: f32,
}

/// One primitive drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelDraw {
    pub model: ModelHandle,
    pub primitive: usize,
    pub world: Mat4,
    /// Replaces the material's albedo colour.
    pub tint: Option<Vec3>,
    /// Replaces a standard material's emissive colour (a pulse).
    pub emissive: Option<Vec3>,
    /// A standard material's texture matrix (its texture's offsets, scales
    /// and rotation, as `getTextureMatrix` makes them).
    pub texture_matrix: Option<TextureMatrix>,
    /// A standard material's texture `level`.
    pub texture_level: Option<f32>,
    /// The primitive's first joint in the frame's palette (a rigged
    /// primitive must have one; it is not drawn without).
    pub skin: Option<u32>,
    pub outline: Option<Outline>,
}

impl ModelDraw {
    /// A plain draw: no tint, skin or outline.
    #[must_use]
    pub fn new(model: ModelHandle, primitive: usize, world: Mat4) -> Self {
        Self {
            model,
            primitive,
            world,
            tint: None,
            emissive: None,
            texture_matrix: None,
            texture_level: None,
            skin: None,
            outline: None,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ModelVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct SkinVertex {
    joints: [u32; 4],
    weights: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    world: [f32; 16],
    tint: [f32; 4],
    outline: [f32; 4],
    skin: [u32; 4],
    emissive: [f32; 4],
    uv_rows: [f32; 4],
    uv_origin: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct LightUniform {
    kind: [f32; 4],
    direction: [f32; 4],
    diffuse: [f32; 4],
    ground: [f32; 4],
    specular: [f32; 4],
}

const MAX_LIGHTS: usize = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    image: [f32; 4],
    lights: [LightUniform; MAX_LIGHTS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    params: [f32; 4],
}

/// Dynamic uniform offsets must be 256-byte aligned.
const DRAW_STRIDE: u64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    blend: BlendKind,
    cull: Cull,
    depth_write: bool,
    /// Drawn with `standard.wgsl`.
    standard: bool,
    /// Clockwise faces front: the world matrix mirrors.
    mirrored: bool,
    skinned: bool,
    pass: PassKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cull {
    None,
    Back,
    Front,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PassKind {
    Color,
    /// The outline's colour, before the mesh, no depth write.
    Outline,
    /// The outline into depth only, after the mesh.
    OutlineDepth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BlendKind {
    Opaque,
    Alpha,
    Add,
    /// `ALPHA_ONEONE`.
    OneOne,
}

struct GpuPrimitive {
    vertex: wgpu::Buffer,
    skin: Option<wgpu::Buffer>,
    index: wgpu::Buffer,
    count: u32,
    material: usize,
    bounds_center: Vec3,
    creation_order: usize,
}

struct GpuMaterial {
    bind_group: wgpu::BindGroup,
    blend: BlendKind,
    two_sided: bool,
    standard: bool,
    depth_write: bool,
    /// Back faces, then front faces (`separateCullingPass`).
    separate_culling: bool,
}

struct GpuModel {
    primitives: Vec<GpuPrimitive>,
    materials: Vec<GpuMaterial>,
}

pub struct ModelRenderer {
    color_format: wgpu::TextureFormat,
    shader: wgpu::ShaderModule,
    standard_shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    material_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    draw_layout: wgpu::BindGroupLayout,
    draw_buffer: wgpu::Buffer,
    draw_capacity: usize,
    joint_buffer: wgpu::Buffer,
    joint_capacity: usize,
    draw_bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    models: Vec<GpuModel>,
    lights: Vec<ModelLight>,
    /// This frame's draws, in drawing order, with their pipeline.
    frame: Vec<(ModelDraw, PipelineKey)>,
    /// Diffuse BRDF: false Lambert, true Burley.
    burley: bool,
}

fn create_buffer(gpu: &Gpu, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_draw_bind_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    draws: &wgpu::Buffer,
    joints: &wgpu::Buffer,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("model draws"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: draws,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniform>() as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: joints.as_entire_binding(),
            },
        ],
    })
}

fn uniform_entry(binding: u32, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

impl ModelRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model globals"),
            entries: &[uniform_entry(0, false)],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model draw"),
            entries: &[
                uniform_entry(0, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model material"),
            entries: &[
                texture_entry(0, wgpu::TextureViewDimension::D2),
                sampler_entry(1),
                uniform_entry(2, false),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model"),
            bind_group_layouts: &[Some(&globals_layout), Some(&draw_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("model.wgsl"));
        let standard_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("standard.wgsl"),
            source: wgpu::ShaderSource::Wgsl(STANDARD_WGSL.into()),
        });
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let draw_capacity = 64;
        let draw_buffer = create_buffer(
            gpu,
            "model draws",
            DRAW_STRIDE * draw_capacity as u64,
            wgpu::BufferUsages::UNIFORM,
        );
        let joint_capacity = 128;
        let joint_buffer = create_buffer(
            gpu,
            "model joints",
            64 * joint_capacity as u64,
            wgpu::BufferUsages::STORAGE,
        );
        let draw_bind_group = create_draw_bind_group(gpu, &draw_layout, &draw_buffer, &joint_buffer);
        // glTF samplers on Washoi's models all repeat with trilinear filtering.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model base"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white = upload_texture(gpu, "model white", &Rgba8Image::white())
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            color_format,
            shader,
            standard_shader,
            layout,
            material_layout,
            pipelines: HashMap::new(),
            globals_buffer,
            globals_bind_group,
            draw_layout,
            draw_buffer,
            draw_capacity,
            joint_buffer,
            joint_capacity,
            draw_bind_group,
            sampler,
            white,
            models: Vec::new(),
            lights: Vec::new(),
            frame: Vec::new(),
            burley: false,
        }
    }

    /// The scene lights that reach the models (at most four are used).
    pub fn set_lights(&mut self, lights: Vec<ModelLight>) {
        self.lights = lights;
    }

    /// Chooses the directional lights' diffuse BRDF: Burley's, or Lambert's.
    pub fn set_burley_diffuse(&mut self, burley: bool) {
        self.burley = burley;
    }

    /// Uploads a model; `mode` says how each of its materials draws.
    pub fn upload(
        &mut self,
        gpu: &Gpu,
        model: &GltfModel,
        mode: impl Fn(&ModelMaterial) -> ModelMaterialMode,
    ) -> ModelHandle {
        let device = &gpu.device;
        let mut views: HashMap<usize, wgpu::TextureView> = HashMap::new();
        let mut materials = Vec::new();
        let fallback = ModelMaterial {
            name: String::new(),
            base_color: [1.0; 4],
            base_color_image: None,
            metallic: 1.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        };
        let count = model.materials.len().max(1);
        for i in 0..count {
            let m = model.materials.get(i).unwrap_or(&fallback);
            let mode = mode(m);
            if let Some(image) = m.base_color_image
                && !views.contains_key(&image)
                && let Some(decoded) = model
                    .images
                    .get(image)
                    .and_then(|img| decode_image(&img.name, &img.bytes))
            {
                let texture = upload_texture(
                    gpu,
                    &model.images[image].name,
                    &Rgba8Image {
                        width: decoded.width,
                        height: decoded.height,
                        pixels: decoded.pixels,
                    },
                );
                views.insert(image, texture.create_view(&wgpu::TextureViewDescriptor::default()));
            }
            let view = m.base_color_image.and_then(|i| views.get(&i)).unwrap_or(&self.white);
            if let ModelMaterialMode::Standard(params) = mode {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("standard material"),
                    contents: bytemuck::bytes_of(&params.uniform()),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                materials.push(GpuMaterial {
                    bind_group: self.material_bind_group(gpu, view, &buffer),
                    blend: match (params.blends(), params.alpha_mode) {
                        (false, _) => BlendKind::Opaque,
                        (true, StandardAlphaMode::Combine) => BlendKind::Alpha,
                        (true, StandardAlphaMode::Add) => BlendKind::Add,
                        (true, StandardAlphaMode::OneOne) => BlendKind::OneOne,
                    },
                    two_sided: !params.back_face_culling,
                    standard: true,
                    // Babylon's transparent pass sets the alpha mode, which
                    // turns depth writes off (`setAlphaMode`).
                    depth_write: !params.disable_depth_write && !params.blends(),
                    separate_culling: params.separate_culling_pass && !params.back_face_culling,
                });
                continue;
            }
            let (blend, two_sided, alpha, unlit) = match mode {
                ModelMaterialMode::Lit => (
                    if m.alpha_mode == AlphaMode::Blend {
                        BlendKind::Alpha
                    } else {
                        BlendKind::Opaque
                    },
                    m.double_sided,
                    1.0,
                    0.0,
                ),
                ModelMaterialMode::AddedLight => (BlendKind::Add, true, 1.0, 1.0),
                ModelMaterialMode::Glass(alpha) => (BlendKind::Alpha, true, alpha, 0.0),
                ModelMaterialMode::Standard(_) => unreachable!("handled above"),
            };
            let uniform = MaterialUniform {
                base_color: m.base_color,
                params: [m.metallic, m.roughness, unlit, alpha],
            };
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("model material"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            materials.push(GpuMaterial {
                bind_group: self.material_bind_group(gpu, view, &buffer),
                blend,
                two_sided,
                standard: false,
                depth_write: blend == BlendKind::Opaque,
                separate_culling: false,
            });
        }
        let primitives = model
            .primitives
            .iter()
            .map(|p| {
                let vertices: Vec<ModelVertex> = p
                    .positions
                    .iter()
                    .enumerate()
                    .map(|(i, &position)| ModelVertex {
                        position,
                        normal: p.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                        uv: p.uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                    })
                    .collect();
                GpuPrimitive {
                    vertex: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("model vertices"),
                        contents: bytemuck::cast_slice(&vertices),
                        // Effects (trails) rewrite theirs each frame.
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    }),
                    skin: skin_vertices(p).map(|skin| {
                        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("model skin"),
                            contents: bytemuck::cast_slice(&skin),
                            usage: wgpu::BufferUsages::VERTEX,
                        })
                    }),
                    index: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("model indices"),
                        contents: bytemuck::cast_slice(&p.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    count: p.indices.len() as u32,
                    material: p.material.unwrap_or(0).min(count - 1),
                    bounds_center: p.bounds_center,
                    creation_order: p.creation_order,
                }
            })
            .collect();
        self.models.push(GpuModel { primitives, materials });
        ModelHandle(self.models.len() - 1)
    }

    /// Rewrites one primitive's vertices (an effect's geometry that moves
    /// each frame), keeping its count, and recentres its bounds, which
    /// order it among the transparent draws (`refreshBoundingInfo`).
    pub fn update_vertices(
        &mut self,
        gpu: &Gpu,
        model: ModelHandle,
        primitive: usize,
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        uvs: &[[f32; 2]],
    ) {
        let Some(p) = self
            .models
            .get_mut(model.0)
            .and_then(|m| m.primitives.get_mut(primitive))
        else {
            return;
        };
        let vertices: Vec<ModelVertex> = positions
            .iter()
            .enumerate()
            .map(|(i, &position)| ModelVertex {
                position,
                normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
            })
            .collect();
        let bytes: &[u8] = bytemuck::cast_slice(&vertices);
        if bytes.len() as u64 <= p.vertex.size() {
            gpu.queue.write_buffer(&p.vertex, 0, bytes);
        }
        let (min, max) = positions
            .iter()
            .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| {
                (lo.min(Vec3::from(*v)), hi.max(Vec3::from(*v)))
            });
        p.bounds_center = (min + max) * 0.5;
    }

    fn material_bind_group(&self, gpu: &Gpu, view: &wgpu::TextureView, buffer: &wgpu::Buffer) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model material"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        })
    }

    fn pipeline(&mut self, gpu: &Gpu, key: PipelineKey) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let blend = match key.blend {
            BlendKind::Opaque => None,
            BlendKind::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
            // ALPHA_ADD: source times its alpha, plus what is there.
            BlendKind::Add => Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            BlendKind::OneOne => Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
        };
        let outline = key.pass != PassKind::Color;
        let vertex_entry = match (outline, key.skinned) {
            (false, false) => "vs_main",
            (false, true) => "vs_skinned",
            (true, false) => "vs_outline",
            (true, true) => "vs_outline_skinned",
        };
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ModelVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
        };
        let skin_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SkinVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![3 => Uint32x4, 4 => Float32x4],
        };
        let buffers = [Some(mesh_layout), Some(skin_layout)];
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: if key.standard {
                    &self.standard_shader
                } else {
                    &self.shader
                },
                entry_point: Some(vertex_entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &buffers[..if key.skinned { 2 } else { 1 }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // glTF's counter-clockwise front, seen through Babylon's
                // left-handed view without the loader's mirroring root,
                // turns clockwise on screen.
                front_face: if key.mirrored {
                    wgpu::FrontFace::Ccw
                } else {
                    wgpu::FrontFace::Cw
                },
                cull_mode: match key.cull {
                    Cull::None => None,
                    Cull::Back => Some(wgpu::Face::Back),
                    Cull::Front => Some(wgpu::Face::Front),
                },
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(match key.pass {
                    PassKind::Color => key.depth_write,
                    PassKind::Outline => false,
                    PassKind::OutlineDepth => true,
                }),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                // OutlineRenderer's zOffset 1 and zOffsetUnits 4, negated.
                bias: if outline {
                    wgpu::DepthBiasState {
                        constant: -4,
                        slope_scale: -1.0,
                        clamp: 0.0,
                    }
                } else {
                    wgpu::DepthBiasState::default()
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: if key.standard {
                    &self.standard_shader
                } else {
                    &self.shader
                },
                entry_point: Some(if outline { "fs_outline" } else { "fs_main" }),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend,
                    write_mask: if key.pass == PassKind::OutlineDepth {
                        wgpu::ColorWrites::empty()
                    } else {
                        wgpu::ColorWrites::ALL
                    },
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline);
    }

    /// Writes this frame's camera, lights, draws and joint palette (the
    /// joint matrices skinned draws index, each already in model space:
    /// joint global times inverse bind).
    pub fn prepare(&mut self, gpu: &Gpu, view_proj: Mat4, eye: Vec3, draws: &[ModelDraw], joints: &[Mat4]) {
        let mut lights = [LightUniform::zeroed(); MAX_LIGHTS];
        for (slot, light) in lights.iter_mut().zip(&self.lights) {
            *slot = match *light {
                ModelLight::Hemispheric {
                    direction,
                    diffuse,
                    specular,
                    ground,
                    intensity,
                } => {
                    let d = direction.normalize_or_zero();
                    LightUniform {
                        kind: [1.0, 0.0, 0.0, 0.0],
                        direction: [d.x, d.y, d.z, 0.0],
                        diffuse: (diffuse * intensity).extend(1.0).into(),
                        ground: (ground * intensity).extend(1.0).into(),
                        specular: (specular * intensity).extend(1.0).into(),
                    }
                }
                ModelLight::Directional {
                    direction,
                    diffuse,
                    specular,
                    intensity,
                } => LightUniform {
                    kind: [2.0, 0.0, 0.0, 0.0],
                    direction: [direction.x, direction.y, direction.z, 1.0],
                    diffuse: (diffuse * intensity).extend(1.0).into(),
                    ground: [0.0; 4],
                    specular: (specular * intensity).extend(1.0).into(),
                },
            };
        }
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 0.0],
            image: [0.0, 0.0, if self.burley { 1.0 } else { 0.0 }, 0.0],
            lights,
        };
        gpu.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        // Opaque first, then blended back to front.
        let mut opaque = Vec::new();
        let mut blended = Vec::new();
        for draw in draws {
            let Some(model) = self.models.get(draw.model.0) else {
                continue;
            };
            let Some(primitive) = model.primitives.get(draw.primitive) else {
                continue;
            };
            let skinned = primitive.skin.is_some();
            if skinned && draw.skin.is_none() {
                continue;
            }
            let material = &model.materials[primitive.material];
            let key = PipelineKey {
                blend: material.blend,
                cull: if material.two_sided { Cull::None } else { Cull::Back },
                depth_write: material.depth_write,
                standard: material.standard,
                mirrored: draw.world.determinant() < 0.0,
                skinned,
                pass: PassKind::Color,
            };
            if material.blend == BlendKind::Opaque {
                let outlined = draw.outline.is_some_and(|o| o.width > 0.0);
                if outlined {
                    opaque.push((
                        *draw,
                        PipelineKey {
                            pass: PassKind::Outline,
                            ..key
                        },
                    ));
                }
                opaque.push((*draw, key));
                if outlined {
                    opaque.push((
                        *draw,
                        PipelineKey {
                            pass: PassKind::OutlineDepth,
                            ..key
                        },
                    ));
                }
            } else {
                let distance = draw.world.transform_point3(primitive.bounds_center).distance(eye);
                if material.separate_culling {
                    // Back faces first: culling the front ones.
                    let back = PipelineKey {
                        cull: Cull::Front,
                        ..key
                    };
                    blended.push((*draw, back, distance, primitive.creation_order));
                    let front = PipelineKey {
                        cull: Cull::Back,
                        ..key
                    };
                    blended.push((*draw, front, distance, primitive.creation_order));
                } else {
                    blended.push((*draw, key, distance, primitive.creation_order));
                }
            }
        }
        blended.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.3.cmp(&b.3)));
        self.frame = opaque
            .into_iter()
            .chain(blended.into_iter().map(|(d, k, _, _)| (d, k)))
            .collect();
        for &(_, key) in &self.frame.clone() {
            self.pipeline(gpu, key);
        }
        let mut rebind = false;
        if self.frame.len() > self.draw_capacity {
            self.draw_capacity = self.frame.len().next_power_of_two();
            self.draw_buffer = create_buffer(
                gpu,
                "model draws",
                DRAW_STRIDE * self.draw_capacity as u64,
                wgpu::BufferUsages::UNIFORM,
            );
            rebind = true;
        }
        if joints.len() > self.joint_capacity {
            self.joint_capacity = joints.len().next_power_of_two();
            self.joint_buffer = create_buffer(
                gpu,
                "model joints",
                64 * self.joint_capacity as u64,
                wgpu::BufferUsages::STORAGE,
            );
            rebind = true;
        }
        if rebind {
            self.draw_bind_group =
                create_draw_bind_group(gpu, &self.draw_layout, &self.draw_buffer, &self.joint_buffer);
        }
        let mut bytes = vec![0u8; DRAW_STRIDE as usize * self.frame.len()];
        let size = std::mem::size_of::<DrawUniform>();
        for (i, (draw, _)) in self.frame.iter().enumerate() {
            let at = i * DRAW_STRIDE as usize;
            let uv = draw.texture_matrix.unwrap_or(TextureMatrix::IDENTITY);
            let outline = draw.outline.unwrap_or(Outline {
                color: Vec3::ZERO,
                width: 0.0,
            });
            let uniform = DrawUniform {
                world: draw.world.to_cols_array(),
                tint: draw.tint.map_or([0.0; 4], |t| t.extend(1.0).into()),
                outline: outline.color.extend(outline.width).into(),
                skin: [draw.skin.unwrap_or(0), 0, 0, 0],
                emissive: draw.emissive.map_or([0.0; 4], |e| e.extend(1.0).into()),
                uv_rows: [
                    uv.u_row[0] as f32,
                    uv.u_row[1] as f32,
                    uv.v_row[0] as f32,
                    uv.v_row[1] as f32,
                ],
                uv_origin: [
                    uv.origin[0] as f32,
                    uv.origin[1] as f32,
                    draw.texture_level.unwrap_or(1.0),
                    0.0,
                ],
            };
            bytes[at..at + size].copy_from_slice(bytemuck::bytes_of(&uniform));
        }
        if !bytes.is_empty() {
            gpu.queue.write_buffer(&self.draw_buffer, 0, &bytes);
        }
        if !joints.is_empty() {
            let flat: Vec<[f32; 16]> = joints.iter().map(Mat4::to_cols_array).collect();
            gpu.queue
                .write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(&flat));
        }
    }

    /// Records this frame's opaque draws.
    pub fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_range(pass, true);
    }

    /// Records this frame's blended draws.
    pub fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_range(pass, false);
    }

    fn draw_range(&self, pass: &mut wgpu::RenderPass<'_>, opaque: bool) {
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        for (i, (draw, key)) in self.frame.iter().enumerate() {
            if (key.blend == BlendKind::Opaque) != opaque {
                continue;
            }
            let model = &self.models[draw.model.0];
            let primitive = &model.primitives[draw.primitive];
            pass.set_pipeline(&self.pipelines[key]);
            pass.set_bind_group(1, &self.draw_bind_group, &[(i as u64 * DRAW_STRIDE) as u32]);
            pass.set_bind_group(2, &model.materials[primitive.material].bind_group, &[]);
            pass.set_vertex_buffer(0, primitive.vertex.slice(..));
            if let Some(skin) = &primitive.skin {
                pass.set_vertex_buffer(1, skin.slice(..));
            }
            pass.set_index_buffer(primitive.index.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..primitive.count, 0, 0..1);
        }
    }

    /// How many primitives a model has, by handle.
    #[must_use]
    pub fn primitive_count(&self, model: ModelHandle) -> usize {
        self.models.get(model.0).map_or(0, |m| m.primitives.len())
    }
}

/// A rigged primitive's joints and weights as a vertex stream.
fn skin_vertices(p: &ModelPrimitive) -> Option<Vec<SkinVertex>> {
    let skin = p.skin.as_ref()?;
    Some(
        (0..p.positions.len())
            .map(|i| SkinVertex {
                joints: skin.joints.get(i).copied().unwrap_or([0; 4]),
                weights: skin.weights.get(i).copied().unwrap_or([0.0; 4]),
            })
            .collect(),
    )
}
