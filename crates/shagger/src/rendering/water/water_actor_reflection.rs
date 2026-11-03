//! Port of SHAG `src/engine/rendering/water/WaterActorReflection.ts`: the
//! water's short-range reflection of players. Each frame, near a pool above
//! the camera's feet, the nearest two players within reach are drawn
//! mirrored in the pool's plane into a small texture, with the map's opaque
//! surfaces above the water drawn black and transparent first so a wall
//! between a player and the pool hides their reflection. The water blends it
//! over the room's cubemap where it covers.
//!
//! What changed: Babylon's `MirrorTexture` mirrors the view and flips the
//! face winding; here the mirrored view is also flipped left to right in
//! clip space, so faces keep their winding and the models' own pipelines
//! draw it, and the matrix the water samples with undoes the flip (and
//! wgpu's downward texture rows). The caller hands over each player's box
//! and part count instead of the scene's tagged meshes, and decides which
//! of its model draws belong to the players chosen. Babylon's mirror clips
//! at the water plane; the players here are not clipped (the blockers are),
//! and the local player's reflection-only body is not drawn.

use glam::{DVec3, Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::water_probe_cache::WaterPatch;
use crate::rendering::camera::Camera;
use crate::rendering::frustum::Frustum;
use crate::rendering::gpu::Gpu;
use crate::rendering::hdr_target::HDR_FORMAT;
use crate::rendering::model_renderer::ModelRenderer;
use crate::rendering::world_geometry::{WorldGeometry, WorldVertex};
use crate::rendering::world_renderer::DEPTH_FORMAT;

/// `ACTOR_REFLECTION_RANGE`: the camera must be this close to a pool.
pub const ACTOR_REFLECTION_RANGE: f64 = 240.0;
/// A player this far from the pool is not reflected.
pub const ACTOR_RANGE: f64 = 160.0;
const MAX_ACTORS: usize = 2;
const MAX_ACTOR_MESHES: usize = 32;
const MAX_OCCLUDER_DRAWS: usize = 256;

/// `distanceToWater`: from `p` to the pool's box, measured to its surface.
#[must_use]
pub fn distance_to_water(p: DVec3, patch: &WaterPatch) -> f64 {
    let dx = (patch.min.x - p.x).max(0.0).max(p.x - patch.max.x);
    let dy = p.y - patch.min.y;
    let dz = (patch.min.z - p.z).max(0.0).max(p.z - patch.max.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// A player that may be reflected: their drawn box and how many primitives
/// draw them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReflectionActor {
    pub min: DVec3,
    pub max: DVec3,
    pub meshes: usize,
}

/// `Matrix.ReflectionToRef(new Plane(0, -1, 0, y))`: mirrors heights about
/// the plane `y`.
#[must_use]
pub fn reflection_about_y(y: f32) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::X,
        -glam::Vec4::Y,
        glam::Vec4::Z,
        glam::Vec4::new(0.0, 2.0 * y, 0.0, 1.0),
    )
}

/// The view a pool at `water_y` reflects for `view_proj`, flipped left to
/// right so its faces keep their winding.
#[must_use]
pub fn mirrored_view_projection(view_proj: Mat4, water_y: f32) -> Mat4 {
    Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)) * view_proj * reflection_about_y(water_y)
}

