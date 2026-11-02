//! SHAG `src/engine/weapon/ProjectileManager.ts`, the drawing of one
//! projectile so far: its MD3 model with the surfaces' shader materials
//! (`loadModelAssets`), the node that carries it, facing its flight
//! (`faceAlong`), and its trail (`updateTrail`, `clampTrailLength`,
//! `trailMaterialFor`).
//!
//! Not yet: flight, bounces and impacts, the carried light, the fade in,
//! sprites and fallback textures, idle-shot reuse.

use glam::{DVec3, Mat4, Quat, Vec3};

use crate::babylon::materials::standard_material::{StandardAlphaMode, StandardMaterialParams, TextureUse};
use crate::babylon::meshes::ribbon_builder::create_ribbon_double_side;
use crate::babylon::meshes::vertex_data::VertexData;
use crate::loader::model::gltf_model::{AlphaMode, GltfModel, ModelImage, ModelMaterial, ModelPrimitive, Skeleton};
use crate::loader::model::md3_loader::{
    Md3StageAnimation, create_material_from_shader, create_surface_mesh, is_effect_surface, parse_md3,
};
use crate::loader::pk3::pk3_loader::Pk3Loader;
use crate::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use crate::loader::texture::resolve_and_load_texture;
use crate::rendering::gpu::Gpu;
use crate::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};
use crate::weapon::effect_geometry::unlit_trail_ribbon_positions;

/// `_md3Correction`: MD3 projectiles face +Y in Quake axes, a -90° turn
/// about Y.
#[must_use]
pub fn md3_correction() -> Quat {
    Quat::from_axis_angle(Vec3::Y, -std::f32::consts::FRAC_PI_2)
}

/// `faceAlong`: the rotation that turns +Z onto `dir` (normalized), then
/// the MD3 turn.
#[must_use]
pub fn face_along(dir: Vec3) -> Quat {
    let forward = Vec3::Z;
    let dot = forward.dot(dir);
    let q = if dot > 0.9999 {
        Quat::IDENTITY
    } else if dot < -0.9999 {
        Quat::from_axis_angle(Vec3::Y, std::f32::consts::PI)
    } else {
        Quat::from_axis_angle(forward.cross(dir).normalize(), dot.clamp(-1.0, 1.0).acos())
    };
    q * md3_correction()
}

/// A projectile node's world matrix: at `position`, facing `direction`,
/// scaled by the definition's `scale`.
#[must_use]
pub fn projectile_matrix(position: Vec3, direction: Vec3, scale: f32) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::splat(scale), face_along(direction.normalize()), position)
}

/// A projectile type's model, uploaded, with each surface's animation.
pub struct ProjectileModel {
    pub model: ModelHandle,
    /// Per primitive.
    animations: Vec<Option<Md3StageAnimation>>,
    /// Surfaces whose texture did not load.
    pub missing_textures: Vec<String>,
}

