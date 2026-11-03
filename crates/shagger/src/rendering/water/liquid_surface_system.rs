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
//! Each pool reflects its cached room (`WaterProbeCache`), captured once at
//! load by [`LiquidSurfaceSystem::capture_reflections`]; lava and pools the
//! cache cannot place a probe over reflect the map's sky.
//!
//! Static highlights from the map's lights and emitting panels
//! (`WaterSurfaceLighting`) are traced once at load by
//! [`LiquidSurfaceSystem::prepare_lighting`].
//!
//! Ripples (`WaterDisturbances`) are owned here and snapped to a pool's
//! surface ([`LiquidSurfaceSystem::add_disturbance`]); each surface packs
//! the ones on its patch.
//!
//! A liquid whose shader names a `cubemap` reflects it where no room was
//! captured (`setEnvironmentTexture(texture, { kind })`), the sky elsewhere.
//!
//! Not yet: the actor reflection pass.

use std::collections::HashMap;

use glam::Vec3;
use wgpu::util::DeviceExt;

use super::unified_water_material::{UNIFIED_WATER_WGSL, WaterKind, WaterUniforms, unified_water_uniforms};
use super::water_disturbances::WaterDisturbances;
use super::water_noise::{WATER_NOISE_SIZE, generate_water_normal_map};
use super::water_probe_cache::{PROBE_SIZE, ProbePlan, StaticTriangles, WaterProbeCache};
use super::water_surface_lighting::{MASK_SIZE, WaterMapLight, WaterSurfaceLighting};
use crate::loader::bsp::liquid_classifier::LiquidSurfaceDescriptor;
use crate::rendering::camera::Camera;
use crate::rendering::gpu::Gpu;
use crate::rendering::hdr_target::HDR_FORMAT;
use crate::rendering::sky_renderer::SkyRenderer;
use crate::rendering::world_fog_controller::FogUniforms;
use crate::rendering::world_geometry::{WorldGeometry, WorldVertex};
use crate::rendering::world_renderer::{DEPTH_FORMAT, WorldRenderer};

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

/// One claimed batch: its material's uniforms with its own environment
/// (`probes.bind` per mesh), and their buffer.
struct Surface {
    batch: usize,
    kind: WaterKind,
    first_index: u32,
    index_count: u32,
    uniforms: WaterUniforms,
    buffer: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    /// Its room capture, an index into `probes`.
    probe: Option<usize>,
    /// Its patch's light visibility mask, an index into `masks`.
    mask: Option<usize>,
}

