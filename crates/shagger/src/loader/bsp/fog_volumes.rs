//! SHAG `src/engine/loader/bsp/FogVolumes.ts`: Q3 fog volumes
//! (`surfaceparm fog` brushes with `fogparms`), drawn as qfusion draws them.
//!
//! qfusion fogs every surface inside a fog brush by how far the view travels
//! through the fog to reach it. Its density (`glsl/include/fog_overload.glsl`)
//! is measured along the view axis, starts `clear_dist` in and is full at
//! `dist`:
//!
//! ```text
//! s = depth - clearDist                     (camera inside the fog)
//! s = (depth - clearDist) * below / (below + eyeHeight)
//!                                           (camera outside: only the part of
//!                                            the view under the fog surface)
//! density = sqrt(clamp(s / (dist - clearDist), 0, 1))
//! ```
//!
//! where "below" is how far the surface point is under the fog's visible
//! plane (the brush side its surface is drawn on) and eyeHeight is the
//! camera's height above it. The colour is the shader's colour normalized to
//! a brightest channel of 1.
//!
//! Each brush is drawn once without a depth test, its far side only (the
//! browser culls by winding; here the fragment checks its face's normal), so
//! it covers its screen area from inside or out. Every pixel reads the
//! opaque scene's depth (the world renderer's copy, which soft particles
//! read), rebuilds the world point behind it, and fogs it when that point is
//! inside the brush. Transparent surfaces are not in the depth and take the
//! fog of what is behind them; the view model is not fogged. The volumes
//! draw after the map's blended surfaces and before its particles and
//! additive ones.

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Mat4, Vec4};
use wgpu::util::DeviceExt;

use crate::loader::bsp::bsp_loader::BspFile;
use crate::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use crate::rendering::effect_quad_batch::EffectFrame;
use crate::rendering::gpu::Gpu;
use crate::rendering::world_renderer::DEPTH_FORMAT;

/// Brush sides a fog volume may have; a fog brush is usually a box.
pub const MAX_FOG_PLANES: usize = 16;

/// A plane as normal and distance: points with `dot(normal, p) > dist` are
/// outside.
pub type FogPlane = [f64; 4];

/// `FogVolume`: one fog brush with its shader's fog.
#[derive(Debug, Clone, PartialEq)]
pub struct FogVolume {
    pub shader_name: String,
    pub brush_index: usize,
    /// The brush's sides in engine coordinates, normals outward.
    pub planes: Vec<FogPlane>,
    /// The side the fog surface is drawn on.
    pub surface: FogPlane,
    /// The shader colour with its brightest channel at 1.
    pub color: [f64; 3],
    pub dist: f64,
    pub clear_dist: f64,
}

/// `toBabylonPlane`: Q3 (x, y, z) Z-up to engine (x, z, y) Y-up; a plane's
/// distance is unchanged.
fn to_engine_plane(normal: [f64; 3], dist: f64) -> FogPlane {
    [normal[0], normal[2], normal[1], dist]
}

/// qfusion `ColorNormalize`: scale so the brightest channel is 1.
fn normalize_color(color: [f64; 3]) -> [f64; 3] {
    let max = color[0].max(color[1]).max(color[2]);
    if max > 0.0 {
        [color[0] / max, color[1] / max, color[2] / max]
    } else {
        [0.0; 3]
    }
}

type Side = ([f64; 3], f64);

/// `drawnSide`: the brush side a face of the fog's shader is drawn on,
/// facing out of the brush: the fog's surface when the compiler wrote no
/// visible side.
fn drawn_side(bsp: &BspFile, shader: &str, sides: &[Side]) -> Option<Side> {
    let name = shader.to_lowercase();
    for face in &bsp.common.faces {
        let matches = usize::try_from(face.texture_index)
            .ok()
            .and_then(|t| bsp.common.textures.get(t))
            .is_some_and(|t| t.name.to_lowercase() == name);
        if !matches {
            continue;
        }
        let Some(at) = usize::try_from(face.first_vertex)
            .ok()
            .and_then(|v| bsp.vertices.position(v))
        else {
            continue;
        };
        let n = face.normal.map(f64::from);
        let at = at.map(f64::from);
        let found = sides.iter().find(|(normal, dist)| {
            normal[0] * n[0] + normal[1] * n[1] + normal[2] * n[2] > 0.99
                && (normal[0] * at[0] + normal[1] * at[1] + normal[2] * at[2] - dist).abs() < 1.0
        });
        if let Some(side) = found {
            return Some(*side);
        }
    }
    None
}

