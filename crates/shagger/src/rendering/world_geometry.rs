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
//! The `func_explosive` brush models (`BspEntityHandlers.handleExplosive`,
//! which builds each with `buildBrushModel` and places it at the entity's
//! origin) get batches of their own, tagged with their model index, so the
//! renderer can drop the ones the bomb destroyed (`BrushModelState`).
//! The `func_static` brush models (`handleStatic`: scenery kept out of the
//! world model, such as a structure standing in the sky) are built where
//! they stand, with the world's batches, as nothing ever removes them.
//!
//! Not yet followed from `buildAll`: liquids, cubemap objects, fog and
//! spatial cells. Billboards are skipped, as SHAG skips them.

use bytemuck::{Pod, Zeroable};

use crate::loader::bsp::bsp_lighting_profile::bsp_lighting_profile;
use crate::loader::bsp::bsp_loader::{BspFile, record_range};
use crate::loader::bsp::mesh::autosprite_records::{autosprite_record, autosprite_vertex_data};
use crate::loader::bsp::mesh::face_builders::{build_mesh_face, build_patch_face, build_polygon_face};
use crate::loader::bsp::mesh::lightmaps::scale_bsp_vertex_colors;
use crate::loader::bsp::mesh::pvs::build_face_clusters;
use crate::loader::bsp::mesh::tangents::build_bsp_tangents;
use shagger_shared::bsp::brush_model_state::extract_brush_models;

use crate::loader::shader::q3_shader_types::{
    AlphaMode, Q3Shader, ResolveMaterialOptions, ResolvedMaterialProps, SkyParms,
};
use crate::rendering::world_materials::{GlowOverlay, glow_overlay};

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
    /// `buildBspTangents` over the vertex's mesh: tangent and handedness,
    /// for normal maps.
    pub tangent: [f32; 4],
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
    /// The clusters of its faces (`bspPvsClusters`); `-1` for a face in
    /// none, which keeps the batch drawn.
    pub pvs_clusters: Vec<i32>,
    /// Babylon-space bounds of everything it can draw, for frustum culling
    /// (autosprites grown by the reach of their turn).
    pub bounds: [[f32; 3]; 2],
    /// The `func_explosive` brush model it belongs to; `None` for the world.
    pub brush_model: Option<usize>,
    /// Its shader's glow overlay stages, when `loadTextures` would draw its
    /// pass with the glow overlay ShaderMaterial
    /// ([`WorldGeometry::find_glow_overlays`]).
    pub glow_overlay: Option<GlowOverlay>,
}

/// A sky shader `BspMeshBuilder.applySkybox` can draw the sky from.
#[derive(Debug, Clone, PartialEq)]
pub struct SkyShader {
    pub texture_name: String,
    pub sky_parms: Option<SkyParms>,
}

#[derive(Debug, Clone, Default)]
pub struct WorldGeometry {
    pub vertices: Vec<WorldVertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<WorldBatch>,
    /// Faces left out because they are sky.
    pub sky_faces: Vec<usize>,
    /// The sky shaders, one per texture, in the order their first face is
    /// met (`skyShaders` in `BspMeshBuilder.build`).
    pub sky_shaders: Vec<SkyShader>,
    /// Lightmap pages per layer (2 when deluxe pages sit between them).
    pub lightmap_stride: usize,
    /// Faces drawn as autosprites.
    pub sprite_faces: usize,
}

impl WorldGeometry {
    /// Marks the batches whose shader (`shader` looks one up by texture
    /// name) takes the glow overlay ShaderMaterial in `loadTextures`
    /// ([`glow_overlay`]).
    pub fn find_glow_overlays<'a>(&mut self, shader: impl Fn(&str) -> Option<&'a Q3Shader>) {
        for batch in &mut self.batches {
            batch.glow_overlay = match batch.passes.as_slice() {
                [props] => glow_overlay(shader(&batch.texture_name), props, false),
                _ => None,
            };
        }
    }

    /// The authored cubemaps (`surroundmap`/`cubemap` stages) the passes
    /// draw, each once, in the order first met: what `loadTextures` hands
    /// `SkyboxLoader.loadEnvironmentCubemap`.
    #[must_use]
    pub fn environment_maps(&self) -> Vec<String> {
        let mut maps: Vec<String> = Vec::new();
        for props in self.batches.iter().flat_map(|b| &b.passes) {
            if let Some(map) = props.environment_map.as_ref().filter(|m| !m.is_empty())
                && props.generated_cubemap != Some(true)
                && !maps.contains(map)
            {
                maps.push(map.clone());
            }
        }
        maps
    }

    /// Which batches belong to a `func_explosive` brush model, by batch
    /// index: geometry that comes and goes, which static consumers (the
    /// ragdolls' collision, the water's occluders) leave out.
    #[must_use]
    pub fn brush_model_batches(&self) -> Vec<bool> {
        self.batches.iter().map(|b| b.brush_model.is_some()).collect()
    }
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
    pvs_clusters: Vec<i32>,
    bounds: [[f32; 3]; 2],
    brush_model: Option<usize>,
}

