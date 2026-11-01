//! The static world as vertex and index buffers, grouped into draw batches
//! the way SHAG's `BspMeshBuilder.buildAll` groups them (one batch per
//! texture, lightmap page and patch-ness; layered lightmap pages share a
//! batch, with the page folded into lightmap U as `u + 2 * layer`).
//!
//! Not yet followed from `buildAll` (they need the Q3 shader registry): sky
//! faces named by a shader rather than the BSP's `SURF_SKY` flag, autosprites,
//! liquids, cubemap objects, fog and spatial cells. Billboards are skipped, as
//! SHAG skips them.

use bytemuck::{Pod, Zeroable};

use crate::loader::bsp::bsp_lighting_profile::bsp_lighting_profile;
use crate::loader::bsp::bsp_loader::BspFile;
use crate::loader::bsp::mesh::face_builders::{build_mesh_face, build_patch_face, build_polygon_face};
use crate::loader::bsp::mesh::lightmaps::scale_bsp_vertex_colors;

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
}

/// A run of indices drawn with one base texture.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldBatch {
    pub texture_index: i32,
    pub texture_name: String,
    /// Patches draw both sides (`backFaceCulling: false`).
    pub two_sided: bool,
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
}

/// Builds the world model's (model 0's) surfaces.
#[must_use]
pub fn build_world_geometry(bsp: &BspFile) -> WorldGeometry {
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
    let mut groups: Vec<Group> = Vec::new();
    let mut keys: std::collections::HashMap<(i32, i32, bool), usize> = std::collections::HashMap::new();
    let first = usize::try_from(world.first_face).unwrap_or(0);
    let count = usize::try_from(world.num_faces).unwrap_or(0);
    for i in first..first + count {
        let Some(face) = bsp.common.faces.get(i) else { continue };
        if !should_render_texture(bsp, face.texture_index) {
            continue;
        }
        let texture = &bsp.common.textures[face.texture_index as usize];
        if texture.surface_flags & SURF_SKY != 0 {
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
        // Every page is a layer here (SHAG layers FBSP pages; an IBSP map's
        // per-page textures give the same picture).
        let lightmap = face.lightmap_index;
        let layered = lightmap >= 0;
        let is_patch = face.face_type == 2;
        let key = (face.texture_index, if layered { -2 } else { lightmap }, is_patch);
        let at = *keys.entry(key).or_insert_with(|| {
            groups.push(Group {
                texture_index: face.texture_index,
                two_sided: is_patch,
                vertices: Vec::new(),
                indices: Vec::new(),
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
            });
        }
        group.indices.extend(g.indices.iter().map(|i| i + base));
    }
    for group in groups {
        let base = out.vertices.len() as u32;
        out.batches.push(WorldBatch {
            texture_index: group.texture_index,
            texture_name: bsp.common.textures[group.texture_index as usize].name.clone(),
            two_sided: group.two_sided,
            first_index: out.indices.len() as u32,
            index_count: group.indices.len() as u32,
        });
        out.vertices.extend(group.vertices);
        out.indices.extend(group.indices.iter().map(|i| i + base));
    }
    out
}
