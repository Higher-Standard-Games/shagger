//! Port of SHAG `src/engine/rendering/water/LiquidSurfaceSystem.ts`: the
//! map's owner of every liquid surface. It claims the world batches the
//! classifier calls liquid (the world renderer then leaves them out), draws
//! them with the unified water material, and feeds its per-frame uniforms
//! from the `r_water_*` cvars.
//!
//! The TypeScript renders its own depth pass (quality 2) and a
//! half-resolution colour capture (quality 3) with the water left out. Here
//! the water draws after the opaque pass, so both are copies of the HDR
//! target taken between the two passes: hardware depth and linear colour.
//!
//! Not yet: the cached room probes (`WaterProbeCache`, so every pool
//! reflects the map's sky), static light highlights (`WaterSurfaceLighting`),
//! the actor reflection pass, disturbances from bodies in the water, and
//! per-liquid environment cubemaps from the shaders.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use super::unified_water_material::{UNIFIED_WATER_WGSL, WaterKind, WaterUniforms, unified_water_uniforms};
use super::water_noise::{WATER_NOISE_SIZE, generate_water_normal_map};
use crate::loader::bsp::liquid_classifier::LiquidSurfaceDescriptor;
use crate::rendering::camera::Camera;
use crate::rendering::gpu::Gpu;
use crate::rendering::world_fog_controller::FogUniforms;
use crate::rendering::world_geometry::{WorldGeometry, WorldVertex};
use crate::rendering::world_renderer::DEPTH_FORMAT;

/// `hasReactiveSurface`: water and slime ripple; lava crusts.
#[must_use]
pub fn has_reactive_surface(kind: WaterKind) -> bool {
    kind != WaterKind::Lava
}

/// The `r_water_*` cvars the frame reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterSettings {
    pub quality: f64,
    pub debug: f64,
    pub highlights: f64,
    pub roughness: f64,
    pub reflection: f64,
    pub blur: f64,
    pub detail: f64,
    pub contact: f64,
}

impl Default for WaterSettings {
    fn default() -> Self {
        Self {
            quality: 2.0,
            debug: 0.0,
            highlights: 0.6,
            roughness: 0.3,
            reflection: 1.3,
            blur: 0.3,
            detail: 0.8,
            contact: 0.25,
        }
    }
}

struct Material {
    uniforms: WaterUniforms,
    buffer: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
}

struct Surface {
    batch: usize,
    material: usize,
    first_index: u32,
    index_count: u32,
}

/// The copies of the scene behind the water.
struct Captures {
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    colour: wgpu::Texture,
    colour_view: wgpu::TextureView,
    size: (u32, u32),
}

pub struct LiquidSurfaceSystem {
    surfaces: Vec<Surface>,
    materials: Vec<Material>,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    noise: wgpu::TextureView,
    noise_sampler: wgpu::Sampler,
    environment: wgpu::TextureView,
    environment_sampler: wgpu::Sampler,
    capture_sampler: wgpu::Sampler,
    has_environment: bool,
    captures: Option<Captures>,
    hidden: Vec<bool>,
    elapsed: f64,
    /// Whether this frame copies the scene (`r_water_quality` 2 and up, with
    /// water on screen).
    wants_capture: bool,
}

