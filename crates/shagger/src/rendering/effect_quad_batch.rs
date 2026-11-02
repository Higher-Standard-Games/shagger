//! The GPU half of SHAG's instanced quad effects (bullet streaks, muzzle
//! flashes, impact sparks): what Babylon does for a thin-instanced quad mesh
//! with a `ShaderMaterial`. One quad, four vec4s per instance, the uniforms
//! those materials take, premultiplied alpha (`ALPHA_PREMULTIPLIED`), no
//! depth writes, both sides. Each effect's own shader follows
//! `effect_quad.wgsl`.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::gpu::Gpu;
use super::world_renderer::DEPTH_FORMAT;

/// One instance: four vec4s.
pub type EffectInstance = [f32; 16];

/// The shared shader half: uniforms, quad input and helpers.
pub const EFFECT_QUAD_WGSL: &str = include_str!("effect_quad.wgsl");

/// What a frame gives every effect.
#[derive(Debug, Clone, Copy)]
pub struct EffectFrame {
    pub view_proj: Mat4,
    pub view: Mat4,
    pub projection: Mat4,
    pub camera_position: Vec3,
    /// The render target's size in pixels (`uViewport`).
    pub viewport: [f32; 2],
    /// The effects' clock in seconds (`uTime`).
    pub time: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    projection: [[f32; 4]; 4],
    camera: [f32; 4],
    viewport_time: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 3],
    uv: [f32; 2],
}

/// One effect's quad, instance buffer and pipeline.
pub struct EffectQuadBatch {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instances: wgpu::Buffer,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: usize,
    count: u32,
}

impl EffectQuadBatch {
    /// `effect_wgsl` is the effect's own shader, with `vs_main` and
    /// `fs_main`.
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat, label: &str, effect_wgsl: &str, capacity: usize) -> Self {
        let device = &gpu.device;
        // uv in [-1, 1]; x runs tail to head on the streaks.
        let quad = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]].map(|[x, y]| QuadVertex {
            position: [x, y, 0.0],
            uv: [x, y],
        });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(&[0u32, 1, 2, 2, 1, 3]),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (capacity.max(1) * std::mem::size_of::<EffectInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(format!("{EFFECT_QUAD_WGSL}\n{effect_wgsl}").into()),
        });
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<QuadVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<EffectInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4],
            }),
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    // ALPHA_PREMULTIPLIED: ONE, ONE_MINUS_SRC_ALPHA; alpha
                    // ONE, ONE.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            vertices,
            indices,
            instances,
            globals,
            bind_group,
            capacity: capacity.max(1),
            count: 0,
        }
    }

    /// Writes the frame's uniforms and, when `dirty`, the live instances.
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, instances: &[EffectInstance], dirty: bool) {
        let c = frame.camera_position;
        let globals = Globals {
            view_proj: frame.view_proj.to_cols_array_2d(),
            view: frame.view.to_cols_array_2d(),
            projection: frame.projection.to_cols_array_2d(),
            camera: [c.x, c.y, c.z, 0.0],
            viewport_time: [
                frame.viewport[0].max(1.0),
                frame.viewport[1].max(1.0),
                frame.time as f32,
                0.0,
            ],
        };
        gpu.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let live = &instances[..instances.len().min(self.capacity)];
        if dirty && !live.is_empty() {
            gpu.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(live));
        }
        self.count = live.len() as u32;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..6, 0, 0..self.count);
    }
}

/// Index-addressed slots, swap-removed on expiry, the oldest recycled when
/// full: the machinery the three effects share.
#[derive(Debug, Default)]
pub struct EffectSlots {
    /// Spawn time and lifetime per slot.
    pub slots: Vec<(f64, f64)>,
    pub data: Vec<EffectInstance>,
    pub dirty: bool,
    capacity: usize,
}

impl EffectSlots {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ..Self::default()
        }
    }

    pub fn remove_at(&mut self, index: usize) {
        self.slots.swap_remove(index);
        self.data.swap_remove(index);
        self.dirty = true;
    }

    /// `oldestIndex`: the first slot with the earliest spawn time.
    #[must_use]
    pub fn oldest_index(&self) -> usize {
        let mut oldest = 0;
        for i in 1..self.slots.len() {
            if self.slots[i].0 < self.slots[oldest].0 {
                oldest = i;
            }
        }
        oldest
    }

    /// Writes one instance, recycling the oldest slot when full.
    pub fn push(&mut self, spawn_time: f64, duration: f64, instance: EffectInstance) {
        if self.slots.len() >= self.capacity {
            self.remove_at(self.oldest_index());
        }
        self.slots.push((spawn_time, duration));
        self.data.push(instance);
        self.dirty = true;
    }

    /// Drops every slot whose age has reached its lifetime.
    pub fn expire(&mut self, now: f64) {
        for i in (0..self.slots.len()).rev() {
            let (spawn, duration) = self.slots[i];
            if now - spawn >= duration {
                self.remove_at(i);
            }
        }
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.data.clear();
        self.dirty = false;
    }
}

/// Packs four vec4s as `Float32Array` stores them.
#[must_use]
pub fn instance(values: [[f64; 4]; 4]) -> EffectInstance {
    let mut out = [0.0f32; 16];
    for (i, v) in values.iter().flatten().enumerate() {
        out[i] = *v as f32;
    }
    out
}