/// A captured room (`CachedWaterProbe`).
struct Probe {
    cube: wgpu::TextureView,
    plan: ProbePlan,
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
    cache: WaterProbeCache,
    probes: Vec<Probe>,
    lighting: WaterSurfaceLighting,
    disturbances: WaterDisturbances,
    masks: Vec<wgpu::TextureView>,
    /// `lighting.bind`'s fallback: one clear texel.
    no_mask: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    noise: wgpu::TextureView,
    noise_sampler: wgpu::Sampler,
    environment: wgpu::TextureView,
    /// Liquids' own cubemaps, by kind.
    kind_environments: Vec<(WaterKind, wgpu::TextureView)>,
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
    /// `claim`: the drawn liquid batches of `geometry`, with the material of
    /// their kind and profile; `probe_lift` is `r_water_probe_height`. `environment` is the map's sky cube
    /// (`setEnvironmentTexture(skyEnvironment)`), gamma space. `None` when
    /// the map draws no liquid.
    #[must_use]
    pub fn new(
        gpu: &Gpu,
        geometry: &WorldGeometry,
        liquids: &[LiquidSurfaceDescriptor],
        environment: Option<&wgpu::TextureView>,
        kind_environments: &[(WaterKind, wgpu::TextureView)],
        color_format: wgpu::TextureFormat,
        probe_lift: f64,
    ) -> Option<Self> {
        let by_texture: HashMap<usize, &LiquidSurfaceDescriptor> = liquids
            .iter()
            .filter(|l| l.drawn)
            .map(|l| (l.texture_index, l))
            .collect();
        let mut claimed = Vec::new();
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
            hidden[batch] = true;
            claimed.push((batch, liquid.kind, profile.to_owned()));
        }
        if claimed.is_empty() {
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
        let surfaces = claimed
            .into_iter()
            .map(|(batch, kind, profile)| {
                let mut uniforms = unified_water_uniforms(kind, &profile, None);
                // `setWaterEnvironment`: the sky cube, skybox mode, gamma space.
                let own = kind_environments.iter().any(|(k, _)| *k == kind);
                uniforms.env[0] = if has_environment || own { 1.0 } else { 0.0 };
                Surface {
                    batch,
                    kind,
                    first_index: geometry.batches[batch].first_index,
                    index_count: geometry.batches[batch].index_count,
                    buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("unified water"),
                        contents: bytemuck::bytes_of(&uniforms),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    }),
                    uniforms,
                    bind_group: None,
                    probe: None,
                    mask: None,
                }
            })
            .collect::<Vec<_>>();
        let reactive: Vec<usize> = surfaces
            .iter()
            .filter(|s| has_reactive_surface(s.kind))
            .map(|s| s.batch)
            .collect();
        let cache = WaterProbeCache::new(geometry, &reactive, probe_lift);
        let no_mask = device
            .create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor {
                    label: Some("water-static-light-visibility"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
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
                &[0, 0, 0, 0],
            )
            .create_view(&wgpu::TextureViewDescriptor::default());
        Some(Self {
            surfaces,
            cache,
            probes: Vec::new(),
            lighting: WaterSurfaceLighting::default(),
            disturbances: WaterDisturbances::default(),
            masks: Vec::new(),
            no_mask,
            pipeline,
            layout,
            noise,
            noise_sampler,
            environment,
            kind_environments: kind_environments.to_vec(),
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

    /// The patches the probe cache found (`patchFor`, `surfaceAt`).
    #[must_use]
    pub fn probe_cache(&self) -> &WaterProbeCache {
        &self.cache
    }

    /// How many rooms were captured.
    #[must_use]
    pub fn probe_count(&self) -> usize {
        self.probes.len()
    }

    /// Where each captured room's probe stands.
    pub fn probe_positions(&self) -> impl Iterator<Item = glam::DVec3> + '_ {
        self.probes.iter().map(|p| p.plan.position)
    }

    /// `captureReflections` (`WaterProbeCache.captureAll`): each pool's room
    /// cubemap, drawn once from its probe point with the static world and
    /// the sky (no liquids, autosprites, models or effects), in linear HDR as
    /// the scene draws for image processing as a post-process. `world` and
    /// `sky` are prepared for each face, so prepare them again afterwards.
    pub fn capture_reflections(
        &mut self,
        gpu: &Gpu,
        geometry: &WorldGeometry,
        world: &WorldRenderer,
        sky: Option<&SkyRenderer>,
        near: f32,
        far: f32,
    ) -> usize {
        let picks = StaticTriangles::new(geometry, &self.hidden);
        let plans = self.cache.plan(&picks);
        // The static meshes: everything but autosprites (liquids are hidden).
        let mask: Vec<bool> = geometry
            .batches
            .iter()
            .map(|b| b.passes.first().is_none_or(|p| !(p.autosprite || p.autosprite2)))
            .collect();
        let projection = glam::camera::lh::proj::directx::perspective(std::f32::consts::FRAC_PI_2, 1.0, near, far);
        // The cube faces in layer order, each looking along its axis with the
        // up that puts its texels where a cube lookup finds them.
        let faces = [
            (Vec3::X, Vec3::Y),
            (Vec3::NEG_X, Vec3::Y),
            (Vec3::Y, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::Z),
            (Vec3::Z, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y),
        ];
        let depth = texture_2d(
            gpu,
            "water probe depth",
            DEPTH_FORMAT,
            (PROBE_SIZE, PROBE_SIZE),
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
        for plan in plans {
            let eye = plan.position.as_vec3();
            let cube = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("water room"),
                size: wgpu::Extent3d {
                    width: PROBE_SIZE,
                    height: PROBE_SIZE,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            for (layer, (forward, up)) in faces.iter().enumerate() {
                let view = glam::camera::lh::view::look_to_mat4(eye, *forward, *up);
                world.prepare(gpu, projection * view, eye);
                if let Some(sky) = sky {
                    sky.prepare_view(gpu, view, projection, eye);
                }
                let target = cube.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer as u32,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("water room"),
                });
                {
                    let mut pass =
                        crate::rendering::offscreen::begin_main_pass(&mut encoder, &target, &depth, wgpu::Color::BLACK);
                    if let Some(sky) = sky {
                        sky.draw(&mut pass);
                    }
                    world.draw_visible(&mut pass, &mask);
                }
                gpu.queue.submit([encoder.finish()]);
            }
            let index = self.probes.len();
            for surface in &mut self.surfaces {
                if self.cache.patch_for(surface.batch) == Some(plan.patch) {
                    surface.probe = Some(index);
                    // `probes.bind`: the room, linear, with its parallax box.
                    let u = &mut surface.uniforms;
                    u.env = [1.0, 1.0, 1.0, 1.0];
                    u.probe_position = [eye.x, eye.y, eye.z, 0.0];
                    u.probe_min = plan.room_min.as_vec3().extend(0.0).to_array();
                    u.probe_max = plan.room_max.as_vec3().extend(0.0).to_array();
                }
            }
            self.probes.push(Probe {
                cube: cube.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    ..Default::default()
                }),
                plan,
            });
        }
        self.rebind(gpu);
        self.probes.len()
    }

    /// `lighting.prepare` and `bind`: the static highlights of each reactive
    /// pool from `sources` (`waterMapLights` and `waterSurfaceLights`), with
    /// their visibility traced through the static opaque world. Returns
    /// `lightCount`.
    pub fn prepare_lighting(&mut self, gpu: &Gpu, geometry: &WorldGeometry, sources: &[WaterMapLight]) -> usize {
        let patches: Vec<(usize, &super::water_probe_cache::WaterPatch)> = self
            .surfaces
            .iter()
            .filter(|s| has_reactive_surface(s.kind))
            .filter_map(|s| self.cache.patch_for(s.batch))
            .map(|p| (p, &self.cache.patches[p]))
            .collect();
        let hidden = &self.hidden;
        self.lighting.prepare(
            || StaticTriangles::new(geometry, hidden).opaque_triangles(),
            &patches,
            sources,
        );
        let mut views: Vec<(usize, usize)> = Vec::new();
        for surface in &mut self.surfaces {
            let patch = has_reactive_surface(surface.kind)
                .then(|| self.cache.patch_for(surface.batch))
                .flatten();
            let u = &mut surface.uniforms;
            u.lights = [[0.0; 4]; super::unified_water_material::MAX_WATER_LIGHTS];
            u.light_colours = u.lights;
            u.light_mask_min = [0.0; 4];
            u.light_mask_max = [1.0, 1.0, 1.0, 0.0];
            surface.mask = None;
            let Some(patch) = patch else { continue };
            let bounds = &self.cache.patches[patch];
            u.light_mask_min = bounds.min.as_vec3().extend(0.0).to_array();
            u.light_mask_max = bounds.max.as_vec3().extend(0.0).to_array();
            let Some(entry) = self.lighting.entry(patch) else {
                continue;
            };
            u.lights = entry.lights;
            u.light_colours = entry.colours;
            let index = match views.iter().find(|(p, _)| *p == patch) {
                Some((_, index)) => *index,
                None => {
                    let view = gpu
                        .device
                        .create_texture_with_data(
                            &gpu.queue,
                            &wgpu::TextureDescriptor {
                                label: Some("water-static-light-visibility"),
                                size: wgpu::Extent3d {
                                    width: MASK_SIZE as u32,
                                    height: MASK_SIZE as u32,
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
                            &entry.mask,
                        )
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    self.masks.push(view);
                    views.push((patch, self.masks.len() - 1));
                    self.masks.len() - 1
                }
            };
            surface.mask = Some(index);
        }
        self.rebind(gpu);
        self.lighting.light_count()
    }

    /// The wave clock: wall time, a hitch capped at a tenth of a second.
    pub fn update(&mut self, dt_seconds: f64) {
        let dt = dt_seconds.min(0.1);
        self.elapsed += dt;
        self.disturbances.update(dt);
    }

    /// `waterDisturbancesFor(scene).add` with this system's
    /// `resolveSurface` (`probes.surfaceAt`): a ripple on the pool under or
    /// over `position`, `false` when there is none.
    pub fn add_disturbance(
        &mut self,
        position: glam::DVec3,
        strength: f64,
        radius: f64,
        direction: Option<(f64, f64)>,
    ) -> bool {
        let cache = &self.cache;
        let resolve = |p| cache.surface_at(p);
        self.disturbances
            .add(position, strength, radius, direction, Some(&resolve))
    }

    /// The ripples, for diagnostics.
    #[must_use]
    pub fn disturbances(&self) -> &WaterDisturbances {
        &self.disturbances
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
        self.rebind(gpu);
    }

    fn rebind(&mut self, gpu: &Gpu) {
        let Some(captures) = self.captures.as_ref() else { return };
        for surface in &mut self.surfaces {
            let fallback = self
                .kind_environments
                .iter()
                .find(|(k, _)| *k == surface.kind)
                .map_or(&self.environment, |(_, view)| view);
            let environment = surface.probe.map_or(fallback, |p| &self.probes[p].cube);
            let mask = surface.mask.map_or(&self.no_mask, |m| &self.masks[m]);
            surface.bind_group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("unified water"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: surface.buffer.as_entire_binding(),
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
                        resource: wgpu::BindingResource::TextureView(environment),
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
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(mask),
                    },
                    // The actor layer is not drawn yet: the noise stands in.
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
        for surface in &mut self.surfaces {
            let u = &mut surface.uniforms;
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
            // Membership comes from the surface's own patch, not its kind.
            let patch = has_reactive_surface(surface.kind)
                .then(|| self.cache.patch_for(surface.batch))
                .flatten()
                .map(|p| &self.cache.patches[p]);
            u.impulses = self
                .disturbances
                .pack(|p| patch.is_some_and(|patch| patch.contains(p.x, p.y, p.z)));
            gpu.queue
                .write_buffer(&surface.buffer, 0, bytemuck::bytes_of(&surface.uniforms));
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
        for surface in &self.surfaces {
            if !visible.get(surface.batch).copied().unwrap_or(true) {
                continue;
            }
            let Some(bind_group) = &surface.bind_group else {
                continue;
            };
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw_indexed(surface.first_index..surface.first_index + surface.index_count, 0, 0..1);
        }
    }
}