/// `loadModelAssets` for an MD3 projectile: parse, build each surface's
/// material from its shader, load its texture, upload. `light_color` is the
/// definition's, which the gunblade blast's glow takes (`blade_blast`).
pub fn load_projectile_model(
    gpu: &Gpu,
    models: &mut ModelRenderer,
    pk3: &mut Pk3Loader,
    shaders: &Q3ShaderRegistry,
    def_id: &str,
    path: &str,
    light_color: Option<Vec3>,
) -> Result<ProjectileModel, String> {
    let bytes = pk3.get_file(path).ok_or_else(|| format!("Model not found: {path}"))?;
    let parsed = parse_md3(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let mut primitives = Vec::new();
    let mut materials = Vec::new();
    let mut images = Vec::new();
    let mut modes = Vec::new();
    let mut animations = Vec::new();
    let mut missing_textures = Vec::new();
    for surface in &parsed.surfaces {
        if is_effect_surface(surface) {
            continue;
        }
        let mesh = create_surface_mesh(surface, def_id, 0);
        let Some(shader) = surface.shaders.first() else {
            continue;
        };
        let mut md3 = create_material_from_shader(shaders, pk3, &shader.name);
        let image = match resolve_and_load_texture(pk3, &md3.texture_path)
            .and_then(|t| pk3.get_file(&t.path).map(|bytes| (t.path, bytes)))
        {
            Some((name, bytes)) => {
                images.push(ModelImage {
                    name,
                    mime_type: None,
                    bytes,
                });
                Some(images.len() - 1)
            }
            None => {
                // No image: the material keeps its colours and no texture.
                missing_textures.push(md3.texture_path.clone());
                md3.params.texture = TextureUse::default();
                md3.animation = None;
                None
            }
        };
        if def_id == "blade_blast"
            && let Some(color) = light_color
        {
            // The source glow is orange on black; its brightness masks the
            // teal light colour, added as a constant emissive.
            md3.params = StandardMaterialParams {
                texture: TextureUse {
                    opacity_rgb: image.is_some(),
                    ..TextureUse::default()
                },
                emissive_as_illumination: false,
                emissive_color: color,
                diffuse_color: Vec3::ZERO,
                disable_lighting: true,
                alpha_mode: StandardAlphaMode::Add,
                alpha_blend: true,
                disable_depth_write: true,
                back_face_culling: false,
                ..md3.params
            };
            md3.animation = None;
        }
        let index = materials.len();
        materials.push(ModelMaterial {
            name: format!("{index}"),
            base_color: [1.0; 4],
            base_color_image: image,
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        });
        modes.push(md3.params);
        animations.push(md3.animation);
        let (min, max) = mesh
            .positions
            .iter()
            .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            });
        primitives.push(ModelPrimitive {
            name: mesh.name,
            node: 0,
            node_scale: Vec3::ONE,
            node_matrix: Mat4::IDENTITY,
            positions: mesh.positions,
            normals: mesh.normals,
            uvs: mesh.uvs,
            // Babylon's counter-clockwise front, turned to the renderer's
            // clockwise one.
            indices: mesh.indices.chunks_exact(3).flat_map(|t| [t[2], t[1], t[0]]).collect(),
            material: Some(index),
            bounds_center: (min + max) * 0.5,
            creation_order: index,
            skin: None,
        });
    }
    let model = GltfModel {
        primitives,
        materials,
        images,
        attachment_points: Default::default(),
        skeleton: Skeleton::default(),
    };
    let handle = models.upload(gpu, &model, |m| {
        ModelMaterialMode::Standard(modes[m.name.parse::<usize>().unwrap_or(0)])
    });
    Ok(ProjectileModel {
        model: handle,
        animations,
        missing_textures,
    })
}

impl ProjectileModel {
    /// The model's draws for a node at `world`, its shader stages
    /// `shader_seconds` into the animation clock.
    pub fn draws(&self, world: Mat4, shader_seconds: f64, out: &mut Vec<ModelDraw>) {
        for (i, animation) in self.animations.iter().enumerate() {
            let mut draw = ModelDraw::new(self.model, i, world);
            if let Some(animation) = animation {
                let frame = animation.at(shader_seconds);
                draw.texture_matrix = Some(frame.texture_matrix);
                draw.texture_level = frame.texture_level;
                draw.tint = frame.diffuse_color;
            }
            out.push(draw);
        }
    }
}

/// A trail keeps this many points, the newest last.
pub const MAX_TRAIL_POINTS: usize = 60;
/// Half the trail's width at its head.
pub const TRAIL_WIDTH: f64 = 6.0;

/// `clampTrailLength`: shortens a trail to `max_length` along its path,
/// newest point last. Points past the cut are packed just behind it, a
/// hundredth of a unit apart, so the ribbon keeps its point count and the
/// width taper ends at the cut.
pub fn clamp_trail_length(points: &mut [DVec3], max_length: f64) {
    let mut length = 0.0;
    for i in (1..points.len()).rev() {
        let head = points[i];
        let tail = points[i - 1];
        let segment = head.distance(tail);
        if length + segment <= max_length {
            length += segment;
            continue;
        }
        let cut = head + (tail - head) * ((max_length - length) / segment);
        let back = if segment > 0.0 {
            (tail - head) * (1.0 / segment)
        } else {
            DVec3::ZERO
        };
        for j in (0..i).rev() {
            points[j] = cut + back * ((i - 1 - j) as f64 * 0.01);
        }
        return;
    }
}

/// Babylon's `Vector3.normalize`: unchanged when zero length.
fn normalized(v: DVec3) -> DVec3 {
    let length = v.length();
    if length == 0.0 { v } else { v * (1.0 / length) }
}

/// A ribbon's normals and UVs.
type RibbonAttributes = (Vec<[f32; 3]>, Vec<[f32; 2]>);

/// A projectile's trail: its recent points and the two crossed ribbons
/// drawn through them (`proj.trailPoints`, `trailMesh`, `trailMeshCross`).
#[derive(Default)]
pub struct ProjectileTrail {
    /// Oldest first.
    pub points: Vec<DVec3>,
    model: Option<ModelHandle>,
    /// Each ribbon's normals and UVs as built; updates move only positions.
    built: Vec<RibbonAttributes>,
}