fn texture_2d(
    gpu: &Gpu,
    label: &str,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

impl LiquidSurfaceSystem {
    /// `claim`: the drawn liquid batches of `geometry`, one material per
    /// kind and profile. `environment` is the map's sky cube
    /// (`setEnvironmentTexture(skyEnvironment)`), gamma space. `None` when
    /// the map draws no liquid.
    #[must_use]
    pub fn new(
        gpu: &Gpu,
        geometry: &WorldGeometry,
        liquids: &[LiquidSurfaceDescriptor],
        environment: Option<&wgpu::TextureView>,
        color_format: wgpu::TextureFormat,
    ) -> Option<Self> {
        let by_texture: HashMap<usize, &LiquidSurfaceDescriptor> = liquids
            .iter()
            .filter(|l| l.drawn)
            .map(|l| (l.texture_index, l))
            .collect();
        let mut keys: Vec<(WaterKind, String)> = Vec::new();
        let mut surfaces = Vec::new();
        let mut hidden = vec![false; geometry.batches.len()];
        for (batch, b) in geometry.batches.iter().enumerate() {
            let Ok(index) = usize::try_from(b.texture_index) else {
                continue;
            };
            let Some(liquid) = by_texture.get(&index) else { continue };
            let requested = liquid.profile.as_deref().unwrap_or("default");
            let profile = if liquid.kind == WaterKind::Water && matches!(requested, "clear" | "deep") {
                requested
            } else {
                "default"
            };
            let key = (liquid.kind, profile.to_owned());
            let material = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                keys.push(key);
                keys.len() - 1
            });
            hidden[batch] = true;
            surfaces.push(Surface {
                batch,
                material,
                first_index: b.first_index,
                index_count: b.index_count,
            });
        }
        if surfaces.is_empty() {
            return None;
        }
        let device = &gpu.device;
        let noise_texture = device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("unified-water-noise"),
                size: wgpu::Extent3d {
                    width: WATER_NOISE_SIZE as u32,
                    height: WATER_NOISE_SIZE as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &generate_water_normal_map(WATER_NOISE_SIZE, 2.0),
        );
        let noise = noise_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let noise_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("unified-water-noise"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let linear = |label| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        };
        let environment_sampler = linear("water environment");
        let capture_sampler = linear("water capture");
        // `waterFallbackCube`: one dark slate texel per face.
        let fallback = device
            .create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor {
                    label: Some("water-fallback-cube"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
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
                &[45, 60, 70, 255].repeat(6),
            )
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
        let has_environment = environment.is_some();
        let environment = environment.cloned().unwrap_or(fallback);

        let texture = |binding, sample_type, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let d2 = wgpu::TextureViewDimension::D2;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("unified water"),
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
                texture(1, float, d2),
                sampler(2),
                texture(3, float, wgpu::TextureViewDimension::Cube),
                sampler(4),
                texture(5, wgpu::TextureSampleType::Depth, d2),
                texture(6, float, d2),
                sampler(7),
                texture(8, float, d2),
                texture(9, float, d2),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("unified_water.wgsl"),
            source: wgpu::ShaderSource::Wgsl(UNIFIED_WATER_WGSL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("unified water"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("unified water"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<WorldVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
                })],
            },
            // Liquid is drawn from below as often as from above.
            primitive: wgpu::PrimitiveState::default(),
            // Transparent: no depth writes, or what lies under it vanishes.
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let materials = keys
            .iter()
            .map(|(kind, profile)| {
                let mut uniforms = unified_water_uniforms(*kind, profile, None);
                // `setWaterEnvironment`: the sky cube, skybox mode, gamma space.
                uniforms.env[0] = if has_environment { 1.0 } else { 0.0 };
                Material {
                    buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("unified water"),
                        contents: bytemuck::bytes_of(&uniforms),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    }),
                    uniforms,
                    bind_group: None,
                }
            })
            .collect();
        Some(Self {
            surfaces,
            materials,
            pipeline,
            layout,
            noise,
            noise_sampler,
            environment,
            environment_sampler,
            capture_sampler,
            has_environment,
            captures: None,
            hidden,
            elapsed: 0.0,
            wants_capture: false,
        })
    }

    /// The world batches the water draws, which the world renderer leaves out.
    #[must_use]
    pub fn hidden_batches(&self) -> &[bool] {
        &self.hidden
    }

    /// `claimedCount`.
    #[must_use]
    pub fn claimed_count(&self) -> usize {
        self.surfaces.len()
    }

    #[must_use]
    pub fn has_environment(&self) -> bool {
        self.has_environment
    }

    /// The wave clock: wall time, a hitch capped at a tenth of a second.
    pub fn update(&mut self, dt_seconds: f64) {
        self.elapsed += dt_seconds.min(0.1);
    }

    fn ensure_captures(&mut self, gpu: &Gpu, size: (u32, u32)) {
        if self.captures.as_ref().is_some_and(|c| c.size == size) {
            return;
        }
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let depth = texture_2d(gpu, "water scene depth", DEPTH_FORMAT, size, usage);
        let colour = texture_2d(
            gpu,
            "water scene colour",
            crate::rendering::hdr_target::HDR_FORMAT,
            size,
            usage,
        );
        self.captures = Some(Captures {
            depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
            colour_view: colour.create_view(&wgpu::TextureViewDescriptor::default()),
            depth,
            colour,
            size,
        });
        let captures = self.captures.as_ref().expect("set above");
        for material in &mut self.materials {
            material.bind_group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("unified water"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: material.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.noise),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.noise_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&self.environment),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.environment_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&captures.depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&captures.colour_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::Sampler(&self.capture_sampler),
                    },
                    // The visibility mask and actor layer are not drawn yet:
                    // the noise stands in, as the TypeScript binds it.
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(&self.noise),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::TextureView(&self.noise),
                    },
                ],
            }));
        }
    }

    /// The per-frame uniforms (`start`'s observer): camera, clock, cvars,
    /// and whether depth and refraction are bound. `visible` is the world
    /// culling's batch mask (empty: all drawn).
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        camera: &Camera,
        size: (u32, u32),
        settings: &WaterSettings,
        fog: FogUniforms,
        visible: &[bool],
    ) {
        self.ensure_captures(gpu, size);
        let on_screen = self
            .surfaces
            .iter()
            .any(|s| visible.get(s.batch).copied().unwrap_or(true));
        let depth = on_screen && settings.quality >= 2.0;
        let refraction = depth && settings.quality >= 3.0;
        self.wants_capture = depth;
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let view = camera.view();
        let forward = camera.forward();
        let f = |x: f64| x as f32;
        for material in &mut self.materials {
            let u = &mut material.uniforms;
            u.view_proj = camera.view_projection(aspect).to_cols_array_2d();
            u.view = view.to_cols_array_2d();
            u.camera = [camera.position.x, camera.position.y, camera.position.z, f(self.elapsed)];
            u.forward = [forward.x, forward.y, forward.z, camera.far];
            u.screen = [size.0 as f32, size.1 as f32, f(settings.debug), 1.0];
            u.flags[0] = if depth { 1.0 } else { 0.0 };
            u.flags[1] = if refraction { 1.0 } else { 0.0 };
            u.flags[2] = 1.0;
            u.foam[3] = f(settings.highlights);
            u.tuning = [
                f(settings.roughness),
                f(settings.reflection),
                f(settings.blur),
                f(settings.detail),
            ];
            u.actor_flags[0] = f(settings.contact);
            // Near, far, depth 0 to 1, and the hardware depth of the capture.
            u.depth_projection = [camera.near, camera.far, 1.0, 1.0];
            u.fog = fog;
            gpu.queue
                .write_buffer(&material.buffer, 0, bytemuck::bytes_of(&material.uniforms));
        }
    }

    /// Copies the scene behind the water (the HDR target after the opaque
    /// pass) when this frame needs it.
    pub fn capture(&self, encoder: &mut wgpu::CommandEncoder, color: &wgpu::Texture, depth: &wgpu::Texture) {
        let Some(captures) = self.captures.as_ref().filter(|_| self.wants_capture) else {
            return;
        };
        let extent = wgpu::Extent3d {
            width: captures.size.0.max(1),
            height: captures.size.1.max(1),
            depth_or_array_layers: 1,
        };
        if color.size() != extent || depth.size() != extent {
            return;
        }
        encoder.copy_texture_to_texture(color.as_image_copy(), captures.colour.as_image_copy(), extent);
        encoder.copy_texture_to_texture(depth.as_image_copy(), captures.depth.as_image_copy(), extent);
    }

    /// Draws the claimed surfaces from the world's buffers, those `visible`
    /// keeps (empty: all).
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        vertices: &wgpu::Buffer,
        indices: &wgpu::Buffer,
        visible: &[bool],
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        let mut bound = None;
        for surface in &self.surfaces {
            if !visible.get(surface.batch).copied().unwrap_or(true) {
                continue;
            }
            let Some(bind_group) = &self.materials[surface.material].bind_group else {
                continue;
            };
            if bound != Some(surface.material) {
                pass.set_bind_group(0, bind_group, &[]);
                bound = Some(surface.material);
            }
            pass.draw_indexed(surface.first_index..surface.first_index + surface.index_count, 0, 0..1);
        }
    }
}
