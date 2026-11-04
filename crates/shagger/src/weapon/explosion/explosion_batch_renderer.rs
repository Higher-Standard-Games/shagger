//! SHAG `src/engine/weapon/explosion/ExplosionBatchRenderer.ts`: the three
//! instanced layers, and `spawn`, which writes an explosion's instances
//! once; the shader animates them from then on.
//!
//! Randomness comes from the caller, in `Math.random`'s draw order, so a
//! test can replay the browser's.

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::explosion_batch_material::{
    EXPLOSION_BATCH_WGSL, EXPLOSION_FIRE_TIME, ExplosionLayer, SPARK_AGE_PER_SECOND,
};
use crate::babylon::meshes::ico_sphere_builder::create_ico_sphere;
use crate::babylon::meshes::mesh_builder::create_cylinder;
use crate::rendering::gpu::Gpu;
use crate::rendering::world_fog_controller::FogUniforms;
use crate::rendering::world_renderer::DEPTH_FORMAT;

/// What one explosion looks like; `ExplosionConfig` supplies it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExplosionLook {
    pub scale: f64,
    pub speed: f64,
    pub core_color: Vec3,
    pub outer_color: Vec3,
    pub smoke_color: Vec3,
    /// Rays per explosion, drawn evenly from `[min, max]`.
    pub rays: (u32, u32),
    /// `shakeIntensity`: the camera shake's multiplier, 0 for none.
    pub shake_intensity: f64,
    /// `shakeMaxRange`: how far the shake reaches, game units.
    pub shake_max_range: f64,
}

/// Explosions alive at once before the oldest is recycled.
const MAX_EXPLOSIONS: usize = 96;
const MAX_RAYS: usize = 12;
/// The particle system the sparks reproduce held at most 40.
pub const SPARKS_PER_EXPLOSION: usize = 40;
/// How long an explosion lives, in its own time; sparks still alive are cut
/// there.
pub const EXPLOSION_LIFETIME: f64 = 0.9;

/// One instance: five vec4s (`iA`..`iE`).
type Instance = [f32; 20];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    time: [f32; 4],
    eye: [f32; 4],
    fog: FogUniforms,
}

/// One instanced layer. Slots are written once at spawn; expiry swaps the
/// last slot into the freed index.
struct LayerBatch {
    capacity: usize,
    expiry: Vec<f64>,
    data: Vec<Instance>,
    dirty: bool,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    instances: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
}

impl LayerBatch {
    fn remove_at(&mut self, index: usize) {
        self.expiry.swap_remove(index);
        self.data.swap_remove(index);
        self.dirty = true;
    }

    fn push(&mut self, values: Instance, expires_at: f64) {
        if self.expiry.len() >= self.capacity {
            let mut soonest = 0;
            for i in 1..self.expiry.len() {
                if self.expiry[i] < self.expiry[soonest] {
                    soonest = i;
                }
            }
            self.remove_at(soonest);
        }
        self.expiry.push(expires_at);
        self.data.push(values);
        self.dirty = true;
    }

    fn update(&mut self, now: f64) {
        for i in (0..self.expiry.len()).rev() {
            if now >= self.expiry[i] {
                self.remove_at(i);
            }
        }
    }
}

/// Stores four values at `at` as `Float32Array` does.
fn set(v: &mut Instance, at: usize, values: [f64; 4]) {
    for (i, value) in values.into_iter().enumerate() {
        v[at + i] = value as f32;
    }
}

/// Every live explosion's fireball, rays and sparks, drawn as three
/// instanced meshes animated in the vertex shader.
pub struct ExplosionBatchRenderer {
    layers: [LayerBatch; 3],
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    fog: FogUniforms,
}

/// The fireball's sphere; the shader pushes its spikes out per explosion
/// size.
fn fireball_shape() -> (Vec<Vertex>, Vec<u32>) {
    let sphere = create_ico_sphere(2.5, 3);
    let vertices = sphere
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| Vertex {
            position: [p[0], p[1], p[2]],
            uv: [0.0; 2],
        })
        .collect();
    (vertices, sphere.indices)
}

