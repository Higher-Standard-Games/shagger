//! The far box (SHAG `src/engine/loader/texture/SkyboxLoader.ts`,
//! `loadSkybox`, and `BspMeshBuilder.applySkybox`): the six faces as a cube
//! texture, drawn first, unlit and without depth, behind the world, as
//! Babylon draws the infinite-distance box in rendering group 0.

use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::camera::Camera;
use super::gpu::Gpu;
use super::world_fog_controller::FogUniforms;
use super::world_geometry::SkyShader;
use super::world_renderer::DEPTH_FORMAT;
use crate::loader::pk3::pk3_loader::Pk3Loader;
use crate::loader::texture::skybox_loader::{SkyboxFaces, load_skybox};

/// `BspMeshBuilder.applySkybox`'s far box branches: the first sky shader
/// with a `skyparms` far box, else, as the last resort, a sky texture's own
/// name as a far box. Returns the faces and the base path they came from.
/// The cloud layer and sky sphere branches between them are not ported yet
/// (no shipped map reaches them).
pub fn apply_skybox(pk3: &mut Pk3Loader, sky_shaders: &[SkyShader]) -> Option<(SkyboxFaces, String)> {
    if let Some(farbox) = sky_shaders
        .iter()
        .find_map(|s| s.sky_parms.as_ref().map(|p| p.farbox.clone()).filter(|f| !f.is_empty()))
        && let Ok(faces) = load_skybox(pk3, &farbox)
    {
        return Some((faces, farbox));
    }
    sky_shaders
        .iter()
        .filter(|s| !s.texture_name.is_empty())
        .find_map(|s| Some((load_skybox(pk3, &s.texture_name).ok()?, s.texture_name.clone())))
}

pub struct SkyRenderer {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    fog: FogUniforms,
}

/// The sky's uniform (`sky.wgsl`'s `Sky`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniform {
    inverse_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    fog: FogUniforms,
}

impl SkyRenderer {
    /// Uploads the faces, slot `i` to cube layer `i` as Babylon uploads
    /// them to `TEXTURE_CUBE_MAP_POSITIVE_X + i`.
    #[must_use]
    pub fn new(gpu: &Gpu, faces: &SkyboxFaces, color_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let texture = device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("sky"),
                size: wgpu::Extent3d {
                    width: faces.size,
                    height: faces.size,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &faces.faces.concat(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        // Trilinear without mip maps: linear, clamped as cube textures are.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky"),
            size: std::mem::size_of::<SkyUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky.wgsl"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("fog.wgsl"), include_str!("sky.wgsl")).into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            buffer,
            bind_group,
            fog: FogUniforms::default(),
        }
    }

    /// Writes this frame's camera: `infiniteDistance` keeps the box centred
    /// on the eye, so only the camera's turn matters.
    pub fn prepare(&self, gpu: &Gpu, camera: &Camera, aspect: f32) {
        let centred = Camera {
            position: Vec3::ZERO,
            ..*camera
        };
        let inverse: Mat4 = centred.view_projection(aspect).inverse();
        let uniform = SkyUniform {
            inverse_view_proj: inverse.to_cols_array_2d(),
            eye: [camera.position.x, camera.position.y, camera.position.z, 0.0],
            fog: self.fog,
        };
        gpu.queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// The world fog the sky blends into, from the next [`prepare`](Self::prepare).
    pub fn set_fog(&mut self, fog: FogUniforms) {
        self.fog = fog;
    }

    /// Records the sky into an open pass; draw it before the world.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
