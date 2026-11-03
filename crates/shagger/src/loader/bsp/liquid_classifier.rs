//! Port of SHAG `src/engine/loader/bsp/LiquidClassifier.ts`: which BSP
//! shaders are liquid surfaces, and of which kind. Compiled content flags
//! first, the shader's own `surfaceparm` second, an exact-name override above
//! both. A surface is never liquid because its name contains "water".

use std::collections::HashMap;

use super::bsp_loader::BspFile;
use crate::physics::content_flags::{is_liquid, liquid_kind_of, surface};

/// `LiquidKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiquidKind {
    Water,
    Slime,
    Lava,
}

impl LiquidKind {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "water" => Some(Self::Water),
            "slime" => Some(Self::Slime),
            "lava" => Some(Self::Lava),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Water => "water",
            Self::Slime => "slime",
            Self::Lava => "lava",
        }
    }
}

/// `LiquidSignal`: what said a shader is liquid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidSignal {
    Override,
    ContentFlag,
    SurfaceParm,
}

/// `LiquidSurfaceDescriptor`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiquidSurfaceDescriptor {
    pub shader_name: String,
    pub texture_index: usize,
    pub kind: LiquidKind,
    pub raw_contents: i32,
    pub surface_flags: i32,
    pub signals: Vec<LiquidSignal>,
    pub profile: Option<String>,
    pub environment_map: Option<String>,
    /// False for a NODRAW liquid: a volume to swim in with no surface.
    pub drawn: bool,
    pub volume_brushes: usize,
    pub drawn_faces: usize,
}

/// `LiquidOverride`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiquidOverride {
    pub include: bool,
    pub exclude: bool,
    pub kind: Option<LiquidKind>,
    pub profile: Option<String>,
}

/// `liquidKindFromSurfaceParms`: lava, then slime, then water.
#[must_use]
pub fn liquid_kind_from_surface_parms<'a>(surface_parms: impl IntoIterator<Item = &'a str>) -> Option<LiquidKind> {
    let parms: Vec<String> = surface_parms.into_iter().map(str::to_lowercase).collect();
    ["lava", "slime", "water"]
        .into_iter()
        .find(|k| parms.iter().any(|p| p == k))
        .and_then(LiquidKind::from_name)
}

/// `normalizeShaderName`: the printable start, its first token, forward
/// slashes, lower case.
#[must_use]
pub fn normalize_shader_name(raw: &str) -> String {
    let printable = raw
        .char_indices()
        .find(|(_, c)| !(' '..='~').contains(c))
        .map_or(raw, |(i, _)| &raw[..i]);
    printable
        .trim()
        .split(char::is_whitespace)
        .next()
        .unwrap_or("")
        .replace('\\', "/")
        .to_lowercase()
}

fn kind_from_override(o: &LiquidOverride, fallback: Option<LiquidKind>) -> Option<LiquidKind> {
    if o.exclude {
        return None;
    }
    o.kind.or(fallback).or(o.include.then_some(LiquidKind::Water))
}

/// `classifyLiquids`.
#[must_use]
pub fn classify_liquids(
    bsp: &BspFile,
    overrides: &HashMap<String, LiquidOverride>,
    surface_parm: impl Fn(&str) -> Option<LiquidKind>,
    environment_map: impl Fn(&str) -> Option<String>,
) -> Vec<LiquidSurfaceDescriptor> {
    let mut volume_brushes: HashMap<i32, usize> = HashMap::new();
    for brush in &bsp.brushes {
        *volume_brushes.entry(brush.shader_index).or_default() += 1;
    }
    let mut drawn_faces: HashMap<i32, usize> = HashMap::new();
    for face in &bsp.faces {
        *drawn_faces.entry(face.texture_index).or_default() += 1;
    }
    let mut out = Vec::new();
    for (texture_index, texture) in bsp.textures.iter().enumerate() {
        let shader_name = normalize_shader_name(&texture.name);
        let o = overrides.get(&shader_name);
        if o.is_some_and(|o| o.exclude) {
            continue;
        }
        let mut signals = Vec::new();
        let mut kind = None;
        if let Some(o) = o.filter(|o| o.include || o.kind.is_some()) {
            signals.push(LiquidSignal::Override);
            kind = kind_from_override(o, None);
        }
        if is_liquid(texture.content_flags) {
            signals.push(LiquidSignal::ContentFlag);
            kind = kind.or_else(|| liquid_kind_of(texture.content_flags).and_then(LiquidKind::from_name));
        }
        if let Some(declared) = surface_parm(&shader_name) {
            signals.push(LiquidSignal::SurfaceParm);
            kind = kind.or(Some(declared));
        }
        let Some(kind) = kind.filter(|_| !signals.is_empty()) else {
            continue;
        };
        let index = texture_index as i32;
        out.push(LiquidSurfaceDescriptor {
            environment_map: environment_map(&shader_name),
            shader_name,
            texture_index,
            kind,
            raw_contents: texture.content_flags,
            surface_flags: texture.surface_flags,
            signals,
            profile: o.and_then(|o| o.profile.clone()),
            drawn: texture.surface_flags & surface::NODRAW == 0,
            volume_brushes: volume_brushes.get(&index).copied().unwrap_or(0),
            drawn_faces: drawn_faces.get(&index).copied().unwrap_or(0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_parms() {
        assert_eq!(
            normalize_shader_name("Textures\\World\\Water extra\u{1}junk"),
            "textures/world/water"
        );
        assert_eq!(
            liquid_kind_from_surface_parms(["nolightmap", "WATER", "slime"]),
            Some(LiquidKind::Slime)
        );
        assert_eq!(liquid_kind_from_surface_parms(["trans"]), None);
        let o = LiquidOverride {
            include: true,
            ..LiquidOverride::default()
        };
        assert_eq!(kind_from_override(&o, None), Some(LiquidKind::Water));
    }
}
