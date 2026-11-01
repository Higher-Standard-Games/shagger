//! The static world as vertex and index buffers, grouped into draw batches
//! the way SHAG's `BspMeshBuilder.buildAll` groups them (one batch per
//! texture, lightmap page and patch-ness; layered lightmap pages share a
//! batch, with the page folded into lightmap U as `u + 2 * layer`).
//!
//! Each texture's shader resolves to its material passes
//! (`Q3ShaderRegistry.resolveMaterialPasses`); a face is sky when its shader
//! says so or the BSP's `SURF_SKY` flag is set, as `buildAll` decides.
//!
//! Autosprite faces get batches of their own after the rest, as `buildAll`
//! makes their meshes: one per texture when every pass adds, else one per
//! face, without lightmaps, with the billboard attributes
//! (`autospriteVertexData`) the vertex shader turns them with.
//!
//! Not yet followed from `buildAll`: liquids, cubemap objects, fog and
//! spatial cells. Billboards are skipped, as SHAG skips them.

use bytemuck::{Pod, Zeroable};

use crate::loader::bsp::bsp_lighting_profile::bsp_lighting_profile;
use crate::loader::bsp::bsp_loader::BspFile;
use crate::loader::bsp::mesh::autosprite_records::{autosprite_record, autosprite_vertex_data};
use crate::loader::bsp::mesh::face_builders::{build_mesh_face, build_patch_face, build_polygon_face};
use crate::loader::bsp::mesh::lightmaps::scale_bsp_vertex_colors;
use crate::loader::shader::q3_shader_types::{AlphaMode, ResolveMaterialOptions, ResolvedMaterialProps};

/// `NON_DRAWABLE_TEXTURES`: tool textures, matched anywhere in the lower-cased name.
pub const NON_DRAWABLE_TEXTURES: [&str; 14] = [
    "common/nodraw",
    "common/clip",
    "common/caulk",
    "common/trigger",
    "common/hint",
    "common/skip",
    "common/areaportal",
    "common/clusterportal",
    "common/donotenter",
    "common/origin",
    "common/noimpact",
    "common/invisible",
    "textures/common/",
    "noshader",
];

/// `SURF_SKY`.
pub const SURF_SKY: i32 = 0x4;

/// One vertex as the world shader reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct WorldVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Lightmap UV; on a layered page, U carries `2 * layer`.
    pub uv2: [f32; 2],
    /// Baked vertex colour, scaled as `scaleBspVertexColors` scales it.
    pub color: [f32; 4],
    /// 1 when the face reads a lightmap page, 0 when it is vertex-lit.
    pub lightmapped: f32,
    /// `q3SpriteCenter`, `q3SpriteFrame`, `q3SpriteRight`
    /// (`Q3AutospritePlugin`): zero for a face that does not turn.
    pub sprite_center: [f32; 4],
    pub sprite_frame: [f32; 4],
    pub sprite_right: [f32; 4],
}

/// A run of indices drawn with one texture's material passes.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldBatch {
    pub texture_index: i32,
    pub texture_name: String,
    /// Patches draw both sides (`backFaceCulling: false`), as do shaders
    /// with `cull none`.
    pub two_sided: bool,
    /// The material passes, in drawing order (one unless the shader layers
    /// several).
    pub passes: Vec<ResolvedMaterialProps>,
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct WorldGeometry {
    pub vertices: Vec<WorldVertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<WorldBatch>,
    /// Faces left out because they are sky.
    pub sky_faces: Vec<usize>,
    /// Lightmap pages per layer (2 when deluxe pages sit between them).
    pub lightmap_stride: usize,
    /// Faces drawn as autosprites.
    pub sprite_faces: usize,
}

/// `shouldRenderTexture` with `filterTextures` on.
#[must_use]
pub fn should_render_texture(bsp: &BspFile, texture_index: i32) -> bool {
    let Some(texture) = usize::try_from(texture_index)
        .ok()
        .and_then(|i| bsp.common.textures.get(i))
    else {
        return false;
    };
    let name = texture.name.to_lowercase();
    !NON_DRAWABLE_TEXTURES.iter().any(|skip| name.contains(skip))
}

struct Group {
    texture_index: i32,
    two_sided: bool,
    vertices: Vec<WorldVertex>,
    indices: Vec<u32>,
    /// An autosprite batch: its positions and its faces (positions, centre,
    /// autosprite2, first vertex, indices in the batch).
    sprites: Option<SpriteGroup>,
}