/// What the water samples with: the mirrored view, rows downward as wgpu
/// stores them (`uv = clip.xy / w * 0.5 + 0.5`).
#[must_use]
pub fn sampling_matrix(mirrored: Mat4) -> Mat4 {
    Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * mirrored
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlockerUniform {
    view_proj: [[f32; 4]; 4],
    /// x: the water's height.
    water: [f32; 4],
}

const BLOCKER_WGSL: &str = "
struct Blocker { view_proj: mat4x4f, water: vec4f };
@group(0) @binding(0) var<uniform> blocker: Blocker;
struct Out { @builtin(position) clip: vec4f, @location(0) height: f32 };
@vertex fn vs_main(@location(0) position: vec3f) -> Out {
    var out: Out;
    out.clip = blocker.view_proj * vec4f(position, 1.0);
    out.height = position.y;
    return out;
}
@fragment fn fs_main(in: Out) -> @location(0) vec4f {
    if (in.height < blocker.water.x) { discard; }
    return vec4f(0.0);
}
";

struct Target {
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
    size: (u32, u32),
}

/// `WaterActorReflection`.
pub struct WaterActorReflection {
    /// `claim`: the opaque world batches, with their boxes.
    occluders: Vec<(u32, u32, [Vec3; 2])>,
    target: Option<Target>,
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// This frame's pool (an index into the probe cache's patches), the
    /// players chosen (indices into `update`'s list) and the blockers.
    pub patch: Option<usize>,
    pub actor_count: usize,
    selected: Vec<usize>,
    blockers: Vec<usize>,
    view_proj: Mat4,
    matrix: Mat4,
    eye: Vec3,
    pub status: &'static str,
}

impl WaterActorReflection {
    /// `claim(root)`: the drawn, opaque, non-liquid, non-autosprite batches
    /// block reflections.
    #[must_use]
    pub fn new(gpu: &Gpu, geometry: &WorldGeometry, liquid: &[bool]) -> Self {
        let occluders = geometry
            .batches
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                b.index_count > 0
                    && !liquid.get(*i).copied().unwrap_or(false)
                    && !b.passes.is_empty()
                    && b.passes
                        .iter()
                        .all(|p| !p.is_transparent && !p.autosprite && !p.autosprite2)
            })
            .map(|(_, b)| {
                (
                    b.first_index,
                    b.index_count,
                    [Vec3::from(b.bounds[0]), Vec3::from(b.bounds[1])],
                )
            })
            .collect();
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water-actor-occluder"),
            source: wgpu::ShaderSource::Wgsl(BLOCKER_WGSL.into()),
        });
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water-actor-occluder"),
            contents: bytemuck::bytes_of(&BlockerUniform {
                view_proj: Mat4::IDENTITY.to_cols_array_2d(),
                water: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water-actor-occluder"),
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
            label: Some("water-actor-occluder"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water-actor-occluder"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water-actor-occluder"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<WorldVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
            },
            // `backFaceCulling = false`.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
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
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            occluders,
            target: None,
            pipeline,
            buffer,
            bind_group,
            patch: None,
            actor_count: 0,
            selected: Vec::new(),
            blockers: Vec::new(),
            view_proj: Mat4::IDENTITY,
            matrix: Mat4::IDENTITY,
            eye: Vec3::ZERO,
            status: "waiting for a frame",
        }
    }

    /// `update`: picks the nearest visible pool within reach above the
    /// camera's feet (`patches`: index, patch, on screen), the nearest
    /// players over it, and the blockers in the mirrored view. Returns
    /// whether a capture is scheduled; the chosen players are
    /// [`Self::selected`].
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        gpu: &Gpu,
        camera: &Camera,
        size: (u32, u32),
        patches: &[(usize, &WaterPatch, bool)],
        actors: &[ReflectionActor],
        enabled: bool,
        disabled_reason: &'static str,
    ) -> bool {
        self.patch = None;
        self.actor_count = 0;
        self.selected.clear();
        self.blockers.clear();
        self.status = disabled_reason;
        if !enabled {
            return false;
        }
        let position = camera.position.as_dvec3();
        let mut best: Option<(usize, &WaterPatch, f64)> = None;
        for &(index, patch, visible) in patches {
            let distance = distance_to_water(position, patch);
            if position.y > patch.min.y + 1.0
                && distance < ACTOR_REFLECTION_RANGE
                && visible
                && best.is_none_or(|(_, _, d)| distance < d)
            {
                best = Some((index, patch, distance));
            }
        }
        self.status = "no nearby visible pool above water (range 240u)";
        let Some((index, patch, _)) = best else {
            return false;
        };
        let mut candidates: Vec<(usize, f64)> = actors
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let distance = distance_to_water((a.min + a.max) * 0.5, patch);
                (distance <= ACTOR_RANGE && a.max.y > patch.min.y).then_some((i, distance))
            })
            .collect();
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
        candidates.truncate(MAX_ACTORS);
        self.status = "no eligible model (loading, hidden, submerged or beyond 160u)";
        if candidates.is_empty() {
            return false;
        }
        self.status = "actor geometry budget exceeded";
        if candidates.iter().map(|&(i, _)| actors[i].meshes).sum::<usize>() > MAX_ACTOR_MESHES {
            return false;
        }
        // The patch's lowest point is its plane (its probe sits above it).
        let water_y = patch.min.y as f32;
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let mirrored = mirrored_view_projection(camera.view_projection(aspect), water_y);
        let frustum = Frustum::from_view_projection(mirrored);
        let blockers: Vec<usize> = (0..self.occluders.len())
            .filter(|&i| {
                let [min, max] = self.occluders[i].2;
                max.y >= water_y && frustum.intersects_box(min, max)
            })
            .collect();
        // Never truncate occluders to hit a budget: that could reveal hidden actors.
        self.status = "occluder budget exceeded";
        if blockers.len() > MAX_OCCLUDER_DRAWS {
            return false;
        }
        let scale = 0.25f64
            .min(320.0 / f64::from(size.0.max(1)))
            .min(320.0 / f64::from(size.1.max(1)));
        let width = ((f64::from(size.0) * scale).floor() as u32).max(1);
        let height = ((f64::from(size.1) * scale).floor() as u32).max(1);
        if self.target.as_ref().is_none_or(|t| t.size != (width, height)) {
            self.target = Some(create_target(gpu, (width, height)));
        }
        gpu.queue.write_buffer(
            &self.buffer,
            0,
            bytemuck::bytes_of(&BlockerUniform {
                view_proj: mirrored.to_cols_array_2d(),
                water: [water_y, 0.0, 0.0, 0.0],
            }),
        );
        self.view_proj = mirrored;
        self.matrix = sampling_matrix(mirrored);
        self.eye = Vec3::new(camera.position.x, 2.0 * water_y - camera.position.y, camera.position.z);
        self.patch = Some(index);
        self.actor_count = candidates.len();
        self.selected = candidates.into_iter().map(|(i, _)| i).collect();
        self.blockers = blockers;
        self.status = "capture scheduled";
        true
    }

    /// The players chosen this frame, as indices into `update`'s list.
    #[must_use]
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    /// The mirrored view the models draw through and the eye it sees from,
    /// while a capture is scheduled.
    #[must_use]
    pub fn view(&self) -> Option<(Mat4, Vec3)> {
        self.patch.map(|_| (self.view_proj, self.eye))
    }

    /// What the water binds: the capture, its sampling matrix and texel
    /// size, while `patch` is this frame's.
    #[must_use]
    pub fn bound(&self, patch: Option<usize>) -> Option<(&wgpu::TextureView, Mat4, [f32; 2])> {
        let target = self.target.as_ref()?;
        (patch.is_some() && patch == self.patch).then(|| {
            (
                &target.colour,
                self.matrix,
                [1.0 / target.size.0 as f32, 1.0 / target.size.1 as f32],
            )
        })
    }

    /// The capture texture, once one exists (for binding).
    #[must_use]
    pub fn texture(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.colour)
    }

    /// Draws the blockers, then the chosen players' opaque model draws
    /// (those `keep` accepts) through [`ModelRenderer::set_second_view`].
    pub fn capture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        world_vertices: &wgpu::Buffer,
        world_indices: &wgpu::Buffer,
        models: &ModelRenderer,
        keep: &dyn Fn(&crate::rendering::model_renderer::ModelDraw) -> bool,
    ) {
        let (Some(target), Some(_)) = (&self.target, self.patch) else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("water-actor-reflection"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.colour,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, world_vertices.slice(..));
        pass.set_index_buffer(world_indices.slice(..), wgpu::IndexFormat::Uint32);
        for &i in &self.blockers {
            let (first, count, _) = self.occluders[i];
            pass.draw_indexed(first..first + count, 0, 0..1);
        }
        models.draw_opaque_view(&mut pass, keep);
    }
}