impl ProjectileTrail {
    /// Forgets the points but keeps the uploaded ribbons, so the trail can
    /// serve the next projectile of its type (SHAG disposes the meshes; a
    /// match fires hundreds, so they are pooled here instead).
    pub fn reset(&mut self) {
        self.points.clear();
    }

    /// `updateTrail`'s points: the first update pre-fills the trail with
    /// points packed just behind `position`; each records `position`,
    /// dropping the oldest, then clamps the length.
    pub fn push(&mut self, position: DVec3, direction: DVec3, max_length: Option<f64>) {
        if self.points.is_empty() {
            let back = if direction.length_squared() > 0.001 {
                direction
            } else {
                DVec3::Y
            };
            for i in 0..MAX_TRAIL_POINTS {
                let behind = (MAX_TRAIL_POINTS - i) as f64 * 0.01;
                self.points.push(position - back * behind);
            }
        }
        if self.points.len() >= MAX_TRAIL_POINTS {
            self.points.remove(0);
        }
        self.points.push(position);
        let overshoot = self.points.len().saturating_sub(MAX_TRAIL_POINTS);
        self.points.drain(..overshoot);
        if let Some(max_length) = max_length.filter(|l| *l != 0.0) {
            clamp_trail_length(&mut self.points, max_length);
        }
    }

    /// `updateTrail`'s ribbon paths for a camera at `camera`: each point
    /// pushed out both ways along the perpendicular facing the camera (A,
    /// B), and along the one crossing it (C, D), by a width tapering from
    /// nothing at the tail to [`TRAIL_WIDTH`] at the head by distance along
    /// the trail.
    #[must_use]
    pub fn paths(&self, camera: Option<DVec3>) -> [Vec<DVec3>; 4] {
        let points = &self.points;
        let n = points.len();
        let mut paths: [Vec<DVec3>; 4] = Default::default();
        if n < 2 {
            return paths;
        }
        let mut arc = vec![0.0; n];
        for i in 1..n {
            arc[i] = arc[i - 1] + points[i].distance(points[i - 1]);
        }
        let total = arc[n - 1];
        for (i, &p) in points.iter().enumerate() {
            let tangent = normalized(if i < n - 1 {
                points[i + 1] - p
            } else {
                p - points[i - 1]
            });
            let mut perp = DVec3::ZERO;
            if let Some(camera) = camera {
                perp = tangent.cross(camera - p);
            }
            if perp.length_squared() < 0.001 {
                perp = tangent.cross(DVec3::Y);
            }
            if perp.length_squared() < 0.001 {
                // Vector3.Forward() in a left-handed scene.
                perp = tangent.cross(DVec3::Z);
            }
            let perp = normalized(perp);
            let t = if total > 0.001 { arc[i] / total } else { 0.0 };
            let width = TRAIL_WIDTH * t;
            let perp_cross = normalized(tangent.cross(perp));
            paths[0].push(p + perp * width);
            paths[1].push(p - perp * width);
            paths[2].push(p + perp_cross * width);
            paths[3].push(p - perp_cross * width);
        }
        paths
    }

    /// Builds the ribbons for a camera at `camera` the first time, and
    /// streams their positions after. `light_color` is the definition's,
    /// the trail's colour (`trailMaterialFor`).
    pub fn update(&mut self, gpu: &Gpu, models: &mut ModelRenderer, camera: Option<DVec3>, light_color: Option<Vec3>) {
        let [a, b, c, d] = self.paths(camera);
        if a.is_empty() {
            return;
        }
        let ribbons = [(a, b), (c, d)];
        match self.model {
            Some(model) => {
                for (i, (p, q)) in ribbons.iter().enumerate() {
                    let positions = unlit_trail_ribbon_positions(p, q);
                    let built = &self.built[i];
                    models.update_vertices(gpu, model, i, &positions, &built.0, &built.1);
                }
            }
            None => {
                let built: Vec<VertexData> = ribbons.iter().map(|(p, q)| create_ribbon_double_side(p, q)).collect();
                self.model = Some(upload_trail(gpu, models, &built, light_color));
                self.built = built
                    .iter()
                    .map(|d| {
                        (
                            d.normals.chunks_exact(3).map(|n| [n[0], n[1], n[2]]).collect(),
                            d.uvs.chunks_exact(2).map(|t| [t[0], t[1]]).collect(),
                        )
                    })
                    .collect();
            }
        }
    }

