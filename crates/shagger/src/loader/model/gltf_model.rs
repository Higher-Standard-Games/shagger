//! A glTF model as SHAG's pickup cache hands it out (SHAG
//! `src/engine/loader/AssetPreloader.ts` `preloadPickupModels` and
//! `cloneCachedModel`, over Babylon's glTF loader).
//!
//! Babylon imports a `.glb` under a `__root__` mesh that turns glTF's
//! right-handed space into its left-handed one (scaling (1, 1, -1), a half
//! turn about Y). The pickup cache keeps only the meshes with vertices,
//! in the loader's order (by glTF node, a multi-primitive node's primitives
//! in turn), and a clone sits under a fresh root: the node tree and
//! `__root__` are gone. So each primitive is its vertex data as the file
//! stores it, posed by its skin, and the caller places it.
//!
//! The pose is the first frame of the model's animations, which
//! `settleConstantSourceAnimations` applies to the one-key weapon clips;
//! where a node has no channel it keeps its rest transform. A skinned
//! primitive is skinned once here, with that pose (Babylon skins on the GPU
//! every frame, to the same result while the pose holds). The authored
//! `tag_*` nodes are kept as attachment points
//! (`collectModelAttachmentPoints`), with the posed transforms.

use std::collections::BTreeMap;

use glam::{Mat4, Quat, Vec3};

use super::glb::{Glb, GlbError};

/// glTF `alphaMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaMode {
    #[default]
    Opaque,
    Mask,
    Blend,
}

/// A material as Babylon's glTF loader builds its `PBRMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelMaterial {
    pub name: String,
    /// `albedoColor` and `alpha`.
    pub base_color: [f32; 4],
    /// The image index of `albedoTexture`.
    pub base_color_image: Option<usize>,
    /// glTF defaults both to 1.
    pub metallic: f32,
    pub roughness: f32,
    pub metallic_roughness_image: Option<usize>,
    pub normal_image: Option<usize>,
    pub emissive: [f32; 3],
    pub emissive_image: Option<usize>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
}

/// An image as the file stores it (WebP, PNG or JPEG bytes).
#[derive(Debug, Clone)]
pub struct ModelImage {
    pub name: String,
    pub mime_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// One Babylon mesh of the model: a glTF primitive, skinned to the pose.
#[derive(Debug, Clone)]
pub struct ModelPrimitive {
    /// Babylon's mesh name: the node's name, or `node<i>`, with
    /// `_primitive<j>` when the glTF mesh has more than one primitive.
    pub name: String,
    pub node: usize,
    /// The node's own scaling, which a flattened clone keeps.
    pub node_scale: Vec3,
    /// Where the node puts the primitive in glTF scene space, for a model
    /// kept whole (a barrel): the node's posed global transform, or identity
    /// for a skinned primitive, which its skin already placed.
    pub node_matrix: Mat4,
    /// glTF space.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: Option<usize>,
    /// The centre of the box around the primitive's vertices, skinned:
    /// Babylon's bounding sphere centre for its one global sub-mesh (the
    /// glTF loader refreshes a skinned mesh's bounds with its skeleton
    /// applied), which orders transparent draws. Babylon takes the
    /// skeleton's pose at load; the weapons' poses are their rest poses.
    /// Primitives sharing one vertex accessor share this centre.
    pub bounds_center: Vec3,
    /// When Babylon created this mesh: its place in a depth-first walk of
    /// the scene's nodes, which breaks ties in the transparent sort.
    pub creation_order: usize,
}

/// A `tag_*` node (`ModelAttachmentPoint`): its local position and rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttachmentPoint {
    pub position: Vec3,
    /// The glTF node's rotation, as Babylon's `rotationQuaternion` holds it.
    pub rotation: Quat,
}