/// `fogVolumesFrom`: every fog brush in the BSP with its shader's fog
/// parameters. The surface is the fog's visible side; when the compiler
/// wrote none (or garbage), the side a face of the fog's shader is drawn on,
/// and failing that the side facing most nearly up, which is where a fog
/// pit's surface is.
#[must_use]
pub fn fog_volumes_from(bsp: &BspFile, shaders: &Q3ShaderRegistry) -> Vec<FogVolume> {
    let mut volumes = Vec::new();
    for effect in &bsp.effects {
        let parms = shaders.get_shader(&effect.name).and_then(|s| s.fog_parms);
        let brush = usize::try_from(effect.brush_index)
            .ok()
            .and_then(|b| bsp.common.brushes.get(b).map(|brush| (b, brush)));
        let (Some(parms), Some((brush_index, brush))) = (parms, brush) else {
            continue;
        };
        let mut q3: Vec<Side> = Vec::new();
        for i in 0..brush.num_sides.max(0) {
            let plane = usize::try_from(brush.first_side + i)
                .ok()
                .and_then(|s| bsp.common.brush_sides.get(s))
                .and_then(|s| usize::try_from(s.plane_index).ok())
                .and_then(|p| bsp.common.planes.get(p));
            if let Some(plane) = plane {
                q3.push((plane.normal.map(f64::from), f64::from(plane.dist)));
            }
        }
        if q3.len() < 4 || q3.len() > MAX_FOG_PLANES {
            eprintln!(
                "[FogVolumes] {}: brush {brush_index} has {} usable sides; skipped",
                effect.name,
                q3.len()
            );
            continue;
        }
        let side = effect.visible_side;
        let surface = usize::try_from(side)
            .ok()
            .and_then(|s| q3.get(s).copied())
            .or_else(|| drawn_side(bsp, &effect.name, &q3))
            .unwrap_or_else(|| {
                q3.iter()
                    .copied()
                    .reduce(|best, p| if p.0[2] > best.0[2] { p } else { best })
                    .expect("at least four sides")
            });
        volumes.push(FogVolume {
            shader_name: effect.name.clone(),
            brush_index,
            planes: q3.iter().map(|(n, d)| to_engine_plane(*n, *d)).collect(),
            surface: to_engine_plane(surface.0, surface.1),
            color: normalize_color(parms.color),
            dist: parms.depth_for_opaque,
            clear_dist: parms.clear_dist,
        });
    }
    volumes
}

/// A brush's closed hull ([`brush_hull`]): corner positions, each side's
/// outward normal per corner, and triangles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hull {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub indices: Vec<u32>,
}

/// `brushHull`: the closed hull of a convex brush, every corner where three
/// sides meet and no side excludes, each side's corners in order around it.
/// Triangles wind counter-clockwise seen from outside.
#[must_use]
pub fn brush_hull(planes: &[FogPlane]) -> Option<Hull> {
    let n: Vec<DVec3> = planes.iter().map(|p| DVec3::new(p[0], p[1], p[2])).collect();
    let mut corners: Vec<DVec3> = Vec::new();
    for a in 0..planes.len() {
        for b in a + 1..planes.len() {
            for c in b + 1..planes.len() {
                let bc = n[b].cross(n[c]);
                let det = n[a].dot(bc);
                if det.abs() < 1e-6 {
                    continue;
                }
                let point = (bc * planes[a][3] + n[c].cross(n[a]) * planes[b][3] + n[a].cross(n[b]) * planes[c][3])
                    * (1.0 / det);
                if planes.iter().enumerate().all(|(i, p)| n[i].dot(point) - p[3] <= 0.01)
                    && !corners.iter().any(|q| q.distance_squared(point) < 0.01)
                {
                    corners.push(point);
                }
            }
        }
    }
    if corners.len() < 4 {
        return None;
    }
    let mut hull = Hull::default();
    for (i, plane) in planes.iter().enumerate() {
        let mut on: Vec<DVec3> = corners
            .iter()
            .copied()
            .filter(|p| (n[i].dot(*p) - plane[3]).abs() < 0.05)
            .collect();
        if on.len() < 3 {
            continue;
        }
        let centre = on.iter().copied().sum::<DVec3>() / on.len() as f64;
        // Order around the side's normal.
        let u = (on[0] - centre).normalize();
        let v = n[i].cross(u);
        let angle = |p: &DVec3| (*p - centre).dot(v).atan2((*p - centre).dot(u));
        on.sort_by(|p, q| angle(p).total_cmp(&angle(q)));
        let base = hull.positions.len() as u32;
        for p in &on {
            hull.positions.push(p.to_array());
            hull.normals.push(n[i].to_array());
        }
        for k in 1..on.len() as u32 - 1 {
            hull.indices.extend([base, base + k, base + k + 1]);
        }
    }
    (!hull.indices.is_empty()).then_some(hull)
}

