//! SHAG `src/engine/loader/shader/Q3ShaderTypes.ts`: what a parsed shader
//! script holds.
//!
//! The types serialize (serde) to the TypeScript objects' JSON: camelCase
//! keys, absent optional fields left out, string unions as their strings.
//! The golden test compares that JSON with `JSON.stringify` of SHAG's own.
//! The resolved material types follow with the registry.

use serde::Serialize;

/// `BlendFactor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum BlendFactor {
    #[serde(rename = "GL_ONE")]
    One,
    #[serde(rename = "GL_ZERO")]
    Zero,
    #[serde(rename = "GL_SRC_COLOR")]
    SrcColor,
    #[serde(rename = "GL_DST_COLOR")]
    DstColor,
    #[serde(rename = "GL_ONE_MINUS_SRC_COLOR")]
    OneMinusSrcColor,
    #[serde(rename = "GL_ONE_MINUS_DST_COLOR")]
    OneMinusDstColor,
    #[serde(rename = "GL_SRC_ALPHA")]
    SrcAlpha,
    #[serde(rename = "GL_DST_ALPHA")]
    DstAlpha,
    #[serde(rename = "GL_ONE_MINUS_SRC_ALPHA")]
    OneMinusSrcAlpha,
    #[serde(rename = "GL_ONE_MINUS_DST_ALPHA")]
    OneMinusDstAlpha,
}

impl BlendFactor {
    /// The factor a `GL_*` name (any case) names.
    #[must_use]
    pub fn from_gl_name(name: &str) -> Option<Self> {
        Some(match name.to_uppercase().as_str() {
            "GL_ONE" => Self::One,
            "GL_ZERO" => Self::Zero,
            "GL_SRC_COLOR" => Self::SrcColor,
            "GL_DST_COLOR" => Self::DstColor,
            "GL_ONE_MINUS_SRC_COLOR" => Self::OneMinusSrcColor,
            "GL_ONE_MINUS_DST_COLOR" => Self::OneMinusDstColor,
            "GL_SRC_ALPHA" => Self::SrcAlpha,
            "GL_DST_ALPHA" => Self::DstAlpha,
            "GL_ONE_MINUS_SRC_ALPHA" => Self::OneMinusSrcAlpha,
            "GL_ONE_MINUS_DST_ALPHA" => Self::OneMinusDstAlpha,
            _ => return None,
        })
    }
}

/// `AlphaFunc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum AlphaFunc {
    #[serde(rename = "GT0")]
    Gt0,
    #[serde(rename = "GE128")]
    Ge128,
    #[serde(rename = "LT128")]
    Lt128,
}

/// `CullMode`. The parser never gives `disable` (it reads it as `none`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CullMode {
    Front,
    #[default]
    Back,
    None,
    Disable,
}

/// `WaveFunc["func"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WaveForm {
    Sin,
    Triangle,
    Square,
    Sawtooth,
    #[serde(rename = "inversesawtooth")]
    InverseSawtooth,
    Noise,
}

/// `WaveFunc`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WaveFunc {
    pub func: WaveForm,
    pub base: f64,
    pub amplitude: f64,
    pub phase: f64,
    pub frequency: f64,
}

/// `TcMod`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TcMod {
    Scroll {
        s_speed: f64,
        t_speed: f64,
    },
    Scale {
        s_scale: f64,
        t_scale: f64,
    },
    Rotate {
        degrees_per_second: f64,
    },
    Stretch {
        func: WaveFunc,
    },
    Turb {
        base: f64,
        amplitude: f64,
        phase: f64,
        frequency: f64,
    },
    Transform {
        m00: f64,
        m01: f64,
        m10: f64,
        m11: f64,
        t0: f64,
        t1: f64,
    },
}

/// `TcGenMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TcGenMode {
    Base,
    Lightmap,
    Environment,
    Surround,
    Vector,
}

/// `TcGenVector`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TcGenVector {
    pub s_vector: [f64; 3],
    pub t_vector: [f64; 3],
}

/// `TcGen`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TcGen {
    pub mode: TcGenMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<TcGenVector>,
}

impl TcGen {
    #[must_use]
    pub fn mode(mode: TcGenMode) -> Self {
        Self { mode, vector: None }
    }
}

/// `DistanceRamp`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DistanceRamp {
    pub from: f64,
    pub to: f64,
    pub near: f64,
    pub far: f64,
}

/// `RgbGen["type"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RgbGenType {
    Identity,
    IdentityLighting,
    Wave,
    DistanceRamp,
    Entity,
    OneMinusEntity,
    Vertex,
    OneMinusVertex,
    LightingDiffuse,
    Const,
}

/// `RgbGen`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RgbGen {
    #[serde(rename = "type")]
    pub kind: RgbGenType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wave: Option<WaveFunc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_ramp: Option<DistanceRamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 3]>,
}

impl RgbGen {
    #[must_use]
    pub fn of(kind: RgbGenType) -> Self {
        Self {
            kind,
            wave: None,
            distance_ramp: None,
            color: None,
        }
    }
}

