//! How a resolved shader material draws: its base image candidates (SHAG
//! `src/engine/loader/bsp/BspMeshBuilder.ts`, `buildDiffuseCandidates` in
//! `loadTextures`) and its blend, alpha test and depth state
//! (`src/engine/loader/shader/Q3Blend.ts`, `configureQ3Alpha` and
//! `q3AlphaMode`, with `createMaterialFromProps`).

use crate::loader::shader::q3_shader_types::{AlphaMode, BlendFactor, ResolvedMaterialProps};

/// `normalizeTexturePath`.
fn normalize_texture_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// `isSpecialTextureToken`: `$lightmap`, `$whiteimage` and the like.
fn is_special_texture_token(path: &str) -> bool {
    path.starts_with('$')
}

/// `addTexturePrefixes` (the builder's own, which drops special tokens).
fn add_texture_prefixes(path: &str) -> Vec<String> {
    let normalized = normalize_texture_path(path);
    if is_special_texture_token(&normalized) {
        return Vec::new();
    }
    let lower = normalized.to_lowercase();
    let mut paths = vec![normalized.clone()];
    if !lower.starts_with("textures/") && !lower.starts_with("models/") {
        paths.push(format!("textures/{normalized}"));
    }
    paths
}

/// `dedupePaths`.
fn dedupe_paths(paths: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .map(|p| normalize_texture_path(&p))
        .filter(|p| !p.is_empty() && seen.insert(p.to_lowercase()))
        .collect()
}

/// `buildTextureNameFallbacks`.
fn build_texture_name_fallbacks(texture_name: &str) -> Vec<String> {
    let mut paths = vec![texture_name.to_owned()];
    if texture_name.contains('_') && !texture_name.contains('/') {
        let parts: Vec<&str> = texture_name.split('_').collect();
        if parts.len() >= 2 {
            paths.push(parts.join("/"));
            if parts.len() >= 3 {
                paths.push(format!("{}/{}/{}", parts[0], parts[1], parts[2..].join("_")));
            }
            if parts.len() >= 4 {
                paths.push(format!(
                    "{}/{}/{}/{}",
                    parts[0],
                    parts[1],
                    parts[2],
                    parts[3..].join("_")
                ));
            }
        }
    }
    paths
}

/// `buildDiffuseCandidates`: the image paths (without extension rules,
/// which the texture loader applies) a material's base image is looked for
/// at, first found wins.
#[must_use]
pub fn build_diffuse_candidates(texture_name: &str, props: &ResolvedMaterialProps) -> Vec<String> {
    let mut candidates = Vec::new();
    let diffuse = props.diffuse_texture.as_deref().filter(|d| !d.is_empty());
    if props.has_material_metadata {
        match diffuse {
            Some(d) if !is_special_texture_token(d) => candidates.extend(add_texture_prefixes(d)),
            Some(_) => {}
            None => {
                for fallback in build_texture_name_fallbacks(texture_name) {
                    candidates.extend(add_texture_prefixes(&fallback));
                }
            }
        }
        return dedupe_paths(candidates);
    }
    if let Some(d) = diffuse
        && !is_special_texture_token(d)
    {
        candidates.extend(add_texture_prefixes(d));
    }
    for fallback in build_texture_name_fallbacks(texture_name) {
        candidates.extend(add_texture_prefixes(&fallback));
    }
    dedupe_paths(candidates)
}

/// What draws when no candidate has an image (the fallbacks after
/// `loadFirstTexture` in `loadTextures`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingImage {
    /// An additive surface: a radial gradient, for halos.
    RadialGradient,
    White,
    Black,
    /// No image: the material's plain (white) diffuse colour.
    Untextured,
}

/// The fallback for a material whose candidates all miss.
#[must_use]
pub fn missing_image(props: &ResolvedMaterialProps) -> MissingImage {
    let diffuse = props.diffuse_texture.as_deref().unwrap_or("");
    if props.alpha_mode == AlphaMode::Add {
        MissingImage::RadialGradient
    } else if diffuse.contains("$whiteimage") {
        MissingImage::White
    } else if diffuse.contains("$blackimage") {
        MissingImage::Black
    } else {
        MissingImage::Untextured
    }
}