#[derive(Debug, Clone)]
pub struct GltfModel {
    /// Renderable meshes in the loader's order; the first is the leader.
    pub primitives: Vec<ModelPrimitive>,
    pub materials: Vec<ModelMaterial>,
    pub images: Vec<ModelImage>,
    pub attachment_points: BTreeMap<String, AttachmentPoint>,
}

#[derive(Debug, Clone, Copy)]
struct Trs {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl Trs {
    fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// The posed local transforms of every node.
fn posed_nodes(glb: &Glb) -> Result<Vec<Trs>, GlbError> {
    let doc = &glb.document;
    let mut nodes: Vec<Trs> = doc
        .nodes
        .iter()
        .map(|n| {
            if let Some(m) = n.matrix {
                let (scale, rotation, translation) = Mat4::from_cols_array(&m).to_scale_rotation_translation();
                Trs {
                    translation,
                    rotation,
                    scale,
                }
            } else {
                Trs {
                    translation: n.translation.map_or(Vec3::ZERO, Vec3::from),
                    rotation: n.rotation.map_or(Quat::IDENTITY, Quat::from_array),
                    scale: n.scale.map_or(Vec3::ONE, Vec3::from),
                }
            }
        })
        .collect();
    for animation in &doc.animations {
        for channel in &animation.channels {
            let Some(node) = channel.target.node.and_then(|n| nodes.get_mut(n)) else {
                continue;
            };
            let Some(sampler) = animation.samplers.get(channel.sampler) else {
                continue;
            };
            let (components, values) = glb.read_floats(sampler.output)?;
            // A cubic spline key stores in-tangent, value, out-tangent.
            let first = if sampler.interpolation.as_deref() == Some("CUBICSPLINE") {
                components
            } else {
                0
            };
            let Some(v) = values.get(first..first + components) else {
                continue;
            };
            match (channel.target.path.as_str(), components) {
                ("translation", 3) => node.translation = Vec3::new(v[0], v[1], v[2]),
                ("rotation", 4) => node.rotation = Quat::from_xyzw(v[0], v[1], v[2], v[3]),
                ("scale", 3) => node.scale = Vec3::new(v[0], v[1], v[2]),
                _ => {}
            }
        }
    }
    Ok(nodes)
}

/// Each node's transform in glTF scene space.
fn global_matrices(glb: &Glb, locals: &[Trs]) -> Vec<Mat4> {
    let doc = &glb.document;
    let mut parent = vec![None; doc.nodes.len()];
    for (i, n) in doc.nodes.iter().enumerate() {
        for &c in &n.children {
            if let Some(p) = parent.get_mut(c) {
                *p = Some(i);
            }
        }
    }
    (0..doc.nodes.len())
        .map(|i| {
            let mut m = locals[i].matrix();
            let mut at = parent[i];
            let mut guard = 0;
            while let Some(p) = at {
                m = locals[p].matrix() * m;
                at = parent[p];
                guard += 1;
                if guard > doc.nodes.len() {
                    break;
                }
            }
            m
        })
        .collect()
}

fn vec3s(values: &[f32]) -> Vec<[f32; 3]> {
    values.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

impl GltfModel {
    /// Loads a `.glb`.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, GlbError> {
        let glb = Glb::parse(bytes)?;
        let doc = &glb.document;
        let locals = posed_nodes(&glb)?;
        let globals = global_matrices(&glb, &locals);

        let mut skins = Vec::new();
        for skin in &doc.skins {
            let inverse_binds = match skin.inverse_bind_matrices {
                Some(a) => glb
                    .read_floats(a)?
                    .1
                    .chunks_exact(16)
                    .map(|c| Mat4::from_cols_slice(c))
                    .collect(),
                None => vec![Mat4::IDENTITY; skin.joints.len()],
            };
            let joints: Vec<Mat4> = skin
                .joints
                .iter()
                .enumerate()
                .map(|(j, &node)| {
                    globals.get(node).copied().unwrap_or(Mat4::IDENTITY)
                        * inverse_binds.get(j).copied().unwrap_or(Mat4::IDENTITY)
                })
                .collect();
            skins.push(joints);
        }

        // Babylon loads the scene's nodes depth first, in the scene's order.
        let mut creation = vec![usize::MAX; doc.nodes.len()];
        let roots = doc
            .scene
            .and_then(|s| doc.scenes.get(s))
            .or(doc.scenes.first())
            .map(|s| s.nodes.clone())
            .unwrap_or_default();
        let mut stack: Vec<usize> = roots.into_iter().rev().collect();
        let mut next = 0;
        while let Some(n) = stack.pop() {
            let Some(slot) = creation.get_mut(n) else { continue };
            if *slot != usize::MAX {
                continue;
            }
            *slot = next;
            next += 1;
            stack.extend(doc.nodes[n].children.iter().rev());
        }

        let mut primitives = Vec::new();
        for (node_index, node) in doc.nodes.iter().enumerate() {
            let Some(mesh) = node.mesh.and_then(|m| doc.meshes.get(m)) else {
                continue;
            };
            let node_name = node.name.clone().unwrap_or_else(|| format!("node{node_index}"));
            let several = mesh.primitives.len() > 1;
            for (p, primitive) in mesh.primitives.iter().enumerate() {
                if primitive.mode.unwrap_or(4) != 4 {
                    continue;
                }
                let Some(&position) = primitive.attributes.get("POSITION") else {
                    continue;
                };
                let mut positions = vec3s(&glb.read_floats(position)?.1);
                if positions.is_empty() {
                    continue;
                }
                let mut normals = match primitive.attributes.get("NORMAL") {
                    Some(&a) => vec3s(&glb.read_floats(a)?.1),
                    None => vec![[0.0, 1.0, 0.0]; positions.len()],
                };
                let uvs = match primitive.attributes.get("TEXCOORD_0") {
                    Some(&a) => glb.read_floats(a)?.1.chunks_exact(2).map(|c| [c[0], c[1]]).collect(),
                    None => vec![[0.0, 0.0]; positions.len()],
                };
                let indices = match primitive.indices {
                    Some(a) => glb.read_uints(a)?,
                    None => (0..positions.len() as u32).collect(),
                };
                let mut node_matrix = globals[node_index];
                if let (Some(joints), Some(&ja), Some(&wa)) = (
                    node.skin.and_then(|s| skins.get(s)),
                    primitive.attributes.get("JOINTS_0"),
                    primitive.attributes.get("WEIGHTS_0"),
                ) {
                    node_matrix = Mat4::IDENTITY;
                    let joint_ids = glb.read_uints(ja)?;
                    let weights = glb.read_floats(wa)?.1;
                    for (v, (pos, nor)) in positions.iter_mut().zip(normals.iter_mut()).enumerate() {
                        let mut skin = Mat4::ZERO;
                        for k in 0..4 {
                            let w = weights.get(v * 4 + k).copied().unwrap_or(0.0);
                            if w == 0.0 {
                                continue;
                            }
                            let j = joint_ids.get(v * 4 + k).copied().unwrap_or(0) as usize;
                            skin += joints.get(j).copied().unwrap_or(Mat4::IDENTITY) * w;
                        }
                        let p = skin.transform_point3(Vec3::from(*pos));
                        let n = skin.transform_vector3(Vec3::from(*nor)).normalize_or_zero();
                        *pos = p.into();
                        *nor = n.into();
                    }
                }
                let bounds_center = {
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for &p in &positions {
                        min = min.min(Vec3::from(p));
                        max = max.max(Vec3::from(p));
                    }
                    if min.x.is_finite() {
                        (min + max) * 0.5
                    } else {
                        Vec3::ZERO
                    }
                };
                primitives.push(ModelPrimitive {
                    name: if several {
                        format!("{node_name}_primitive{p}")
                    } else {
                        node_name.clone()
                    },
                    node: node_index,
                    node_scale: locals[node_index].scale,
                    node_matrix,
                    positions,
                    normals,
                    uvs,
                    indices,
                    material: primitive.material,
                    bounds_center,
                    creation_order: creation[node_index].saturating_mul(64).saturating_add(p),
                });
            }
        }

        let image_of = |info: &Option<super::glb::TextureInfo>| {
            info.as_ref()
                .and_then(|t| doc.textures.get(t.index))
                .and_then(super::glb::Texture::image)
        };
        let materials = doc
            .materials
            .iter()
            .map(|m| {
                let pbr = m.pbr_metallic_roughness.clone().unwrap_or_default();
                ModelMaterial {
                    name: m.name.clone().unwrap_or_default(),
                    base_color: pbr.base_color_factor.unwrap_or([1.0; 4]),
                    base_color_image: image_of(&pbr.base_color_texture),
                    metallic: pbr.metallic_factor.unwrap_or(1.0),
                    roughness: pbr.roughness_factor.unwrap_or(1.0),
                    metallic_roughness_image: image_of(&pbr.metallic_roughness_texture),
                    normal_image: image_of(&m.normal_texture),
                    emissive: m.emissive_factor.unwrap_or([0.0; 3]),
                    emissive_image: image_of(&m.emissive_texture),
                    alpha_mode: match m.alpha_mode.as_deref() {
                        Some("MASK") => AlphaMode::Mask,
                        Some("BLEND") => AlphaMode::Blend,
                        _ => AlphaMode::Opaque,
                    },
                    alpha_cutoff: m.alpha_cutoff.unwrap_or(0.5),
                    double_sided: m.double_sided,
                }
            })
            .collect();

        let mut images = Vec::new();
        for (i, image) in doc.images.iter().enumerate() {
            let bytes = match image.buffer_view {
                Some(view) => glb.view_bytes(view)?.to_vec(),
                None => Vec::new(),
            };
            images.push(ModelImage {
                name: image.name.clone().unwrap_or_else(|| format!("image{i}")),
                mime_type: image.mime_type.clone(),
                bytes,
            });
        }

        let mut attachment_points = BTreeMap::new();
        for (i, node) in doc.nodes.iter().enumerate() {
            let Some(name) = node.name.as_deref() else { continue };
            if !name.starts_with("tag_") {
                continue;
            }
            attachment_points.insert(
                name.to_owned(),
                AttachmentPoint {
                    position: locals[i].translation,
                    rotation: locals[i].rotation,
                },
            );
        }

        Ok(Self {
            primitives,
            materials,
            images,
            attachment_points,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle skinned model: joint 0 at rest (+1 on X) with an
    /// animation keyed to +3 on X, its inverse bind undoing the rest; a
    /// `tag_barrel` child; a WebP-only texture.
    fn glb() -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let f = |bin: &mut Vec<u8>, values: &[f32]| {
            for v in values {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        };
        // 0: positions (3 x VEC3) at 0
        f(&mut bin, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        // 1: normals at 36
        f(&mut bin, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        // 2: weights at 72 (3 x VEC4)
        f(&mut bin, &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        // 3: inverse bind (MAT4) at 120: translate -1 on X
        f(
            &mut bin,
            &[
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0,
            ],
        );
        // 4: key time at 184, 5: key translation at 188
        f(&mut bin, &[0.0]);
        f(&mut bin, &[3.0, 0.0, 0.0]);
        // 6: joints (u8 VEC4) at 200, 7: indices (u16) at 212
        bin.extend_from_slice(&[0u8; 12]);
        for i in [0u16, 1, 2, 0] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        // 8: image bytes at 220
        bin.extend_from_slice(b"RIFF\0\0\0\0WEBP");
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let json = r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [2, 0]}],
            "nodes": [
                {"name": "joint", "translation": [1, 0, 0], "children": [1]},
                {"name": "tag_barrel", "translation": [0, 2, 0], "rotation": [0, 0, 0.7071068, 0.7071068]},
                {"mesh": 0, "skin": 0, "translation": [100, 0, 0]}
            ],
            "meshes": [{"name": "m", "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "WEIGHTS_0": 2, "JOINTS_0": 6}, "indices": 7, "material": 0}]}],
            "skins": [{"joints": [0], "inverseBindMatrices": 3}],
            "animations": [{"channels": [{"sampler": 0, "target": {"node": 0, "path": "translation"}}],
                            "samplers": [{"input": 4, "output": 5}]}],
            "materials": [{"name": "models/x/x_glow", "pbrMetallicRoughness": {"metallicFactor": 0, "baseColorTexture": {"index": 0}}}],
            "textures": [{"sampler": 0, "extensions": {"EXT_texture_webp": {"source": 0}}}],
            "images": [{"name": "x", "bufferView": 8, "mimeType": "image/webp"}],
            "samplers": [{}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4"},
                {"bufferView": 3, "componentType": 5126, "count": 1, "type": "MAT4"},
                {"bufferView": 4, "componentType": 5126, "count": 1, "type": "SCALAR"},
                {"bufferView": 5, "componentType": 5126, "count": 1, "type": "VEC3"},
                {"bufferView": 6, "componentType": 5121, "count": 3, "type": "VEC4"},
                {"bufferView": 7, "componentType": 5123, "count": 3, "type": "SCALAR"}
            ],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 36},
                {"buffer": 0, "byteOffset": 72, "byteLength": 48},
                {"buffer": 0, "byteOffset": 120, "byteLength": 64},
                {"buffer": 0, "byteOffset": 184, "byteLength": 4},
                {"buffer": 0, "byteOffset": 188, "byteLength": 12},
                {"buffer": 0, "byteOffset": 200, "byteLength": 12},
                {"buffer": 0, "byteOffset": 212, "byteLength": 6},
                {"buffer": 0, "byteOffset": 220, "byteLength": 12}
            ],
            "buffers": [{"byteLength": BIN}]
        }"#
        .replace("BIN", &bin.len().to_string());
        let mut json = json.into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn skins_to_the_first_animation_frame() {
        let model = GltfModel::from_glb(&glb()).unwrap();
        assert_eq!(model.primitives.len(), 1);
        let p = &model.primitives[0];
        assert_eq!(p.name, "node2");
        assert_eq!(p.indices, vec![0, 1, 2]);
        // The joint moved from +1 to +3: the skin moves every vertex +2.
        assert_eq!(p.positions, vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]]);
        assert_eq!(p.normals[0], [0.0, 0.0, 1.0]);
        // A skinned primitive ignores its node's transform.
        assert_eq!(p.node_matrix, Mat4::IDENTITY);
        assert_eq!(p.bounds_center, Vec3::new(2.5, 0.5, 0.0));
        // Scene order [2, 0]: the mesh node is created first.
        assert_eq!(p.creation_order, 0);
    }

    #[test]
    fn keeps_tags_materials_and_webp_images() {
        let model = GltfModel::from_glb(&glb()).unwrap();
        let tag = model.attachment_points["tag_barrel"];
        assert_eq!(tag.position, Vec3::new(0.0, 2.0, 0.0));
        assert!((tag.rotation * Vec3::X - Vec3::Y).length() < 1e-6);
        let m = &model.materials[0];
        assert_eq!(m.name, "models/x/x_glow");
        assert_eq!(m.base_color_image, Some(0));
        assert_eq!(m.metallic, 0.0);
        assert_eq!(m.roughness, 1.0);
        assert_eq!(model.images[0].bytes, b"RIFF\0\0\0\0WEBP");
    }

    #[test]
    fn refuses_what_is_not_a_glb() {
        assert!(GltfModel::from_glb(b"not a glb at all").is_err());
    }
}
