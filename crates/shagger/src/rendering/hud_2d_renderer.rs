//! Puts a [`Hud2d`] frame on the output after image processing: one
//! pipeline, the glyph atlas as an R8 texture, straight alpha blending.

use wgpu::util::DeviceExt;

use super::gpu::Gpu;
use crate::ui::hud_2d::{Hud2d, HudVertex};

pub struct Hud2dRenderer {
    pipeline: wgpu::RenderPipeline,
    screen: wgpu::Buffer,
    atlas: wgpu::Texture,
    atlas_size: u32,
    bind_group: wgpu::BindGroup,
    /// The frame's vertices, then its indices, in one buffer: one upload a
    /// frame instead of two (each upload stages through a buffer of its
    /// own).
    geometry: wgpu::Buffer,
    geometry_capacity: usize,
    /// Where the indices start in `geometry`, in bytes.
    index_offset: u64,
    index_count: u32,
    /// The screen size last uploaded.
    screen_size: [f32; 2],
    /// The frame's bytes, kept for their storage.
    bytes: Vec<u8>,
}

fn buffer(gpu: &Gpu, usage: wgpu::BufferUsages, bytes: u64) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hud 2d"),
        size: bytes.max(16),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Hud2dRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat, atlas_size: u32) -> Self {
        let device = &gpu.device;
        let screen = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hud 2d screen"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hud 2d atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hud 2d atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hud 2d"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                        view_dimension: wgpu::TextureViewDimension::D2,
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
            label: Some("hud 2d"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen.as_entire_binding(),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hud 2d"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hud 2d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hud_2d.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HudVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hud 2d"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            screen,
            atlas,
            atlas_size,
            bind_group,
            geometry: buffer(gpu, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX, 0),
            geometry_capacity: 0,
            index_offset: 0,
            index_count: 0,
            screen_size: [1.0, 1.0],
            bytes: Vec::new(),
        }
    }

    /// Uploads the frame (and the atlas when it changed).
    pub fn prepare(&mut self, gpu: &Gpu, hud: &mut Hud2d) {
        if hud.target != self.screen_size {
            self.screen_size = hud.target;
            gpu.queue.write_buffer(
                &self.screen,
                0,
                bytemuck::cast_slice(&[hud.target[0], hud.target[1], 0.0f32, 0.0]),
            );
        }
        if hud.atlas.dirty && hud.atlas.size == self.atlas_size {
            gpu.queue.write_texture(
                self.atlas.as_image_copy(),
                &hud.atlas.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.atlas_size),
                    rows_per_image: Some(self.atlas_size),
                },
                wgpu::Extent3d {
                    width: self.atlas_size,
                    height: self.atlas_size,
                    depth_or_array_layers: 1,
                },
            );
            hud.atlas.dirty = false;
        }
        // Vertices, then the indices at a 4-byte boundary.
        let vertex_bytes: &[u8] = bytemuck::cast_slice(&hud.vertices);
        let index_offset = vertex_bytes.len().next_multiple_of(4);
        self.bytes.clear();
        self.bytes.extend_from_slice(vertex_bytes);
        self.bytes.resize(index_offset, 0);
        self.bytes.extend_from_slice(bytemuck::cast_slice(&hud.indices));
        // `write_buffer` takes whole multiples of 4 bytes.
        self.bytes.resize(self.bytes.len().next_multiple_of(4), 0);
        if self.bytes.len() > self.geometry_capacity {
            self.geometry_capacity = self.bytes.len().next_power_of_two();
            self.geometry = buffer(
                gpu,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX,
                self.geometry_capacity as u64,
            );
        }
        if !hud.indices.is_empty() {
            gpu.queue.write_buffer(&self.geometry, 0, &self.bytes);
        }
        self.index_offset = index_offset as u64;
        self.index_count = hud.indices.len() as u32;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.geometry.slice(..self.index_offset));
        pass.set_index_buffer(self.geometry.slice(self.index_offset..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}