    /// The ribbons' draws, once built.
    pub fn draws(&self, out: &mut Vec<ModelDraw>) {
        if let Some(model) = self.model {
            for i in 0..2 {
                out.push(ModelDraw::new(model, i, Mat4::IDENTITY));
            }
        }
    }
}

fn bounds_center(positions: &[[f32; 3]]) -> Vec3 {
    let (min, max) = positions
        .iter()
        .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
    (min + max) * 0.5
}

/// `trailMaterialFor`: the light colour glowing (a blue default), no
/// diffuse or lighting, both sides, alpha 0.6.
#[must_use]
pub fn trail_material(light_color: Option<Vec3>) -> StandardMaterialParams {
    StandardMaterialParams {
        emissive_color: light_color.unwrap_or(Vec3::new(0.3, 0.5, 1.0)),
        diffuse_color: Vec3::ZERO,
        disable_lighting: true,
        back_face_culling: false,
        alpha: 0.6,
        ..StandardMaterialParams::default()
    }
}

/// The trail and its crossing blade as one model of two primitives
/// sharing the trail material.
fn upload_trail(
    gpu: &Gpu,
    models: &mut ModelRenderer,
    ribbons: &[VertexData],
    light_color: Option<Vec3>,
) -> ModelHandle {
    let primitives = ribbons
        .iter()
        .enumerate()
        .map(|(i, data)| {
            let positions: Vec<[f32; 3]> = data.positions.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
            ModelPrimitive {
                name: if i == 0 { "trail" } else { "trailCross" }.to_owned(),
                node: 0,
                node_scale: Vec3::ONE,
                node_matrix: Mat4::IDENTITY,
                bounds_center: bounds_center(&positions),
                positions,
                normals: data.normals.chunks_exact(3).map(|n| [n[0], n[1], n[2]]).collect(),
                uvs: data.uvs.chunks_exact(2).map(|t| [t[0], t[1]]).collect(),
                // Babylon's counter-clockwise front, turned to the renderer's
                // clockwise one.
                indices: data.indices.chunks_exact(3).flat_map(|t| [t[2], t[1], t[0]]).collect(),
                material: Some(0),
                creation_order: i,
                skin: None,
            }
        })
        .collect();
    let model = GltfModel {
        primitives,
        materials: vec![ModelMaterial {
            name: "trail".to_owned(),
            base_color: [1.0; 4],
            base_color_image: None,
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        }],
        images: Vec::new(),
        attachment_points: Default::default(),
        skeleton: Skeleton::default(),
    };
    let material = trail_material(light_color);
    models.upload(gpu, &model, |_| ModelMaterialMode::Standard(material))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trail_prefills_then_records() {
        let mut trail = ProjectileTrail::default();
        trail.push(DVec3::new(10.0, 0.0, 0.0), DVec3::X, None);
        assert_eq!(trail.points.len(), MAX_TRAIL_POINTS);
        assert_eq!(trail.points[59], DVec3::new(10.0, 0.0, 0.0));
        assert!((trail.points[58].x - (10.0 - 0.01)).abs() < 1e-12);
        trail.push(DVec3::new(20.0, 0.0, 0.0), DVec3::X, Some(5.0));
        assert_eq!(trail.points.len(), MAX_TRAIL_POINTS);
        assert_eq!(trail.points[59], DVec3::new(20.0, 0.0, 0.0));
        assert!((trail.points[58].x - 15.0).abs() < 1e-12);
        assert!((trail.points[57].x - 14.99).abs() < 1e-12);
        let [a, b, ..] = trail.paths(Some(DVec3::new(15.0, 0.0, -100.0)));
        // Full width at the head, facing the camera: across Y.
        assert!((a[59] - DVec3::new(20.0, 6.0, 0.0)).length() < 1e-9, "{}", a[59]);
        assert!((b[59] - DVec3::new(20.0, -6.0, 0.0)).length() < 1e-9);
        assert_eq!(a[0], trail.points[0]);
    }

    #[test]
    fn faces_along_its_flight() {
        // The correction turns +X (the models' nose) onto +Z, and the face
        // turns +Z onto the flight.
        for dir in [Vec3::X, Vec3::Z, -Vec3::Z, Vec3::new(1.0, 1.0, 0.0).normalize()] {
            let q = face_along(dir);
            let md3_forward = Vec3::X;
            assert!((q * md3_forward - dir).length() < 1e-5, "{dir}");
        }
    }
}