const EMPTY_BOUNDS: [[f32; 3]; 2] = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];

impl Group {
    /// `buildBspTangents` over the group's mesh, as `loadTextures` adds them
    /// to a mesh with a normal map (here to every mesh; only normal-mapped
    /// passes read them).
    fn add_tangents(&mut self) {
        let attribute = |f: fn(&WorldVertex) -> &[f32]| -> Vec<f32> {
            self.vertices.iter().flat_map(|v| f(v).iter().copied()).collect()
        };
        let positions = attribute(|v| &v.position);
        let normals = attribute(|v| &v.normal);
        let uvs = attribute(|v| &v.uv);
        let tangents = build_bsp_tangents(&positions, &normals, &uvs, &self.indices);
        for (vertex, tangent) in self.vertices.iter_mut().zip(tangents.as_chunks::<4>().0.iter()) {
            vertex.tangent.copy_from_slice(tangent);
        }
    }

    fn add_clusters(&mut self, clusters: &[i32]) {
        for &c in clusters {
            if !self.pvs_clusters.contains(&c) {
                self.pvs_clusters.push(c);
            }
        }
    }

    fn grow(&mut self, point: [f32; 3], reach: f32) {
        for (k, &c) in point.iter().enumerate() {
            self.bounds[0][k] = self.bounds[0][k].min(c - reach);
            self.bounds[1][k] = self.bounds[1][k].max(c + reach);
        }
    }
}

/// One autosprite face of a batch: positions, centre, autosprite2, first
/// vertex and indices in the batch.
type SpriteFace = (Vec<f64>, [f64; 3], bool, usize, Vec<u32>);

#[derive(Default)]
struct SpriteGroup {
    positions: Vec<f64>,
    faces: Vec<SpriteFace>,
}

