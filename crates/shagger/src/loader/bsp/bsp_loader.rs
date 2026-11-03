//! IBSP (Quake 3, v46) and FBSP (Warsow, v1) map files
//! (SHAG `src/engine/loader/bsp/BspLoader.ts`).
//!
//! A direct translation over `&[u8]`. File floats stay `f32`, as stored, and
//! widen to `f64` where the TypeScript computes with them. Where the
//! TypeScript throws (a lump outside the file, a bad lightmap size), this
//! returns a [`BspError`]; where it only warns, this does nothing.

use shagger_shared::js::{is_js_whitespace, js_parse_float, js_string_to_number, js_trim};
use std::fmt;
use std::ops::Deref;

const BSP_MAGIC_IBSP: u32 = 0x5053_4249;
const BSP_MAGIC_FBSP: u32 = 0x5053_4246;

/// Lump indices, the same for IBSP and FBSP.
pub mod lump {
    pub const ENTITIES: usize = 0;
    pub const TEXTURES: usize = 1;
    pub const PLANES: usize = 2;
    pub const NODES: usize = 3;
    pub const LEAFS: usize = 4;
    pub const LEAFFACES: usize = 5;
    pub const LEAFBRUSHES: usize = 6;
    pub const MODELS: usize = 7;
    pub const BRUSHES: usize = 8;
    pub const BRUSHSIDES: usize = 9;
    pub const VERTICES: usize = 10;
    pub const MESHVERTS: usize = 11;
    pub const EFFECTS: usize = 12;
    pub const FACES: usize = 13;
    pub const LIGHTMAPS: usize = 14;
    pub const LIGHTVOLS: usize = 15;
    pub const VISDATA: usize = 16;
    /// FBSP only.
    pub const LIGHTARRAY: usize = 17;
}

const LUMP_COUNT: usize = 17;

/// `FaceType`.
pub mod face_type {
    pub const POLYGON: i32 = 1;
    pub const PATCH: i32 = 2;
    pub const MESH: i32 = 3;
    pub const BILLBOARD: i32 = 4;
}

const IBSP_LIGHTMAP_SIZE: u32 = 128;
const FBSP_LIGHTMAP_SIZE: u32 = 512;

/// Max lightmaps per surface (FBSP light styles).
pub const MAX_LIGHTMAPS: usize = 4;

/// Why a map could not be read: where the TypeScript throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspError(pub String);

impl fmt::Display for BspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BspError {}

type Result<T> = std::result::Result<T, BspError>;

fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(BspError(message.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspHeader {
    pub magic: u32,
    pub version: i32,
    pub is_fbsp: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspLump {
    pub offset: i32,
    pub length: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspTexture {
    pub name: String,
    pub surface_flags: i32,
    pub content_flags: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspPlane {
    /// Components within 1e-5 of zero are snapped to exactly 0 (Q3 `CM_SnapVector`).
    pub normal: [f32; 3],
    pub dist: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspNode {
    pub plane_index: i32,
    /// Negative means leaf `-(child + 1)`.
    pub children: [i32; 2],
    pub mins: [i32; 3],
    pub maxs: [i32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspLeaf {
    pub cluster: i32,
    pub area: i32,
    pub mins: [i32; 3],
    pub maxs: [i32; 3],
    pub first_leaf_face: i32,
    pub num_leaf_faces: i32,
    pub first_leaf_brush: i32,
    pub num_leaf_brushes: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub first_face: i32,
    pub num_faces: i32,
    pub first_brush: i32,
    pub num_brushes: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspBrush {
    pub first_side: i32,
    pub num_sides: i32,
    pub shader_index: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspBrushSide {
    pub plane_index: i32,
    pub shader_index: i32,
    /// FBSP only.
    pub surface_num: Option<i32>,
}

/// IBSP vertex (Q3 format), 44 bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspVertexIbsp {
    pub position: [f32; 3],
    pub tex_coord: [f32; 2],
    pub lightmap_coord: [f32; 2],
    pub normal: [f32; 3],
    pub color: [u8; 4],
}

/// FBSP vertex (RBSP format), 80 bytes with 4 lightmap slots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspVertexFbsp {
    pub position: [f32; 3],
    pub tex_coord: [f32; 2],
    pub lightmap_coords: [[f32; 2]; 4],
    pub normal: [f32; 3],
    pub colors: [[u8; 4]; 4],
}

/// The vertex lump, in the file's own layout.
#[derive(Debug, Clone, PartialEq)]
pub enum BspVertices {
    Ibsp(Vec<BspVertexIbsp>),
    Fbsp(Vec<BspVertexFbsp>),
}

impl BspVertices {
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Ibsp(v) => v.len(),
            Self::Fbsp(v) => v.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn position(&self, index: usize) -> Option<[f32; 3]> {
        match self {
            Self::Ibsp(v) => v.get(index).map(|v| v.position),
            Self::Fbsp(v) => v.get(index).map(|v| v.position),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspFace {
    pub texture_index: i32,
    pub effect_index: i32,
    /// One of [`face_type`]; the file may hold anything.
    pub face_type: i32,
    pub first_vertex: i32,
    pub num_vertices: i32,
    pub first_mesh_vert: i32,
    pub num_mesh_verts: i32,
    /// IBSP: the lightmap; FBSP: the first of up to 4.
    pub lightmap_index: i32,
    /// FBSP only.
    pub lightmap_indices: Option<[i32; 4]>,
    pub lightmap_start: [i32; 2],
    pub lightmap_size: [i32; 2],
    pub lightmap_origin: [f32; 3],
    pub lightmap_vecs: [[f32; 3]; 2],
    pub normal: [f32; 3],
    /// Control point grid, for [`face_type::PATCH`].
    pub patch_size: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspEffect {
    pub name: String,
    pub brush_index: i32,
    pub unknown: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspVisData {
    pub num_clusters: i32,
    pub bytes_per_cluster: i32,
    pub bit_vectors: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BspLightVol {
    /// False for an unreachable compressed-grid site, which is left out of
    /// interpolation (`valid: false`; the TypeScript leaves it unset otherwise).
    pub valid: bool,
    /// FBSP sums up to four styles, so this can exceed 255.
    pub ambient: [u16; 3],
    pub directional: [u16; 3],
    /// Phi, theta.
    pub direction: [u8; 2],
}

/// An entity's key/value pairs, like the TypeScript `Record<string, string>`:
/// a repeated key keeps its first position and takes the last value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Properties(Vec<(String, String)>);

impl Properties {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        // A plain object ignores a string assigned to __proto__.
        if key == "__proto__" {
            return;
        }
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => value.clone_into(&mut slot.1),
            None => self.0.push((key.to_owned(), value.to_owned())),
        }
    }

    /// In `Object.entries` order: array-index keys ascending, then the rest
    /// in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut index: Vec<(u32, &(String, String))> = self
            .0
            .iter()
            .filter_map(|kv| array_index(&kv.0).map(|i| (i, kv)))
            .collect();
        index.sort_by_key(|(i, _)| *i);
        index
            .into_iter()
            .map(|(_, kv)| kv)
            .chain(self.0.iter().filter(|kv| array_index(&kv.0).is_none()))
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn array_index(key: &str) -> Option<u32> {
    let n: u32 = key.parse().ok()?;
    (n != u32::MAX && n.to_string() == key).then_some(n)
}

/// A parsed entity from the entities lump.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BspEntity {
    pub classname: String,
    pub properties: Properties,
}

/// The light grid's placement, from the world model's bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightGrid {
    pub origin: [f64; 3],
    /// Sample counts per axis. NaN when the world bounds are NaN.
    pub size: [f64; 3],
    pub step: [f64; 3],
}

/// What both the full and the collision-only parse read.
#[derive(Debug, Clone, PartialEq)]
pub struct BspCommon {
    pub header: BspHeader,
    pub lumps: Vec<BspLump>,
    pub entities: Vec<BspEntity>,
    pub textures: Vec<BspTexture>,
    pub planes: Vec<BspPlane>,
    pub nodes: Vec<BspNode>,
    pub leafs: Vec<BspLeaf>,
    pub leaf_faces: Vec<i32>,
    pub leaf_brushes: Vec<i32>,
    pub models: Vec<BspModel>,
    pub brushes: Vec<BspBrush>,
    pub brush_sides: Vec<BspBrushSide>,
    pub faces: Vec<BspFace>,
    pub vis_data: Option<BspVisData>,
}

/// `BspFile`: everything, for rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct BspFile {
    pub common: BspCommon,
    pub vertices: BspVertices,
    pub mesh_verts: Vec<i32>,
    pub effects: Vec<BspEffect>,
    /// Raw RGB pages, `lightmap_size` square.
    pub lightmaps: Vec<Vec<u8>>,
    pub light_vols: Vec<BspLightVol>,
    /// IBSP: 128; FBSP: the authored size or 512.
    pub lightmap_size: u32,
    /// `None` without a world model.
    pub light_grid: Option<LightGrid>,
}

/// `BspCollisionData`: gameplay data only. `vertices` holds patch control
/// positions at their file index and nothing elsewhere, like the sparse
/// TypeScript array.
#[derive(Debug, Clone, PartialEq)]
pub struct BspCollisionData {
    pub common: BspCommon,
    pub vertices: Vec<Option<[f32; 3]>>,
}

impl Deref for BspFile {
    type Target = BspCommon;
    fn deref(&self) -> &BspCommon {
        &self.common
    }
}

impl Deref for BspCollisionData {
    type Target = BspCommon;
    fn deref(&self) -> &BspCommon {
        &self.common
    }
}

/// `BspLoader.load`.
///
/// # Errors
/// Where SHAG throws.
pub fn load(data: &[u8]) -> Result<BspFile> {
    let loader = Loader::new(data)?;
    let common = loader.parse_common()?;
    let lightmap_size = resolve_lightmap_size(common.header.is_fbsp, &common.entities)?;
    let vertices = loader.parse_vertices()?;
    let mesh_verts = loader.parse_i32s(lump::MESHVERTS)?;
    let effects = loader.parse_effects()?;
    let lightmaps = loader.parse_lightmaps(lightmap_size)?;
    let light_vols = loader.parse_light_vols()?;
    let light_grid = calculate_light_grid(&common.entities, &common.models);
    Ok(BspFile {
        common,
        vertices,
        mesh_verts,
        effects,
        lightmaps,
        light_vols,
        lightmap_size,
        light_grid,
    })
}

/// `BspLoader.loadCollision`: gameplay data without render vertices or lighting.
///
/// # Errors
/// Where SHAG throws.
pub fn load_collision(data: &[u8]) -> Result<BspCollisionData> {
    let loader = Loader::new(data)?;
    let common = loader.parse_common()?;
    let vertices = loader.parse_patch_positions(&common.faces)?;
    Ok(BspCollisionData { common, vertices })
}

struct Loader<'a> {
    data: &'a [u8],
    header: BspHeader,
    lumps: Vec<BspLump>,
}

/// Little-endian reads inside a lump. Callers stay within bounds by
/// construction (counts are whole records), so these index directly.
struct Lump<'a>(&'a [u8]);

impl Lump<'_> {
    fn i32(&self, at: usize) -> i32 {
        i32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }
    fn f32(&self, at: usize) -> f32 {
        f32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }
    fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }
    fn f32s<const N: usize>(&self, at: usize) -> [f32; N] {
        std::array::from_fn(|k| self.f32(at + 4 * k))
    }
    fn i32s<const N: usize>(&self, at: usize) -> [i32; N] {
        std::array::from_fn(|k| self.i32(at + 4 * k))
    }
    fn u8s<const N: usize>(&self, at: usize) -> [u8; N] {
        std::array::from_fn(|k| self.0[at + k])
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn count(&self, size: usize) -> usize {
        self.0.len() / size
    }
}

/// `new DataView(buffer, offset, length)`: a RangeError outside the buffer.
fn view(data: &[u8], offset: i64, length: i64) -> Result<&[u8]> {
    let total = i64::try_from(data.len()).unwrap_or(i64::MAX);
    if offset < 0 || length < 0 || offset > total || offset + length > total {
        return err(format!("Invalid DataView length {length} at offset {offset}"));
    }
    // Both are within 0..=data.len() here.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(&data[offset as usize..(offset + length) as usize])
}

/// `TextDecoder().decode`: lossy UTF-8 with a leading BOM removed.
fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// A 64-byte, NUL-terminated name.
fn name64(bytes: &[u8]) -> String {
    let bytes = &bytes[..64];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(64);
    decode_text(&bytes[..end])
}

impl<'a> Loader<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 8 {
            return err("Offset is outside the bounds of the DataView");
        }
        let head = Lump(data);
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let version = head.i32(4);
        let is_fbsp = match magic {
            BSP_MAGIC_FBSP => true,
            BSP_MAGIC_IBSP => false,
            _ => return err(format!("[BspLoader] Invalid BSP magic: 0x{magic:x}")),
        };
        let count = LUMP_COUNT + usize::from(is_fbsp);
        if data.len() < 8 + 8 * count {
            return err("Offset is outside the bounds of the DataView");
        }
        let lumps = (0..count)
            .map(|i| BspLump {
                offset: head.i32(8 + 8 * i),
                length: head.i32(12 + 8 * i),
            })
            .collect();
        Ok(Self {
            data,
            header: BspHeader {
                magic,
                version,
                is_fbsp,
            },
            lumps,
        })
    }

    fn lump(&self, index: usize) -> Result<Lump<'a>> {
        let l = self.lumps[index];
        view(self.data, i64::from(l.offset), i64::from(l.length)).map(Lump)
    }

    fn parse_common(&self) -> Result<BspCommon> {
        Ok(BspCommon {
            header: self.header,
            lumps: self.lumps.clone(),
            entities: parse_entity_string(&self.parse_entities_text()?),
            textures: self.parse_textures()?,
            planes: self.parse_planes()?,
            nodes: self.parse_nodes()?,
            leafs: self.parse_leafs()?,
            leaf_faces: self.parse_i32s(lump::LEAFFACES)?,
            leaf_brushes: self.parse_i32s(lump::LEAFBRUSHES)?,
            models: self.parse_models()?,
            brushes: self.parse_brushes()?,
            brush_sides: self.parse_brush_sides()?,
            faces: self.parse_faces()?,
            vis_data: self.parse_vis_data()?,
        })
    }

    fn parse_entities_text(&self) -> Result<String> {
        let mut text = decode_text(self.lump(lump::ENTITIES)?.0);
        if text.ends_with('\0') {
            text.pop();
        }
        Ok(text)
    }

    fn parse_textures(&self) -> Result<Vec<BspTexture>> {
        let l = self.lump(lump::TEXTURES)?;
        Ok((0..l.count(72))
            .map(|i| {
                let at = i * 72;
                BspTexture {
                    name: name64(&l.0[at..]),
                    surface_flags: l.i32(at + 64),
                    content_flags: l.i32(at + 68),
                }
            })
            .collect())
    }

    fn parse_planes(&self) -> Result<Vec<BspPlane>> {
        const NORMAL_EPSILON: f64 = 0.000_01;
        let l = self.lump(lump::PLANES)?;
        Ok((0..l.count(16))
            .map(|i| {
                let normal = l
                    .f32s::<3>(i * 16)
                    .map(|n| if f64::from(n).abs() < NORMAL_EPSILON { 0.0 } else { n });
                BspPlane {
                    normal,
                    dist: l.f32(i * 16 + 12),
                }
            })
            .collect())
    }

    fn parse_nodes(&self) -> Result<Vec<BspNode>> {
        let l = self.lump(lump::NODES)?;
        Ok((0..l.count(36))
            .map(|i| {
                let at = i * 36;
                BspNode {
                    plane_index: l.i32(at),
                    children: l.i32s(at + 4),
                    mins: l.i32s(at + 12),
                    maxs: l.i32s(at + 24),
                }
            })
            .collect())
    }

    fn parse_leafs(&self) -> Result<Vec<BspLeaf>> {
        let l = self.lump(lump::LEAFS)?;
        Ok((0..l.count(48))
            .map(|i| {
                let at = i * 48;
                BspLeaf {
                    cluster: l.i32(at),
                    area: l.i32(at + 4),
                    mins: l.i32s(at + 8),
                    maxs: l.i32s(at + 20),
                    first_leaf_face: l.i32(at + 32),
                    num_leaf_faces: l.i32(at + 36),
                    first_leaf_brush: l.i32(at + 40),
                    num_leaf_brushes: l.i32(at + 44),
                }
            })
            .collect())
    }

    fn parse_i32s(&self, index: usize) -> Result<Vec<i32>> {
        let l = self.lump(index)?;
        Ok((0..l.count(4)).map(|i| l.i32(i * 4)).collect())
    }

    fn parse_models(&self) -> Result<Vec<BspModel>> {
        let l = self.lump(lump::MODELS)?;
        Ok((0..l.count(40))
            .map(|i| {
                let at = i * 40;
                BspModel {
                    mins: l.f32s(at),
                    maxs: l.f32s(at + 12),
                    first_face: l.i32(at + 24),
                    num_faces: l.i32(at + 28),
                    first_brush: l.i32(at + 32),
                    num_brushes: l.i32(at + 36),
                }
            })
            .collect())
    }

    fn parse_brushes(&self) -> Result<Vec<BspBrush>> {
        let l = self.lump(lump::BRUSHES)?;
        Ok((0..l.count(12))
            .map(|i| BspBrush {
                first_side: l.i32(i * 12),
                num_sides: l.i32(i * 12 + 4),
                shader_index: l.i32(i * 12 + 8),
            })
            .collect())
    }

    fn parse_brush_sides(&self) -> Result<Vec<BspBrushSide>> {
        let l = self.lump(lump::BRUSHSIDES)?;
        let size = if self.header.is_fbsp { 12 } else { 8 };
        Ok((0..l.count(size))
            .map(|i| {
                let at = i * size;
                BspBrushSide {
                    plane_index: l.i32(at),
                    shader_index: l.i32(at + 4),
                    surface_num: self.header.is_fbsp.then(|| l.i32(at + 8)),
                }
            })
            .collect())
    }

    fn parse_vertices(&self) -> Result<BspVertices> {
        let l = self.lump(lump::VERTICES)?;
        Ok(if self.header.is_fbsp {
            BspVertices::Fbsp(
                (0..l.count(80))
                    .map(|i| {
                        let at = i * 80;
                        BspVertexFbsp {
                            position: l.f32s(at),
                            tex_coord: l.f32s(at + 12),
                            lightmap_coords: std::array::from_fn(|k| l.f32s(at + 20 + 8 * k)),
                            normal: l.f32s(at + 52),
                            colors: std::array::from_fn(|k| l.u8s(at + 64 + 4 * k)),
                        }
                    })
                    .collect(),
            )
        } else {
            BspVertices::Ibsp(
                (0..l.count(44))
                    .map(|i| {
                        let at = i * 44;
                        BspVertexIbsp {
                            position: l.f32s(at),
                            tex_coord: l.f32s(at + 12),
                            lightmap_coord: l.f32s(at + 20),
                            normal: l.f32s(at + 28),
                            color: l.u8s(at + 40),
                        }
                    })
                    .collect(),
            )
        })
    }

    fn parse_patch_positions(&self, faces: &[BspFace]) -> Result<Vec<Option<[f32; 3]>>> {
        let l = self.lump(lump::VERTICES)?;
        let stride = if self.header.is_fbsp { 80 } else { 44 };
        let count = i64::try_from(l.count(stride)).unwrap_or(i64::MAX);
        let mut vertices: Vec<Option<[f32; 3]>> = Vec::new();
        for face in faces.iter().filter(|f| f.face_type == face_type::PATCH) {
            let first = i64::from(face.first_vertex);
            let end = first + i64::from(face.patch_size[0]) * i64::from(face.patch_size[1]);
            if first < 0 || end > count {
                return err("[BspLoader] Invalid patch control range");
            }
            for i in first..end {
                // 0 <= i < count here.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let i = i as usize;
                if vertices.len() <= i {
                    vertices.resize(i + 1, None);
                }
                vertices[i] = Some(l.f32s(i * stride));
            }
        }
        Ok(vertices)
    }

    fn parse_effects(&self) -> Result<Vec<BspEffect>> {
        let l = self.lump(lump::EFFECTS)?;
        Ok((0..l.count(72))
            .map(|i| {
                let at = i * 72;
                BspEffect {
                    name: name64(&l.0[at..]),
                    brush_index: l.i32(at + 64),
                    unknown: l.i32(at + 68),
                }
            })
            .collect())
    }

    fn parse_faces(&self) -> Result<Vec<BspFace>> {
        let l = self.lump(lump::FACES)?;
        if self.header.is_fbsp {
            return Ok((0..l.count(148))
                .map(|i| {
                    let at = i * 148;
                    let lightmap_indices: [i32; 4] = l.i32s(at + 36);
                    BspFace {
                        texture_index: l.i32(at),
                        effect_index: l.i32(at + 4),
                        face_type: l.i32(at + 8),
                        first_vertex: l.i32(at + 12),
                        num_vertices: l.i32(at + 16),
                        first_mesh_vert: l.i32(at + 20),
                        num_mesh_verts: l.i32(at + 24),
                        lightmap_index: lightmap_indices[0],
                        lightmap_indices: Some(lightmap_indices),
                        lightmap_start: [l.i32(at + 52), l.i32(at + 68)],
                        lightmap_size: [l.i32(at + 84), l.i32(at + 88)],
                        lightmap_origin: l.f32s(at + 92),
                        lightmap_vecs: [l.f32s(at + 104), l.f32s(at + 116)],
                        normal: l.f32s(at + 128),
                        patch_size: l.i32s(at + 140),
                    }
                })
                .collect());
        }
        Ok((0..l.count(104))
            .map(|i| {
                let at = i * 104;
                BspFace {
                    texture_index: l.i32(at),
                    effect_index: l.i32(at + 4),
                    face_type: l.i32(at + 8),
                    first_vertex: l.i32(at + 12),
                    num_vertices: l.i32(at + 16),
                    first_mesh_vert: l.i32(at + 20),
                    num_mesh_verts: l.i32(at + 24),
                    lightmap_index: l.i32(at + 28),
                    lightmap_indices: None,
                    lightmap_start: l.i32s(at + 32),
                    lightmap_size: l.i32s(at + 40),
                    lightmap_origin: l.f32s(at + 48),
                    lightmap_vecs: [l.f32s(at + 60), l.f32s(at + 72)],
                    normal: l.f32s(at + 84),
                    patch_size: l.i32s(at + 96),
                }
            })
            .collect())
    }

    fn parse_lightmaps(&self, size: u32) -> Result<Vec<Vec<u8>>> {
        let l = self.lump(lump::LIGHTMAPS)?;
        let page = u64::from(size) * u64::from(size) * 3;
        let length = l.len() as u64;
        if !length.is_multiple_of(page) {
            return err(format!(
                "[BspLoader] Lightmap lump ({length} bytes) does not contain complete {size}x{size} RGB pages"
            ));
        }
        // page divides a slice length here, so it fits in usize.
        #[allow(clippy::cast_possible_truncation)]
        let page = page as usize;
        Ok(l.0.chunks_exact(page).map(<[u8]>::to_vec).collect())
    }

    fn parse_light_vols(&self) -> Result<Vec<BspLightVol>> {
        let l = self.lump(lump::LIGHTVOLS)?;
        if !self.header.is_fbsp {
            return Ok((0..l.count(8))
                .map(|i| {
                    let at = i * 8;
                    BspLightVol {
                        valid: true,
                        ambient: l.u8s::<3>(at).map(u16::from),
                        directional: l.u8s::<3>(at + 3).map(u16::from),
                        direction: l.u8s(at + 6),
                    }
                })
                .collect());
        }
        if !l.len().is_multiple_of(30) {
            return err("[BspLoader] Invalid FBSP lightgrid record size");
        }
        let records: Vec<BspLightVol> =
            l.0.as_chunks::<30>()
                .0
                .iter()
                .map(|r| {
                    let mut ambient = [0u16; 3];
                    let mut directional = [0u16; 3];
                    for style in 0..4 {
                        if r[24 + style] == 255 {
                            continue;
                        }
                        for channel in 0..3 {
                            ambient[channel] += u16::from(r[style * 3 + channel]);
                            directional[channel] += u16::from(r[12 + style * 3 + channel]);
                        }
                    }
                    BspLightVol {
                        valid: true,
                        ambient,
                        directional,
                        direction: [r[28], r[29]],
                    }
                })
                .collect();
        let array = self.lump(lump::LIGHTARRAY)?;
        if !array.len().is_multiple_of(2) {
            return err("[BspLoader] Invalid FBSP lightarray size");
        }
        let invalid = BspLightVol {
            valid: false,
            ambient: [0; 3],
            directional: [0; 3],
            direction: [0; 2],
        };
        (0..array.len() / 2)
            .map(|i| match array.u16(i * 2) {
                0xffff => Ok(invalid),
                index => records.get(usize::from(index)).copied().map_or_else(
                    || {
                        err(format!(
                            "[BspLoader] FBSP lightarray index {index} exceeds {} records",
                            records.len()
                        ))
                    },
                    Ok,
                ),
            })
            .collect()
    }

    fn parse_vis_data(&self) -> Result<Option<BspVisData>> {
        let l = self.lump(lump::VISDATA)?;
        if l.len() == 0 {
            return Ok(None);
        }
        if l.len() < 8 {
            return err("Offset is outside the bounds of the DataView");
        }
        let num_clusters = l.i32(0);
        let bytes_per_cluster = l.i32(4);
        // The bit vectors are read from the file, not the lump: SHAG does not
        // check that they fit in it.
        let offset = i64::from(self.lumps[lump::VISDATA].offset) + 8;
        let length = i64::from(num_clusters) * i64::from(bytes_per_cluster);
        let bit_vectors = view(self.data, offset, length)?.to_vec();
        Ok(Some(BspVisData {
            num_clusters,
            bytes_per_cluster,
            bit_vectors,
        }))
    }
}

/// The match for `/"([^"]+)"\s+"([^"]*)"/` in `line`, if any.
fn match_key_value(line: &str) -> Option<(&str, &str)> {
    let quotes: Vec<usize> = line.match_indices('"').map(|(i, _)| i).collect();
    for (q, &open) in quotes.iter().enumerate() {
        // The key runs to the next quote and must not be empty.
        let &close = quotes.get(q + 1)?;
        if close == open + 1 {
            continue;
        }
        let after = &line[close + 1..];
        let gap = after.len() - after.trim_start_matches(is_js_whitespace).len();
        if gap == 0 {
            continue;
        }
        let value_open = close + 1 + gap;
        if line.as_bytes().get(value_open) != Some(&b'"') {
            continue;
        }
        let rest = &line[value_open + 1..];
        if let Some(end) = rest.find('"') {
            return Some((&line[open + 1..close], &rest[..end]));
        }
    }
    None
}

/// `parseEntityString`.
#[must_use]
pub fn parse_entity_string(text: &str) -> Vec<BspEntity> {
    let mut entities = Vec::new();
    let mut current: Option<BspEntity> = None;
    for line in text.split('\n') {
        let trimmed = js_trim(line);
        if trimmed == "{" {
            current = Some(BspEntity::default());
        } else if trimmed == "}" {
            if let Some(entity) = current.take() {
                entities.push(entity);
            }
        } else if let Some(entity) = current.as_mut()
            && let Some((key, value)) = match_key_value(trimmed)
        {
            entity.properties.set(key, value);
            if key == "classname" {
                value.clone_into(&mut entity.classname);
            }
        }
    }
    entities
}

fn worldspawn(entities: &[BspEntity]) -> Option<&BspEntity> {
    entities.iter().find(|e| e.classname == "worldspawn")
}

/// `resolveLightmapSize`.
fn resolve_lightmap_size(is_fbsp: bool, entities: &[BspEntity]) -> Result<u32> {
    if !is_fbsp {
        return Ok(IBSP_LIGHTMAP_SIZE);
    }
    let Some(text) = worldspawn(entities).and_then(|w| {
        w.properties
            .iter()
            .find(|(k, _)| k.to_lowercase() == "_lightmapimagesize")
            .map(|(_, v)| v)
    }) else {
        return Ok(FBSP_LIGHTMAP_SIZE);
    };
    let size = js_string_to_number(text);
    // Safe integer, positive, a power of two. `size * size * 3` is then a safe
    // integer exactly up to 2^25.
    if !(size.fract() == 0.0 && size > 0.0 && size <= 9_007_199_254_740_991.0) || size.log2().fract() != 0.0 {
        return err(format!("[BspLoader] Invalid _lightmapImageSize: {text}"));
    }
    if size > 33_554_432.0 {
        return err(format!("[BspLoader] _lightmapImageSize is too large: {text}"));
    }
    // A power of two up to 2^25.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(size as u32)
}

/// `Math.max(0, x)`, which keeps NaN.
fn js_max0(x: f64) -> f64 {
    if x.is_nan() { x } else { x.max(0.0) }
}

/// `calculateLightGrid`.
fn calculate_light_grid(entities: &[BspEntity], models: &[BspModel]) -> Option<LightGrid> {
    let world = models.first()?;
    let mut step = [64.0, 64.0, 128.0];
    if let Some(text) = worldspawn(entities)
        .and_then(|w| w.properties.get("gridsize"))
        .filter(|t| !t.is_empty())
    {
        let parts: Vec<f64> = split_whitespace_runs(text).into_iter().map(js_parse_float).collect();
        if parts.len() == 3 && parts.iter().all(|&n| n.is_finite() && n >= 1.0) {
            step = [parts[0], parts[1], parts[2]];
        }
    }
    let origin: [f64; 3] = std::array::from_fn(|k| (f64::from(world.mins[k]) / step[k]).ceil() * step[k]);
    let maxs: [f64; 3] = std::array::from_fn(|k| (f64::from(world.maxs[k]) / step[k]).floor() * step[k]);
    let size = std::array::from_fn(|k| js_max0(((maxs[k] - origin[k]) / step[k]).floor()) + 1.0);
    Some(LightGrid { origin, size, step })
}

/// `text.split(/\s+/)`.
fn split_whitespace_runs(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(is_js_whitespace) {
        parts.push(&rest[..start]);
        rest = rest[start..].trim_start_matches(is_js_whitespace);
    }
    parts.push(rest);
    parts
}

/// `isClusterVisible`: no PVS, a negative cluster or a byte outside the
/// vectors count as visible / not visible exactly as in the TypeScript.
#[must_use]
pub fn is_cluster_visible(vis: Option<&BspVisData>, a: i32, b: i32) -> bool {
    let Some(vis) = vis else { return true };
    if a < 0 || b < 0 {
        return true;
    }
    let byte = i64::from(a) * i64::from(vis.bytes_per_cluster) + i64::from(b / 8);
    usize::try_from(byte)
        .ok()
        .and_then(|i| vis.bit_vectors.get(i))
        .is_some_and(|v| v & (1 << (b % 8)) != 0)
}

/// `findLeafForPoint`. Returns `None` for an empty tree, and also where the
/// TypeScript would crash on a bad node or plane index.
#[must_use]
pub fn find_leaf_for_point<'b>(
    point: [f64; 3],
    nodes: &[BspNode],
    leafs: &'b [BspLeaf],
    planes: &[BspPlane],
) -> Option<&'b BspLeaf> {
    if nodes.is_empty() {
        return None;
    }
    let mut index: i32 = 0;
    while index >= 0 {
        let node = nodes.get(usize::try_from(index).ok()?)?;
        let plane = planes.get(usize::try_from(node.plane_index).ok()?)?;
        let n = plane.normal.map(f64::from);
        let dist = n[0] * point[0] + n[1] * point[1] + n[2] * point[2] - f64::from(plane.dist);
        index = if dist >= 0.0 {
            node.children[0]
        } else {
            node.children[1]
        };
    }
    leafs.get(usize::try_from(-(i64::from(index) + 1)).ok()?)
}

/// `getBspTextureNames`.
#[must_use]
pub fn bsp_texture_names(bsp: &BspCommon) -> Vec<&str> {
    bsp.textures.iter().map(|t| t.name.as_str()).collect()
}

/// `getBspUsedTextures`: textures faces use, in first-use order, without `common/`.
#[must_use]
pub fn bsp_used_textures(bsp: &BspCommon) -> Vec<&str> {
    let mut used: Vec<usize> = Vec::new();
    for face in &bsp.faces {
        if let Ok(i) = usize::try_from(face.texture_index)
            && i < bsp.textures.len()
            && !used.contains(&i)
        {
            used.push(i);
        }
    }
    used.into_iter()
        .map(|i| bsp.textures[i].name.as_str())
        .filter(|name| !name.to_lowercase().contains("common/"))
        .collect()
}