/// One autosprite face of a batch: positions, centre, autosprite2, first
/// vertex and indices in the batch.
type SpriteFace = (Vec<f64>, [f64; 3], bool, usize, Vec<u32>);

#[derive(Default)]
struct SpriteGroup {
    positions: Vec<f64>,
    faces: Vec<SpriteFace>,
}

/// Builds the world model's (model 0's) surfaces. `material_passes` resolves
/// a texture name to its passes (a `Q3ShaderRegistry::resolve_material_passes`);
/// it is asked once per texture, with `worldSurface` set and the map's
/// deluxe state.
#[must_use]
pub fn build_world_geometry(
    bsp: &BspFile,
    mut material_passes: impl FnMut(&str, &ResolveMaterialOptions) -> Vec<ResolvedMaterialProps>,
) -> WorldGeometry {
    let profile = bsp_lighting_profile(bsp);
    let stride = if profile.deluxe { 2 } else { 1 };
    let vertex_gain = if bsp.common.header.is_fbsp { profile.gain } else { 1.0 };
    let mut out = WorldGeometry {
        lightmap_stride: stride,
        ..WorldGeometry::default()
    };
    let Some(world) = bsp.common.models.first() else {
        return out;
    };
    let options = ResolveMaterialOptions {
        world_surface: true,
        deluxe: profile.deluxe,
        ..ResolveMaterialOptions::default()
    };
    let mut passes_by_texture: std::collections::HashMap<i32, Vec<ResolvedMaterialProps>> =
        std::collections::HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut keys: std::collections::HashMap<(i32, i32, bool), usize> = std::collections::HashMap::new();
    let mut sprite_groups: Vec<Group> = Vec::new();
    let mut sprite_keys: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    let first = usize::try_from(world.first_face).unwrap_or(0);
    let count = usize::try_from(world.num_faces).unwrap_or(0);
    for i in first..first + count {
        let Some(face) = bsp.common.faces.get(i) else { continue };
        if !should_render_texture(bsp, face.texture_index) {
            continue;
        }
        let texture = &bsp.common.textures[face.texture_index as usize];
        let passes = passes_by_texture.entry(face.texture_index).or_insert_with(|| {
            let mut passes = material_passes(&texture.name, &options);
            if passes.is_empty() {
                passes.push(ResolvedMaterialProps::default());
            }
            passes
        });
        // Every pass shares the shader-level flags (sky, cull).
        let two_sided_shader = !passes[0].back_face_culling;
        if passes[0].is_sky || texture.surface_flags & SURF_SKY != 0 {
            out.sky_faces.push(i);
            continue;
        }
        let built = match face.face_type {
            1 => build_polygon_face(bsp, face),
            2 => build_patch_face(bsp, face),
            3 => build_mesh_face(bsp, face),
            _ => None,
        };
        let Some(g) = built else { continue };
        if passes[0].autosprite || passes[0].autosprite2 {
            if g.positions.is_empty() {
                continue;
            }
            // Additive sprites of a texture share a batch; others draw alone, in face order.
            let additive = passes.iter().all(|p| p.alpha_mode == AlphaMode::Add);
            let new_group = || Group {
                texture_index: face.texture_index,
                two_sided: two_sided_shader,
                vertices: Vec::new(),
                indices: Vec::new(),
                sprites: Some(SpriteGroup::default()),
            };
            let at = if additive {
                *sprite_keys.entry(face.texture_index).or_insert_with(|| {
                    sprite_groups.push(new_group());
                    sprite_groups.len() - 1
                })
            } else {
                sprite_groups.push(new_group());
                sprite_groups.len() - 1
            };
            let group = &mut sprite_groups[at];
            let sprites = group.sprites.as_mut().expect("a sprite group");
            let start = group.vertices.len();
            let n = g.vertex_count();
            let mut sum = [0.0f64; 3];
            for v in g.positions.chunks_exact(3) {
                for k in 0..3 {
                    sum[k] += v[k];
                }
            }
            let center = sum.map(|c| c / n as f64);
            let colors = scale_bsp_vertex_colors(&g.colors, vertex_gain);
            for v in 0..n {
                let f = |a: &[f64], k: usize, w: usize| a[v * w + k] as f32;
                group.vertices.push(WorldVertex {
                    position: [f(&g.positions, 0, 3), f(&g.positions, 1, 3), f(&g.positions, 2, 3)],
                    normal: [f(&g.normals, 0, 3), f(&g.normals, 1, 3), f(&g.normals, 2, 3)],
                    uv: [f(&g.uvs, 0, 2), f(&g.uvs, 1, 2)],
                    uv2: [f(&g.uv2s, 0, 2), f(&g.uv2s, 1, 2)],
                    color: [f(&colors, 0, 4), f(&colors, 1, 4), f(&colors, 2, 4), f(&colors, 3, 4)],
                    // Sprites typically don't use lightmaps (lightmapIndex -1).
                    lightmapped: 0.0,
                    sprite_center: [0.0; 4],
                    sprite_frame: [0.0; 4],
                    sprite_right: [0.0; 4],
                });
            }
            let own: Vec<u32> = g.indices.iter().map(|i| i + start as u32).collect();
            group.indices.extend(&own);
            sprites.positions.extend(&g.positions);
            sprites
                .faces
                .push((g.positions.clone(), center, passes[0].autosprite2, start, own));
            continue;
        }
        // Every page is a layer here (SHAG layers FBSP pages; an IBSP map's
        // per-page textures give the same picture).
        let lightmap = face.lightmap_index;
        let layered = lightmap >= 0;
        let is_patch = face.face_type == 2;
        let key = (face.texture_index, if layered { -2 } else { lightmap }, is_patch);
        let at = *keys.entry(key).or_insert_with(|| {
            groups.push(Group {
                texture_index: face.texture_index,
                two_sided: is_patch || two_sided_shader,
                vertices: Vec::new(),
                indices: Vec::new(),
                sprites: None,
            });
            groups.len() - 1
        });
        let group = &mut groups[at];
        let layer_offset = if layered {
            2.0 * f64::from(lightmap / stride as i32)
        } else {
            0.0
        };
        let colors = scale_bsp_vertex_colors(&g.colors, vertex_gain);
        let base = group.vertices.len() as u32;
        for v in 0..g.vertex_count() {
            let f = |a: &[f64], k: usize, n: usize| a[v * n + k] as f32;
            group.vertices.push(WorldVertex {
                position: [f(&g.positions, 0, 3), f(&g.positions, 1, 3), f(&g.positions, 2, 3)],
                normal: [f(&g.normals, 0, 3), f(&g.normals, 1, 3), f(&g.normals, 2, 3)],
                uv: [f(&g.uvs, 0, 2), f(&g.uvs, 1, 2)],
                uv2: [(g.uv2s[v * 2] + layer_offset) as f32, f(&g.uv2s, 1, 2)],
                color: [f(&colors, 0, 4), f(&colors, 1, 4), f(&colors, 2, 4), f(&colors, 3, 4)],
                lightmapped: if layered { 1.0 } else { 0.0 },
                sprite_center: [0.0; 4],
                sprite_frame: [0.0; 4],
                sprite_right: [0.0; 4],
            });
        }
        group.indices.extend(g.indices.iter().map(|i| i + base));
    }
    for group in &mut sprite_groups {
        let sprites = group.sprites.as_ref().expect("a sprite group");
        let records: Vec<_> = sprites
            .faces
            .iter()
            .map(|(positions, center, autosprite2, start, own)| {
                autosprite_record(positions, *center, *autosprite2, *start, &sprites.positions, own)
            })
            .collect();
        let data = autosprite_vertex_data(&records, &sprites.positions);
        for (v, vertex) in group.vertices.iter_mut().enumerate() {
            let at = v * 4;
            vertex.sprite_center.copy_from_slice(&data.center[at..at + 4]);
            vertex.sprite_frame.copy_from_slice(&data.frame[at..at + 4]);
            vertex.sprite_right.copy_from_slice(&data.right[at..at + 4]);
        }
        out.sprite_faces += sprites.faces.len();
    }
    for group in groups.into_iter().chain(sprite_groups) {
        let base = out.vertices.len() as u32;
        out.batches.push(WorldBatch {
            texture_index: group.texture_index,
            texture_name: bsp.common.textures[group.texture_index as usize].name.clone(),
            two_sided: group.two_sided,
            passes: passes_by_texture[&group.texture_index].clone(),
            first_index: out.indices.len() as u32,
            index_count: group.indices.len() as u32,
        });
        out.vertices.extend(group.vertices);
        out.indices.extend(group.indices.iter().map(|i| i + base));
    }
    out
}
