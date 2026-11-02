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

/// `addTexturePrefixes` (the builder's own, which drops special tokens): the
/// candidates for a path a stage names, such as an `animMap` frame.
pub fn add_texture_prefixes(path: &str) -> Vec<String> {
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

/// `loadOptionalTexture`'s candidates for a companion image (normal, gloss,
/// glow): none when the shader blanks it, the named image's when it names
/// one, otherwise the found base image's path with each suffix.
#[must_use]
pub fn optional_texture_candidates(
    explicit_path: Option<&str>,
    explicit_blank: bool,
    fallback_base_path: &str,
    suffixes: &[&str],
) -> Vec<String> {
    if explicit_blank {
        return Vec::new();
    }
    if let Some(path) = explicit_path.filter(|p| !p.is_empty()) {
        return add_texture_prefixes(path);
    }
    if fallback_base_path.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for suffix in suffixes {
        candidates.extend(add_texture_prefixes(&format!("{fallback_base_path}{suffix}")));
    }
    dedupe_paths(candidates)
}

/// The normal map suffixes `loadTextures` tries.
pub const NORMAL_SUFFIXES: [&str; 3] = ["_norm", "_normal", "_bump"];
/// The gloss map suffixes `loadTextures` tries.
pub const GLOSS_SUFFIXES: [&str; 2] = ["_gloss", "_spec"];

/// `shouldUseGeneratedPBR`: an opaque surface with both a normal and a gloss
/// map draws with the generated specular-gloss PBR material.
#[must_use]
pub fn should_use_generated_pbr(props: &ResolvedMaterialProps, has_normal_map: bool, has_gloss_map: bool) -> bool {
    if !has_normal_map || !has_gloss_map {
        return false;
    }
    if props.is_transparent || props.alpha_mode != AlphaMode::Opaque {
        return false;
    }
    if props.alpha_cutoff.is_some() {
        return false;
    }
    !(props.is_sky || props.is_fullbright)
}

/// Qfusion's gloss strength (`glossIntensity`, default 1.5).
#[must_use]
pub fn gloss_intensity(props: &ResolvedMaterialProps) -> f64 {
    props.gloss_intensity.unwrap_or(1.5)
}

/// Qfusion's gloss lobe (`glossExponent`, default 24).
#[must_use]
pub fn gloss_exponent(props: &ResolvedMaterialProps) -> f64 {
    props.gloss_exponent.unwrap_or(24.0)
}

/// `createGeneratedPBRMaterial`'s microsurface for a gloss exponent.
#[must_use]
pub fn pbr_micro_surface(exponent: f64) -> f64 {
    1.0 - (2.0 / (exponent + 2.0)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_candidates() {
        assert!(optional_texture_candidates(Some("x"), true, "textures/a/b", &NORMAL_SUFFIXES).is_empty());
        assert_eq!(
            optional_texture_candidates(Some("a/b_n"), false, "textures/a/b", &NORMAL_SUFFIXES),
            ["a/b_n", "textures/a/b_n"]
        );
        assert_eq!(
            optional_texture_candidates(None, false, "textures/a/b", &GLOSS_SUFFIXES),
            ["textures/a/b_gloss", "textures/a/b_spec"]
        );
        assert!(optional_texture_candidates(None, false, "", &GLOSS_SUFFIXES).is_empty());
    }

    #[test]
    fn generated_pbr_needs_both_maps_on_an_opaque_surface() {
        let props = ResolvedMaterialProps::default();
        assert!(should_use_generated_pbr(&props, true, true));
        assert!(!should_use_generated_pbr(&props, true, false));
        let cutout = ResolvedMaterialProps {
            alpha_cutoff: Some(0.5),
            ..ResolvedMaterialProps::default()
        };
        assert!(!should_use_generated_pbr(&cutout, true, true));
        assert!((pbr_micro_surface(24.0) - (1.0 - (2.0f64 / 26.0).sqrt())).abs() < 1e-15);
    }

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