fn create_target(gpu: &Gpu, (width, height): (u32, u32)) -> Target {
    let texture = |label, format, usage| {
        gpu.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    Target {
        colour: texture(
            "water-actor-reflection",
            HDR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        depth: texture(
            "water-actor-reflection depth",
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ),
        size: (width, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mirror_keeps_winding_and_reflects_heights() {
        let camera = Camera {
            position: Vec3::new(0.0, 100.0, -200.0),
            ..Camera::default()
        };
        let vp = camera.view_projection(16.0 / 9.0);
        let mirrored = mirrored_view_projection(vp, 20.0);
        assert!(mirrored.determinant() * vp.determinant() > 0.0);
        // A point and its mirror image land on the same pixel, flipped
        // left to right.
        let p = Vec3::new(30.0, 60.0, 0.0);
        let image = Vec3::new(30.0, -20.0, 0.0);
        let a = vp.project_point3(image);
        let b = mirrored.project_point3(p);
        assert!((a.x + b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5, "{a} {b}");
        // The water samples it with rows downward.
        let s = sampling_matrix(mirrored).project_point3(p);
        assert!((s.y + b.y).abs() < 1e-6);
    }

    #[test]
    fn distance_is_to_the_surface() {
        let patch = WaterPatch {
            batches: Vec::new(),
            min: DVec3::new(0.0, 10.0, 0.0),
            max: DVec3::new(100.0, 10.0, 100.0),
            position: DVec3::ZERO,
            triangles: Vec::new(),
        };
        assert!((distance_to_water(DVec3::new(50.0, 40.0, 50.0), &patch) - 30.0).abs() < 1e-12);
        assert!((distance_to_water(DVec3::new(-40.0, 40.0, 50.0), &patch) - 50.0).abs() < 1e-12);
    }
}