/// `parseModelIndex`: a `*N` model's index (`parseInt`'s leading digits).
#[must_use]
pub fn parse_model_index(model: &str) -> Option<usize> {
    let digits: String = model
        .strip_prefix('*')?
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The `func_static` entities' brush models (`handleStatic`): each `*N`
/// model past the world's, in entity order, once.
#[must_use]
pub fn static_brush_models(bsp: &BspFile) -> Vec<usize> {
    let mut out = Vec::new();
    for entity in &bsp.entities {
        if entity.classname != "func_static" {
            continue;
        }
        let Some(index) = entity.properties.get("model").and_then(parse_model_index) else {
            continue;
        };
        if index >= 1 && index < bsp.common.models.len() && !out.contains(&index) {
            out.push(index);
        }
    }
    out
}

/// Builds the world model's (model 0's) surfaces, then the `func_explosive`
/// brush models' (`extractBrushModels`), offset to their entities' origins. `material_passes` resolves
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
    let face_clusters = build_face_clusters(bsp);
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
    let mut keys: std::collections::HashMap<(i32, i32, bool, Option<usize>), usize> = std::collections::HashMap::new();
    let mut sprite_groups: Vec<Group> = Vec::new();
    let mut sprite_keys: std::collections::HashMap<(i32, Option<usize>), usize> = std::collections::HashMap::new();
    // Every face to build: the world's, then each explosive's with its
    // model index and its Babylon-space origin, then each static's; the
    // last says whether it is a brush model's.
    let mut faces: Vec<(usize, Option<usize>, [f64; 3], bool)> =
        record_range(world.first_face, world.num_faces, bsp.common.faces.len())
            .map(|i| (i, None, [0.0; 3], false))
            .collect();
    for definition in extract_brush_models(&bsp.entities, &bsp.models) {
        let Some(model) = bsp.common.models.get(definition.model_index) else {
            continue;
        };
        let origin = [definition.origin.x, definition.origin.y, definition.origin.z];
        faces.extend(
            record_range(model.first_face, model.num_faces, bsp.common.faces.len())
                .map(|i| (i, Some(definition.model_index), origin, true)),
        );
    }
    for index in static_brush_models(bsp) {
        let model = &bsp.common.models[index];
        faces.extend(
            record_range(model.first_face, model.num_faces, bsp.common.faces.len()).map(|i| (i, None, [0.0; 3], true)),
        );
    }
    for (i, brush_model, origin, in_brush_model) in faces {
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
            // A brush model's sky would hang in the world's sky box.
            if in_brush_model {
                continue;
            }
            if !out.sky_shaders.iter().any(|s| s.texture_name == texture.name) {
                out.sky_shaders.push(SkyShader {
                    texture_name: texture.name.clone(),
                    sky_parms: passes[0].sky_parms.clone(),
                });
            }
            out.sky_faces.push(i);
            continue;
        }
        let built = match face.face_type {
            1 => build_polygon_face(bsp, face),
            2 => build_patch_face(bsp, face),
            3 => build_mesh_face(bsp, face),
            _ => None,
        };
        let Some(mut g) = built else { continue };
        if brush_model.is_some() {
            for p in g.positions.as_chunks_mut::<3>().0.iter_mut() {
                for k in 0..3 {
                    p[k] += origin[k];
                }
            }
        }
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
                pvs_clusters: Vec::new(),
                bounds: EMPTY_BOUNDS,
                brush_model,
            };
            let at = if additive {
                *sprite_keys.entry((face.texture_index, brush_model)).or_insert_with(|| {
                    sprite_groups.push(new_group());
                    sprite_groups.len() - 1
                })
            } else {
                sprite_groups.push(new_group());
                sprite_groups.len() - 1
            };
            let group = &mut sprite_groups[at];
            let start = group.vertices.len();
            let n = g.vertex_count();
            let mut sum = [0.0f64; 3];
            for v in g.positions.as_chunks::<3>().0.iter() {
                for k in 0..3 {
                    sum[k] += v[k];
                }
            }
            let center = sum.map(|c| c / n as f64);
            group.add_clusters(face_clusters.get(i).map_or(&[-1][..], Vec::as_slice));
            // A sprite turns about its centre, so it can reach as far as its
            // farthest corner in any direction.
            let reach = g
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2)).sqrt())
                .fold(0.0f64, f64::max);
            group.grow(center.map(|c| c as f32), reach as f32 + 1.0);
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
                    tangent: [0.0; 4],
                });
            }
            let own: Vec<u32> = g.indices.iter().map(|i| i + start as u32).collect();
            group.indices.extend(&own);
            let sprites = group.sprites.as_mut().expect("a sprite group");
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
        let key = (
            face.texture_index,
            if layered { -2 } else { lightmap },
            is_patch,
            brush_model,
        );
        let at = *keys.entry(key).or_insert_with(|| {
            groups.push(Group {
                texture_index: face.texture_index,
                two_sided: is_patch || two_sided_shader,
                vertices: Vec::new(),
                indices: Vec::new(),
                sprites: None,
                pvs_clusters: Vec::new(),
                bounds: EMPTY_BOUNDS,
                brush_model,
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
        group.add_clusters(face_clusters.get(i).map_or(&[-1][..], Vec::as_slice));
        for p in g.positions.as_chunks::<3>().0.iter() {
            group.grow([p[0] as f32, p[1] as f32, p[2] as f32], 0.0);
        }
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
                tangent: [0.0; 4],
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
    for mut group in groups.into_iter().chain(sprite_groups) {
        group.add_tangents();
        let base = out.vertices.len() as u32;
        out.batches.push(WorldBatch {
            texture_index: group.texture_index,
            texture_name: bsp.common.textures[group.texture_index as usize].name.clone(),
            two_sided: group.two_sided,
            passes: passes_by_texture[&group.texture_index].clone(),
            first_index: out.indices.len() as u32,
            index_count: group.indices.len() as u32,
            pvs_clusters: group.pvs_clusters,
            bounds: group.bounds,
            brush_model: group.brush_model,
            glow_overlay: None,
        });
        out.vertices.extend(group.vertices);
        out.indices.extend(group.indices.iter().map(|i| i + base));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_brush_model_is_named_by_its_leading_digits() {
        assert_eq!(super::parse_model_index("*5"), Some(5));
        assert_eq!(super::parse_model_index("*12abc"), Some(12));
        assert_eq!(super::parse_model_index("models/a.md3"), None);
        assert_eq!(super::parse_model_index("*"), None);
    }
}