fn ray_shape() -> (Vec<Vertex>, Vec<u32>) {
    let cone = create_cylinder(1.0, 0.0, 1.0, 6);
    let vertices = cone
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .zip(cone.uvs.as_chunks::<2>().0.iter())
        .map(|(p, t)| Vertex {
            position: [p[0], p[1], p[2]],
            uv: [t[0], t[1]],
        })
        .collect();
    (vertices, cone.indices)
}

fn spark_quad() -> (Vec<Vertex>, Vec<u32>) {
    let corners = [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let vertices = corners
        .iter()
        .zip(uvs)
        .map(|(c, uv)| Vertex {
            position: [c[0], c[1], 0.0],
            uv,
        })
        .collect();
    (vertices, vec![0, 1, 2, 2, 1, 3])
}

impl ExplosionBatchRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("explosion globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("explosion globals"),
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
            label: Some("explosion globals"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("explosion"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("explosion_batch.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}{EXPLOSION_BATCH_WGSL}", include_str!("../../rendering/fog.wgsl")).into(),
            ),
        });
        let layer = |layer: ExplosionLayer, (vertices, indices): (Vec<Vertex>, Vec<u32>), capacity: usize| {
            let pipeline = Self::pipeline(gpu, &layout, &shader, color_format, layer);
            LayerBatch {
                capacity,
                expiry: Vec::new(),
                data: Vec::new(),
                dirty: false,
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("explosion vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_count: indices.len() as u32,
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("explosion indices"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                instances: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("explosion instances"),
                    size: (capacity * std::mem::size_of::<Instance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                pipeline,
            }
        };
        Self {
            layers: [
                layer(ExplosionLayer::Fireball, fireball_shape(), MAX_EXPLOSIONS),
                layer(ExplosionLayer::Ray, ray_shape(), MAX_EXPLOSIONS * MAX_RAYS),
                layer(
                    ExplosionLayer::Spark,
                    spark_quad(),
                    MAX_EXPLOSIONS * SPARKS_PER_EXPLOSION,
                ),
            ],
            fog: FogUniforms::default(),
            globals,
            bind_group,
        }
    }

    fn pipeline(
        gpu: &Gpu,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        color_format: wgpu::TextureFormat,
        layer: ExplosionLayer,
    ) -> wgpu::RenderPipeline {
        let blend = if layer.additive() {
            // ALPHA_ADD
            wgpu::BlendState {
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
            }
        } else {
            // ALPHA_COMBINE
            wgpu::BlendState::ALPHA_BLENDING
        };
        let vertex_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        let instance_attributes =
            wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vertex_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Instance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attributes,
            }),
        ];
        gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("explosion"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some(layer.vertex_entry()),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Babylon's builders wind front faces counter-clockwise on
                // screen in its left-handed view.
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: layer.back_face_culling().then_some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // Babylon draws the blended layers in its transparent pass,
                // whose alpha mode turns depth writes off.
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(if layer.additive() { "fs_spark" } else { "fs_fire" }),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// `spawn`: one explosion's fireball, rays and sparks at `now` seconds,
    /// the fireball spreading across the hit surface's `normal`. `random`
    /// stands for `Math.random`.
    pub fn spawn(
        &mut self,
        now: f64,
        position: DVec3,
        look: &ExplosionLook,
        normal: DVec3,
        random: &mut impl FnMut() -> f64,
    ) {
        let ExplosionLook {
            scale,
            speed,
            core_color: core,
            outer_color: outer,
            smoke_color: smoke,
            rays,
            ..
        } = *look;
        let fire_end = now + EXPLOSION_FIRE_TIME / speed;
        let mut v: Instance = [0.0; 20];
        let (core, outer, smoke) = (core.as_dvec3(), outer.as_dvec3(), smoke.as_dvec3());
        set(&mut v, 0, [position.x, position.y, position.z, now]);
        set(&mut v, 4, [normal.x, normal.y, normal.z, speed]);
        set(&mut v, 8, [core.x, core.y, core.z, scale]);
        set(&mut v, 12, [outer.x, outer.y, outer.z, 0.0]);
        set(&mut v, 16, [smoke.x, smoke.y, smoke.z, 0.0]);
        self.layers[0].push(v, fire_end);

        let ray_count = f64::from(rays.0) + (random() * f64::from(rays.1 - rays.0 + 1)).floor();
        for _ in 0..(ray_count as usize).min(MAX_RAYS) {
            let theta = random() * std::f64::consts::PI * 2.0;
            let phi = (2.0 * random() - 1.0).acos();
            let length = (2.0 + random() * 3.0) * scale;
            let width = (0.3 + random() * 0.3) * scale;
            set(
                &mut v,
                4,
                [phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos(), speed],
            );
            set(&mut v, 12, [outer.x, outer.y, outer.z, width]);
            set(&mut v, 16, [length, 0.0, 0.0, 0.0]);
            self.layers[1].push(v, fire_end);
        }

        // Sparks: a burst from inside a sphere, flying out from its centre.
        let radius = 0.5 * scale;
        let spark_cut = now + EXPLOSION_LIFETIME / speed;
        set(&mut v, 16, [0.0; 4]);
        for _ in 0..SPARKS_PER_EXPLOSION {
            let r = radius - random() * radius;
            let phi = random() * std::f64::consts::PI * 2.0;
            let theta = (2.0 * random() - 1.0).acos();
            let mut dx = phi.cos() * theta.sin();
            let mut dy = theta.cos();
            let mut dz = phi.sin() * theta.sin();
            let mut length = (dx * dx + dy * dy + dz * dz).sqrt();
            if length == 0.0 || length.is_nan() {
                length = 1.0;
            }
            dx /= length;
            dy /= length;
            dz /= length;
            let power = (15.0 + random() * 20.0) * scale;
            let life = 0.2 + random() * 0.3;
            let size = (0.15 + random() * 0.25) * scale;
            let mix = random();
            set(
                &mut v,
                0,
                [position.x + dx * r, position.y + dy * r, position.z + dz * r, now],
            );
            set(&mut v, 4, [dx * power, dy * power, dz * power, life]);
            set(
                &mut v,
                8,
                [
                    core.x + (outer.x - core.x) * mix,
                    core.y + (outer.y - core.y) * mix,
                    core.z + (outer.z - core.z) * mix,
                    size,
                ],
            );
            set(&mut v, 12, [smoke.x, smoke.y, smoke.z, 0.0]);
            self.layers[2].push(v, (now + life / SPARK_AGE_PER_SECOND).min(spark_cut));
        }
    }

    /// `update`: drops what has expired by `now`.
    pub fn update(&mut self, now: f64) {
        for layer in &mut self.layers {
            layer.update(now);
        }
    }

    /// Writes the camera, the clock and any changed instances.
    /// The world fog the explosions draw in, from the next `prepare`.
    pub fn set_fog(&mut self, fog: FogUniforms) {
        self.fog = fog;
    }

    pub fn prepare(&mut self, gpu: &Gpu, view_proj: Mat4, view: Mat4, now: f64) {
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            view: view.to_cols_array_2d(),
            time: [now as f32, 0.0, 0.0, 0.0],
            eye: {
                let eye = view.inverse().w_axis;
                [eye.x, eye.y, eye.z, 0.0]
            },
            fog: self.fog,
        };
        gpu.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        for layer in &mut self.layers {
            if layer.dirty && !layer.data.is_empty() {
                gpu.queue
                    .write_buffer(&layer.instances, 0, bytemuck::cast_slice(&layer.data));
            }
            layer.dirty = false;
        }
    }

    /// Fireballs, then rays, then sparks (alpha indices 44, 45 and 46).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        for layer in &self.layers {
            if layer.data.is_empty() {
                continue;
            }
            pass.set_pipeline(&layer.pipeline);
            pass.set_vertex_buffer(0, layer.vertices.slice(..));
            pass.set_vertex_buffer(1, layer.instances.slice(..));
            pass.set_index_buffer(layer.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..layer.index_count, 0, 0..layer.data.len() as u32);
        }
    }

    /// Live instances per layer, for tests and diagnostics.
    #[must_use]
    pub fn debug_counts(&self) -> [usize; 3] {
        self.layers.each_ref().map(|l| l.data.len())
    }

    /// `clear`.
    pub fn clear(&mut self) {
        for layer in &mut self.layers {
            layer.expiry.clear();
            layer.data.clear();
            layer.dirty = false;
        }
    }
}