/// The volumes' shader (MIT): the far side of each brush, fogging the scene
/// point behind each pixel when it lies inside the brush.
const FOG_VOLUME_WGSL: &str = r"
struct Globals {
    view_proj: mat4x4f,
    inv_view_proj: mat4x4f,
    // xyz: the eye
    eye: vec4f,
    // xyz: the view axis
    forward: vec4f,
    // xy: the target's size in pixels
    screen: vec4f,
};

struct Volume {
    planes: array<vec4f, 16>,
    // x: how many planes
    count: vec4f,
    surface: vec4f,
    color: vec4f,
    // x: 1 / (dist - clearDist), y: clearDist
    scale_clear: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var scene_depth: texture_depth_2d;
@group(2) @binding(0) var<uniform> volume: Volume;

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
};

@vertex
fn vs_main(@location(0) position: vec3f, @location(1) normal: vec3f) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(position, 1.0);
    out.world = position;
    out.normal = normal;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let eye = globals.eye.xyz;
    // The far side only: one fragment per pixel, inside the volume or out.
    if dot(in.normal, in.world - eye) <= 0.0 { discard; }
    let size = vec2f(textureDimensions(scene_depth));
    let pixel = vec2i(in.clip.xy);
    let d = textureLoad(scene_depth, pixel, 0);
    // Nothing drawn here: the sky, or the cleared background.
    if d >= 1.0 { discard; }
    let ndc = vec2f(in.clip.x / size.x * 2.0 - 1.0, 1.0 - in.clip.y / size.y * 2.0);
    let behind = globals.inv_view_proj * vec4f(ndc, d, 1.0);
    let p = behind.xyz / behind.w;
    let z = dot(p - eye, globals.forward.xyz);
    let count = i32(volume.count.x);
    for (var i = 0; i < 16; i++) {
        if i < count && dot(p, volume.planes[i].xyz) - volume.planes[i].w > 1.0 { discard; }
    }
    // qfusion FogGenCoord: depth past the clear distance, and only the part
    // of the view under the fog surface when the eye is above it.
    let fog_dist = z - volume.scale_clear.y;
    let below = dot(p, volume.surface.xyz) - volume.surface.w;
    let eye_height = dot(eye, volume.surface.xyz) - volume.surface.w;
    var s = fog_dist;
    if eye_height > 0.0 {
        let to_eye = below - eye_height;
        s = select(0.0, fog_dist * below / min(to_eye, -0.001), below <= 0.0);
    }
    let t = -below * volume.scale_clear.x + 1.5 / 256.0;
    let density = sqrt(clamp(s * volume.scale_clear.x, 0.0, 1.0)) * step(1.0 / 256.0, t);
    if density <= 0.0 { discard; }
    return vec4f(pow(volume.color.rgb, vec3f(2.2)), density);
}
";

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    screen: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VolumeUniform {
    planes: [[f32; 4]; MAX_FOG_PLANES],
    count: [f32; 4],
    surface: [f32; 4],
    color: [f32; 4],
    scale_clear: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HullVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

struct DrawnVolume {
    bind_group: wgpu::BindGroup,
    first_index: u32,
    index_count: u32,
    bounds: [DVec3; 2],
}

/// `FogVolumeSet`: the map's fog volumes on the GPU.
pub struct FogVolumes {
    pub volumes: Vec<FogVolume>,
    drawn: Vec<DrawnVolume>,
    pipeline: Option<wgpu::RenderPipeline>,
    globals: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    /// Whether a volume is in view this frame (`prepare`).
    in_view: bool,
}

impl FogVolumes {
    /// `createFogVolumes`: the map's fog brushes, drawn into
    /// `color_format`, reading the scene depth through `depth_layout` (the
    /// world renderer's soft depth layout).
    #[must_use]
    pub fn new(
        gpu: &Gpu,
        color_format: wgpu::TextureFormat,
        depth_layout: &wgpu::BindGroupLayout,
        volumes: Vec<FogVolume>,
    ) -> Self {
        let device = &gpu.device;
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fog volumes globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_layout = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            })
        };
        let globals_layout = uniform_layout("fog volumes globals");
        let volume_layout = uniform_layout("fog volume");
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog volumes globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let mut vertices: Vec<HullVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut drawn = Vec::new();
        for volume in &volumes {
            let Some(hull) = brush_hull(&volume.planes) else {
                eprintln!(
                    "[FogVolumes] {}: brush {} has no closed hull; skipped",
                    volume.shader_name, volume.brush_index
                );
                continue;
            };
            let base = vertices.len() as u32;
            let first_index = indices.len() as u32;
            let mut bounds = [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)];
            for (p, n) in hull.positions.iter().zip(&hull.normals) {
                bounds[0] = bounds[0].min(DVec3::from_array(*p));
                bounds[1] = bounds[1].max(DVec3::from_array(*p));
                vertices.push(HullVertex {
                    position: p.map(|v| v as f32),
                    normal: n.map(|v| v as f32),
                });
            }
            indices.extend(hull.indices.iter().map(|i| base + i));
            let mut uniform = VolumeUniform {
                planes: [[0.0; 4]; MAX_FOG_PLANES],
                count: [volume.planes.len() as f32, 0.0, 0.0, 0.0],
                surface: volume.surface.map(|v| v as f32),
                color: [
                    volume.color[0] as f32,
                    volume.color[1] as f32,
                    volume.color[2] as f32,
                    1.0,
                ],
                scale_clear: [
                    (1.0 / (volume.dist - volume.clear_dist)) as f32,
                    volume.clear_dist as f32,
                    0.0,
                    0.0,
                ],
            };
            for (slot, plane) in uniform.planes.iter_mut().zip(&volume.planes) {
                *slot = plane.map(|v| v as f32);
            }
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fog volume"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fog volume"),
                layout: &volume_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            drawn.push(DrawnVolume {
                bind_group,
                first_index,
                index_count: indices.len() as u32 - first_index,
                bounds,
            });
        }
        let (pipeline, vertex_buffer, index_buffer) = if drawn.is_empty() {
            (None, None, None)
        } else {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("fog volumes"),
                source: wgpu::ShaderSource::Wgsl(FOG_VOLUME_WGSL.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("fog volumes"),
                bind_group_layouts: &[Some(&globals_layout), Some(depth_layout), Some(&volume_layout)],
                immediate_size: 0,
            });
            let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fog volumes"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<HullVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                // No depth test: the far side covers the volume's screen area
                // even from inside it.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    // ALPHA_COMBINE.
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fog volume hulls"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fog volume hulls"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            (Some(pipeline), Some(vertex_buffer), Some(index_buffer))
        };
        Self {
            volumes,
            drawn,
            pipeline,
            globals,
            globals_bind_group,
            vertices: vertex_buffer,
            indices: index_buffer,
            in_view: false,
        }
    }

    /// How many volumes are drawn.
    #[must_use]
    pub fn len(&self) -> usize {
        self.drawn.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.drawn.is_empty()
    }

    /// Whether a volume's bounds meet the view `view_proj` sees: the scene
    /// depth is read only then (`prepare`'s `inView`).
    #[must_use]
    pub fn any_in_view(&self, view_proj: Mat4) -> bool {
        self.drawn.iter().any(|d| {
            let [lo, hi] = d.bounds;
            let clips: [Vec4; 8] = std::array::from_fn(|i| {
                let p = DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                view_proj * Vec4::new(p.x as f32, p.y as f32, p.z as f32, 1.0)
            });
            // Outside when every corner is beyond one clip plane.
            let out = |f: &dyn Fn(&Vec4) -> bool| clips.iter().all(f);
            !(out(&|c| c.x < -c.w)
                || out(&|c| c.x > c.w)
                || out(&|c| c.y < -c.w)
                || out(&|c| c.y > c.w)
                || out(&|c| c.z < 0.0)
                || out(&|c| c.z > c.w))
        })
    }

    /// `prepare`: the camera for this frame, and whether the volumes draw
    /// (`in_view`: one is in view and the scene depth was copied for it).
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, in_view: bool) {
        self.in_view = in_view && !self.drawn.is_empty();
        if !self.in_view {
            return;
        }
        let inverse = frame.view_proj.inverse();
        let eye = frame.camera_position;
        // The view axis: toward the far plane's centre.
        let far = inverse * Vec4::new(0.0, 0.0, 1.0, 1.0);
        let forward = (far.truncate() / far.w - eye).normalize_or_zero();
        let globals = Globals {
            view_proj: frame.view_proj.to_cols_array_2d(),
            inv_view_proj: inverse.to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 0.0],
            forward: [forward.x, forward.y, forward.z, 0.0],
            screen: [frame.viewport[0], frame.viewport[1], 0.0, 0.0],
        };
        gpu.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
    }

    /// Draws the volumes this frame, reading the scene depth through
    /// `depth` (the world renderer's soft depth bind group).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, depth: &wgpu::BindGroup) {
        let (Some(pipeline), Some(vertices), Some(indices)) = (&self.pipeline, &self.vertices, &self.indices) else {
            return;
        };
        if !self.in_view {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_bind_group(1, depth, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        for volume in &self.drawn {
            pass.set_bind_group(2, &volume.bind_group, &[]);
            pass.draw_indexed(volume.first_index..volume.first_index + volume.index_count, 0, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    //! SHAG `src/engine/loader/bsp/__tests__/FogVolumes.test.ts`.
    use super::*;
    use crate::loader::shader::q3_shader_parser::Q3ShaderParser;
    use crate::loader::shader::q3_shader_types::FogParms;

    /// An axis-aligned box as six outward planes.
    fn boxed(min: [f64; 3], max: [f64; 3]) -> Vec<FogPlane> {
        vec![
            [1.0, 0.0, 0.0, max[0]],
            [-1.0, 0.0, 0.0, -min[0]],
            [0.0, 1.0, 0.0, max[1]],
            [0.0, -1.0, 0.0, -min[1]],
            [0.0, 0.0, 1.0, max[2]],
            [0.0, 0.0, -1.0, -min[2]],
        ]
    }

    #[test]
    fn closes_a_box_with_two_triangles_per_side_wound_outward() {
        let hull = brush_hull(&boxed([0.0; 3], [64.0, 32.0, 16.0])).unwrap();
        assert_eq!(hull.indices.len(), 6 * 2 * 3);
        let p = |i: u32| DVec3::from_array(hull.positions[i as usize]);
        for t in hull.indices.chunks(3) {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let normal = (b - a).cross(c - a);
            let centre = (a + b + c) / 3.0;
            let outward = centre - DVec3::new(32.0, 16.0, 8.0);
            assert!(normal.dot(outward) > 0.0);
            // The normal each corner carries is its side's, outward too.
            assert!(DVec3::from_array(hull.normals[t[0] as usize]).dot(outward) > 0.0);
        }
        let xs: Vec<f64> = hull.positions.iter().map(|p| p[0]).collect();
        assert!(xs.iter().copied().fold(f64::INFINITY, f64::min).abs() < 1e-9);
        assert!((xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 64.0).abs() < 1e-9);
    }

    #[test]
    fn ignores_a_plane_that_cuts_nothing_off() {
        let mut planes = boxed([0.0; 3], [16.0; 3]);
        planes.push([1.0, 1.0, 0.0, 100.0]);
        assert_eq!(brush_hull(&planes).unwrap().indices.len(), 6 * 2 * 3);
    }

    fn fog(line: &str) -> Option<FogParms> {
        Q3ShaderParser::new()
            .parse_shader_file(&format!("textures/t/fog\n{{\n surfaceparm fog\n {line}\n}}\n"))
            .unwrap()[0]
            .fog_parms
    }

    #[test]
    fn reads_qfusions_clear_distance() {
        assert_eq!(
            fog("fogparms ( .04 0.36 .04 ) 500 5"),
            Some(FogParms {
                color: [0.04, 0.36, 0.04],
                depth_for_opaque: 500.0,
                clear_dist: 5.0,
            })
        );
    }

    #[test]
    fn has_no_clear_distance_without_one_and_keeps_it_128_short_of_opaque() {
        assert_eq!(fog("fogparms ( 1 1 1 ) 300").unwrap().clear_dist, 0.0);
        assert_eq!(fog("fogparms ( 1 1 1 ) 300 250").unwrap().clear_dist, 172.0);
        assert_eq!(fog("fogparms ( 1 1 1 ) 100 50").unwrap().clear_dist, 0.0);
    }
}
