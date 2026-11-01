//! SHAG `src/engine/weapon/ProjectileManager.ts`, the drawing of one
//! projectile so far: its MD3 model with the surfaces' shader materials
//! (`loadModelAssets`), and the node that carries it, facing its flight
//! (`faceAlong`).
//!
//! Not yet: flight, bounces and impacts, trails, the carried light, the
//! fade in, sprites and fallback textures, idle-shot reuse.

use glam::{Mat4, Quat, Vec3};

use crate::babylon::materials::standard_material::{StandardAlphaMode, StandardMaterialParams, TextureUse};
use crate::loader::model::gltf_model::{AlphaMode, GltfModel, ModelImage, ModelMaterial, ModelPrimitive, Skeleton};
use crate::loader::model::md3_loader::{
    Md3StageAnimation, create_material_from_shader, create_surface_mesh, is_effect_surface, parse_md3,
};
use crate::loader::pk3::pk3_loader::Pk3Loader;
use crate::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use crate::loader::texture::resolve_and_load_texture;
use crate::rendering::gpu::Gpu;
use crate::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};

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

#[cfg(test)]
mod tests {
    use super::*;

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