/// `q3AlphaMode`: Babylon's blend equations for the Q3 modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blend {
    /// `ALPHA_DISABLE`.
    Opaque,
    /// `ALPHA_COMBINE`: src·a + dst·(1−a).
    Combine,
    /// `ALPHA_ONEONE`: src + dst (Q3 add ignores texture alpha).
    OneOne,
    /// `ALPHA_ADD`: src·a + dst, for `GL_SRC_ALPHA GL_ONE`.
    Add,
    /// `ALPHA_MULTIPLY`: src·dst.
    Multiply,
}

/// `q3AlphaMode`.
#[must_use]
pub fn q3_alpha_mode(props: &ResolvedMaterialProps) -> Blend {
    match props.alpha_mode {
        AlphaMode::Add => {
            if props.blend_func.is_some_and(|b| b[0] == BlendFactor::SrcAlpha) {
                Blend::Add
            } else {
                Blend::OneOne
            }
        }
        AlphaMode::Multiply => Blend::Multiply,
        AlphaMode::Blend => Blend::Combine,
        AlphaMode::Opaque => Blend::Opaque,
    }
}

impl Blend {
    /// The wgpu blend state Babylon sets for the mode.
    #[must_use]
    pub fn state(self) -> Option<wgpu::BlendState> {
        use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op, BlendState};
        let component = |src, dst| BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: Op::Add,
        };
        match self {
            Self::Opaque => None,
            Self::Combine => Some(BlendState {
                color: component(F::SrcAlpha, F::OneMinusSrcAlpha),
                alpha: component(F::One, F::OneMinusSrcAlpha),
            }),
            Self::OneOne => Some(BlendState {
                color: component(F::One, F::One),
                alpha: component(F::Zero, F::One),
            }),
            Self::Add => Some(BlendState {
                color: component(F::SrcAlpha, F::One),
                alpha: component(F::Zero, F::One),
            }),
            Self::Multiply => Some(BlendState {
                color: component(F::Dst, F::Zero),
                alpha: component(F::One, F::Zero),
            }),
        }
    }
}

/// The draw state `configureQ3Alpha` and `createMaterialFromProps` give a
/// pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassState {
    pub blend: Blend,
    /// Discard below this alpha (`MATERIAL_ALPHATEST`).
    pub alpha_cutoff: Option<f32>,
    /// `useAlphaFromDiffuseTexture`.
    pub texture_alpha: bool,
    pub depth_write: bool,
    /// `zOffset = -2` for decals.
    pub polygon_offset: bool,
    /// Additive passes draw their image unlit (white emissive, black
    /// diffuse).
    pub unlit: bool,
}

impl PassState {
    #[must_use]
    pub fn of(props: &ResolvedMaterialProps) -> Self {
        let blend = q3_alpha_mode(props);
        let test = props.alpha_cutoff.is_some();
        // Babylon writes depth for the opaque and alpha-test queues, and for
        // blended ones only when the stage asks (`forceDepthWrite`).
        let depth_write = props.depth_write
            && (blend == Blend::Opaque || props.primary_stage.as_ref().and_then(|s| s.depth_write) == Some(true));
        Self {
            blend,
            alpha_cutoff: props.alpha_cutoff.map(|c| c as f32),
            texture_alpha: test || blend != Blend::Opaque,
            depth_write,
            polygon_offset: props.polygon_offset,
            unlit: props.alpha_mode == AlphaMode::Add,
        }
    }

    /// Drawn after the opaque world (Babylon's transparent queue).
    #[must_use]
    pub fn is_blended(&self) -> bool {
        self.blend != Blend::Opaque
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_the_builder() {
        let props = ResolvedMaterialProps {
            diffuse_texture: Some("base\\wall.tga".into()),
            ..ResolvedMaterialProps::default()
        };
        assert_eq!(
            build_diffuse_candidates("a_b_c", &props),
            [
                "base/wall.tga",
                "textures/base/wall.tga",
                "a_b_c",
                "textures/a_b_c",
                "a/b/c",
                "textures/a/b/c"
            ]
        );
        let material = ResolvedMaterialProps {
            has_material_metadata: true,
            diffuse_texture: Some("$whiteimage".into()),
            ..ResolvedMaterialProps::default()
        };
        assert!(build_diffuse_candidates("x", &material).is_empty());
        assert_eq!(missing_image(&material), MissingImage::White);
    }
}
