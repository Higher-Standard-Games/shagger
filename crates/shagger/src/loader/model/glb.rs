//! Reading a binary glTF (`.glb`): the JSON chunk, the BIN chunk, and typed
//! accessors over it.
//!
//! RUST.md 20.10 names the `gltf` crate for this. Washoi's models need
//! `EXT_texture_webp`, whose textures carry no core `source`, and the crate
//! rejects such a texture as invalid; so the few parts of the format the
//! models use are read here with serde, as Babylon's glTF loader reads them
//! (`glTFLoader.ts` `_loadFloatAccessorAsync` and friends).

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlbError(pub String);

impl std::fmt::Display for GlbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GlbError {}

fn err(message: impl Into<String>) -> GlbError {
    GlbError(message.into())
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub children: Vec<usize>,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
    pub matrix: Option<[f32; 16]>,
    pub translation: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Primitive {
    pub attributes: std::collections::BTreeMap<String, usize>,
    pub indices: Option<usize>,
    pub material: Option<usize>,
    pub mode: Option<u32>,
    #[serde(default)]
    pub extensions: Option<Value>,
}

/// A primitive's attributes as `KHR_draco_mesh_compression` decodes them:
/// floats per attribute name, with their component count, and the indices.
#[derive(Debug, Clone, Default)]
pub struct DracoPrimitive {
    pub attributes: std::collections::BTreeMap<String, (usize, Vec<f32>)>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Mesh {
    #[serde(default)]
    pub name: Option<String>,
    pub primitives: Vec<Primitive>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accessor {
    pub buffer_view: Option<usize>,
    #[serde(default)]
    pub byte_offset: usize,
    pub component_type: u32,
    #[serde(default)]
    pub normalized: bool,
    pub count: usize,
    #[serde(rename = "type")]
    pub kind: String,
    pub sparse: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferView {
    pub buffer: usize,
    #[serde(default)]
    pub byte_offset: usize,
    pub byte_length: usize,
    pub byte_stride: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureInfo {
    pub index: usize,
    #[serde(default)]
    pub tex_coord: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PbrMetallicRoughness {
    pub base_color_factor: Option<[f32; 4]>,
    pub base_color_texture: Option<TextureInfo>,
    pub metallic_factor: Option<f32>,
    pub roughness_factor: Option<f32>,
    pub metallic_roughness_texture: Option<TextureInfo>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pbr_metallic_roughness: Option<PbrMetallicRoughness>,
    pub normal_texture: Option<TextureInfo>,
    pub emissive_texture: Option<TextureInfo>,
    pub emissive_factor: Option<[f32; 3]>,
    pub alpha_mode: Option<String>,
    pub alpha_cutoff: Option<f32>,
    #[serde(default)]
    pub double_sided: bool,
    #[serde(default)]
    pub extensions: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Texture {
    pub sampler: Option<usize>,
    pub source: Option<usize>,
    #[serde(default)]
    pub extensions: Option<Value>,
}

impl Texture {
    /// The image a texture shows: `EXT_texture_webp`'s source first, as
    /// Babylon's `EXT_texture_webp` extension loads it, then the core one.
    #[must_use]
    pub fn image(&self) -> Option<usize> {
        self.extensions
            .as_ref()
            .and_then(|e| e.get("EXT_texture_webp"))
            .and_then(|e| e.get("source"))
            .and_then(Value::as_u64)
            .map(|s| s as usize)
            .or(self.source)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    #[serde(default)]
    pub name: Option<String>,
    pub buffer_view: Option<usize>,
    pub mime_type: Option<String>,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sampler {
    pub mag_filter: Option<u32>,
    pub min_filter: Option<u32>,
    pub wrap_s: Option<u32>,
    pub wrap_t: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skin {
    pub inverse_bind_matrices: Option<usize>,
    pub joints: Vec<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelTarget {
    pub node: Option<usize>,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Channel {
    pub sampler: usize,
    pub target: ChannelTarget,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnimationSampler {
    pub input: usize,
    pub output: usize,
    pub interpolation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Animation {
    #[serde(default)]
    pub name: Option<String>,
    pub channels: Vec<Channel>,
    pub samplers: Vec<AnimationSampler>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub nodes: Vec<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub meshes: Vec<Mesh>,
    #[serde(default)]
    pub accessors: Vec<Accessor>,
    #[serde(default)]
    pub buffer_views: Vec<BufferView>,
    #[serde(default)]
    pub materials: Vec<Material>,
    #[serde(default)]
    pub textures: Vec<Texture>,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default)]
    pub samplers: Vec<Sampler>,
    #[serde(default)]
    pub skins: Vec<Skin>,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    pub scene: Option<usize>,
}

/// A parsed `.glb`: its JSON and its binary chunk.
#[derive(Debug, Clone)]
pub struct Glb {
    pub document: Document,
    pub bin: Vec<u8>,
}

const GLB_MAGIC: u32 = 0x4654_6c67; // "glTF"
const CHUNK_JSON: u32 = 0x4e4f_534a;
const CHUNK_BIN: u32 = 0x004e_4942;

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, GlbError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| err("truncated glb"))
}

impl Glb {
    /// Parses glb version 2: a 12-byte header, then chunks.
    pub fn parse(bytes: &[u8]) -> Result<Self, GlbError> {
        if u32_at(bytes, 0)? != GLB_MAGIC {
            return Err(err("not a glb"));
        }
        if u32_at(bytes, 4)? != 2 {
            return Err(err("glb version is not 2"));
        }
        let length = (u32_at(bytes, 8)? as usize).min(bytes.len());
        let mut at = 12;
        let mut json = None;
        let mut bin = Vec::new();
        while at + 8 <= length {
            let chunk_length = u32_at(bytes, at)? as usize;
            let kind = u32_at(bytes, at + 4)?;
            let data = bytes
                .get(at + 8..at + 8 + chunk_length)
                .ok_or_else(|| err("truncated glb chunk"))?;
            match kind {
                CHUNK_JSON => json = Some(data),
                CHUNK_BIN if bin.is_empty() => bin = data.to_vec(),
                _ => {}
            }
            at += 8 + chunk_length;
        }
        let json = json.ok_or_else(|| err("glb has no JSON chunk"))?;
        let document: Document = serde_json::from_slice(json).map_err(|e| err(format!("glb JSON: {e}")))?;
        Ok(Self { document, bin })
    }

    /// The bytes of a buffer view (only the GLB's own buffer 0 is read).
    pub fn view_bytes(&self, view: usize) -> Result<&[u8], GlbError> {
        let view = self
            .document
            .buffer_views
            .get(view)
            .ok_or_else(|| err("buffer view out of range"))?;
        if view.buffer != 0 {
            return Err(err("external buffers are not read"));
        }
        self.bin
            .get(view.byte_offset..view.byte_offset.saturating_add(view.byte_length))
            .ok_or_else(|| err("buffer view past the BIN chunk"))
    }

    /// An accessor's elements as floats, `components` per element, with
    /// normalized integers mapped to 0..1 (or -1..1) as glTF defines.
    pub fn read_floats(&self, accessor: usize) -> Result<(usize, Vec<f32>), GlbError> {
        let a = self
            .document
            .accessors
            .get(accessor)
            .ok_or_else(|| err("accessor out of range"))?;
        let components = components_of(a)?;
        let mut out = Vec::with_capacity(a.count * components);
        self.for_each_component(a, components, |b| {
            out.push(match a.component_type {
                5126 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                5121 => {
                    let v = f32::from(b[0]);
                    if a.normalized { v / 255.0 } else { v }
                }
                5120 => {
                    let v = f32::from(b[0] as i8);
                    if a.normalized { (v / 127.0).max(-1.0) } else { v }
                }
                5123 => {
                    let v = f32::from(u16::from_le_bytes([b[0], b[1]]));
                    if a.normalized { v / 65535.0 } else { v }
                }
                5122 => {
                    let v = f32::from(i16::from_le_bytes([b[0], b[1]]));
                    if a.normalized { (v / 32767.0).max(-1.0) } else { v }
                }
                _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
            });
        })?;
        Ok((components, out))
    }

    /// An index or joint accessor as unsigned integers.
    pub fn read_uints(&self, accessor: usize) -> Result<Vec<u32>, GlbError> {
        let a = self
            .document
            .accessors
            .get(accessor)
            .ok_or_else(|| err("accessor out of range"))?;
        let components = components_of(a)?;
        let mut out = Vec::with_capacity(a.count * components);
        self.for_each_component(a, components, |b| {
            out.push(match a.component_type {
                5121 | 5120 => u32::from(b[0]),
                5123 | 5122 => u32::from(u16::from_le_bytes([b[0], b[1]])),
                _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            });
        })?;
        Ok(out)
    }

    fn for_each_component(&self, a: &Accessor, components: usize, mut each: impl FnMut(&[u8])) -> Result<(), GlbError> {
        if a.sparse.is_some() {
            return Err(err("sparse accessors are not read"));
        }
        let size = match a.component_type {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            other => return Err(err(format!("component type {other}"))),
        };
        let Some(view_index) = a.buffer_view else {
            // All zeros; a count no model has (fuzzing: two billion) is
            // refused rather than filled.
            let total = a
                .count
                .checked_mul(components)
                .filter(|&n| n <= 1 << 24)
                .ok_or_else(|| err("accessor too large"))?;
            let zero = [0u8; 4];
            for _ in 0..total {
                each(&zero);
            }
            return Ok(());
        };
        let view = self
            .document
            .buffer_views
            .get(view_index)
            .ok_or_else(|| err("accessor names a missing buffer view"))?;
        let bytes = self.view_bytes(view_index)?;
        let stride = view.byte_stride.unwrap_or(components * size);
        let past = || err("accessor past its view");
        for i in 0..a.count {
            for c in 0..components {
                let at = i
                    .checked_mul(stride)
                    .and_then(|o| o.checked_add(a.byte_offset + c * size))
                    .ok_or_else(past)?;
                each(bytes.get(at..at + size).ok_or_else(past)?);
            }
        }
        Ok(())
    }
}

impl Glb {
    /// Decodes a primitive's `KHR_draco_mesh_compression` data, as Babylon's
    /// `KHR_draco_mesh_compression` extension does with Draco's decoder: the
    /// extension names each glTF attribute by its Draco unique id. `None`
    /// when the primitive is not compressed.
    pub fn decode_draco(&self, primitive: &Primitive) -> Result<Option<DracoPrimitive>, GlbError> {
        let Some(ext) = primitive
            .extensions
            .as_ref()
            .and_then(|e| e.get("KHR_draco_mesh_compression"))
        else {
            return Ok(None);
        };
        let view = ext
            .get("bufferView")
            .and_then(Value::as_u64)
            .ok_or_else(|| err("draco primitive has no buffer view"))?;
        let bytes = self.view_bytes(view as usize)?;
        let mut buffer = draco_core::decoder_buffer::DecoderBuffer::new(bytes);
        let mut mesh = draco_core::mesh::Mesh::new();
        draco_core::mesh_decoder::MeshDecoder::new()
            .decode(&mut buffer, &mut mesh)
            .map_err(|e| err(format!("draco: {e:?}")))?;
        let points = mesh.num_points();
        let mut out = DracoPrimitive::default();
        if let Some(attributes) = ext.get("attributes").and_then(Value::as_object) {
            for (name, id) in attributes {
                let id = id.as_u64().ok_or_else(|| err("draco attribute id"))? as u32;
                let attribute = mesh
                    .attribute_by_unique_id(id)
                    .ok_or_else(|| err(format!("draco attribute {name} missing")))?;
                let components = attribute.num_components() as usize;
                let mut values = attribute.read_f32s(points, components);
                // Draco keeps a normalized integer attribute (weights as
                // bytes) as its integers; the glTF accessor says how they
                // map to floats, as `read_floats` maps them.
                if let Some(a) = primitive
                    .attributes
                    .get(name)
                    .and_then(|&a| self.document.accessors.get(a))
                    && a.normalized
                {
                    let scale = match a.component_type {
                        5121 => Some((255.0, 0.0)),
                        5120 => Some((127.0, -1.0)),
                        5123 => Some((65535.0, 0.0)),
                        5122 => Some((32767.0, -1.0)),
                        _ => None,
                    };
                    if let Some((max, floor)) = scale {
                        for v in &mut values {
                            *v = (*v / max).max(floor);
                        }
                    }
                }
                out.attributes.insert(name.clone(), (components, values));
            }
        }
        out.indices = (0..mesh.num_faces())
            .flat_map(|f| {
                let face = mesh.face(draco_core::geometry_indices::FaceIndex(f as u32));
                [face[0].0, face[1].0, face[2].0]
            })
            .collect();
        Ok(Some(out))
    }
}

fn components_of(a: &Accessor) -> Result<usize, GlbError> {
    Ok(match a.kind.as_str() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        other => return Err(err(format!("accessor type {other}"))),
    })
}