/// `AlphaGen["type"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AlphaGenType {
    Identity,
    Wave,
    DistanceRamp,
    Entity,
    OneMinusEntity,
    Vertex,
    OneMinusVertex,
    LightingSpecular,
    Portal,
    Const,
}

/// `AlphaGen`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlphaGen {
    #[serde(rename = "type")]
    pub kind: AlphaGenType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wave: Option<WaveFunc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_ramp: Option<DistanceRamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

impl AlphaGen {
    #[must_use]
    pub fn of(kind: AlphaGenType) -> Self {
        Self {
            kind,
            wave: None,
            distance_ramp: None,
            value: None,
        }
    }
}

/// `Q3MaterialDirective`: the Warsow `material` tokens, raw and in the common
/// four slots (diffuse, normal, gloss, glow).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Q3MaterialDirective {
    pub raw_tokens: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffuse: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gloss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<Vec<String>>,
}

/// `Q3ShaderConditionalBlock["condition"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Condition {
    #[serde(rename = "deluxe")]
    Deluxe,
    #[serde(rename = "!deluxe")]
    NotDeluxe,
    #[serde(rename = "textureCubeMap")]
    TextureCubeMap,
    #[serde(rename = "!textureCubeMap")]
    NotTextureCubeMap,
    #[serde(rename = "unsupported")]
    Unsupported,
}

/// `Q3ShaderConditionalBlock`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Q3ShaderConditionalBlock {
    pub condition: Condition,
    pub stages: Vec<Q3ShaderStage>,
}

/// `animMap`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimMap {
    pub frequency: f64,
    pub frames: Vec<String>,
}

/// `depthFunc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DepthFunc {
    Lequal,
    Equal,
}

/// `Q3ShaderStage`: one pass.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Q3ShaderStage {
    /// The texture, or `$lightmap`, `$whiteimage` and the like.
    pub map: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cube_map: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<Q3MaterialDirective>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clamp_map: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anim_map: Option<AnimMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend_func: Option<[BlendFactor; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_func: Option<AlphaFunc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rgb_gen: Option<RgbGen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_gen: Option<AlphaGen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tc_mod: Option<Vec<TcMod>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tc_gen: Option<TcGen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_write: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_func: Option<DepthFunc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<bool>,
}

/// `DeformVertexes["type"]`. The parser keeps only the autosprites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeformType {
    Wave,
    Normal,
    Bulge,
    Move,
    Autosprite,
    Autosprite2,
    Projectionshadow,
    Text,
}

/// `DeformVertexes`, as the parser fills it (type only).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DeformVertexes {
    #[serde(rename = "type")]
    pub kind: DeformType,
}

/// `DEFAULT_SKY_HEIGHT`: Warfork's sky height when a shader gives none.
pub const DEFAULT_SKY_HEIGHT: f64 = 2048.0;

/// `SkyParms`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkyParms {
    pub farbox: String,
    /// The height the shader gives, 0 when it gives none.
    pub cloud_height: f64,
    pub nearbox: String,
    /// The shader's height, else [`DEFAULT_SKY_HEIGHT`].
    pub sky_height: f64,
    /// `skyparmssides`: each side's image, in Quake order rt bk lf ft up dn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sides: Option<Vec<String>>,
}

/// `fogParms`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FogParms {
    pub color: [f64; 3],
    pub depth_for_opaque: f64,
}

/// `sort`: a number, or a preset name when it does not parse.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Sort {
    Number(f64),
    Name(String),
}

/// `Q3Shader`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Q3Shader {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_particle: Option<bool>,
    /// Lower case.
    pub name: String,
    /// A set in insertion order, lower case.
    pub surface_parms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_light: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gloss_intensity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gloss_exponent: Option<f64>,
    pub cull: CullMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<Sort>,
    /// Every stage, conditional ones included, in source order.
    pub stages: Vec<Q3ShaderStage>,
    /// The stages outside conditional blocks.
    pub unconditional_stages: Vec<Q3ShaderStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_blocks: Option<Vec<Q3ShaderConditionalBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deform_vertexes: Option<Vec<DeformVertexes>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_parms: Option<SkyParms>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_parms: Option<FogParms>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nomipmaps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nopicmip: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_fog: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygon_offset: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_mergable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_image: Option<String>,
}

impl Q3Shader {
    /// The defaults `parseShaderBody` starts from.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            soft_particle: None,
            name: name.to_lowercase(),
            surface_parms: Vec::new(),
            surface_light: None,
            gloss_intensity: None,
            gloss_exponent: None,
            cull: CullMode::Back,
            sort: None,
            stages: Vec::new(),
            unconditional_stages: Vec::new(),
            conditional_blocks: None,
            deform_vertexes: None,
            sky_parms: None,
            fog_parms: None,
            portal: None,
            nomipmaps: None,
            nopicmip: None,
            no_fog: None,
            polygon_offset: None,
            entity_mergable: None,
            editor_image: None,
        }
    }

    /// `surfaceParms.has(parm)`.
    #[must_use]
    pub fn has_surface_parm(&self, parm: &str) -> bool {
        self.surface_parms.iter().any(|p| p == parm)
    }
}
