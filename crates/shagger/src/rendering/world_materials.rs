//! How a resolved shader material draws: its base image candidates (SHAG
//! `src/engine/loader/bsp/BspMeshBuilder.ts`, `buildDiffuseCandidates` in
//! `loadTextures`) and its blend, alpha test and depth state
//! (`src/engine/loader/shader/Q3Blend.ts`, `configureQ3Alpha` and
//! `q3AlphaMode`, with `createMaterialFromProps`), and which of
//! `loadTextures`' special materials it takes instead of the
//! StandardMaterial: the glow overlay (`Q3ShaderMaterialFactory.ts`,
//! `analyzeShaderPattern`), the filter blend (`FilterBlendShaderMaterial.ts`)
//! and the vertex lighting plugin (`BspVertexLightingPlugin.ts`).

use crate::loader::shader::q3_shader_types::{
    AlphaMode, BlendFactor, Q3Shader, Q3ShaderStage, ResolvedBranch, ResolvedMaterialProps,
};

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

/// `isAdditiveBlend` (Q3ShaderMaterialFactory.ts).
fn is_additive_blend(stage: &Q3ShaderStage) -> bool {
    matches!(
        stage.blend_func,
        Some([BlendFactor::One, BlendFactor::One] | [BlendFactor::SrcAlpha, BlendFactor::One])
    )
}

/// `isAlphaBlend` (Q3ShaderMaterialFactory.ts).
fn is_alpha_blend(stage: &Q3ShaderStage) -> bool {
    stage.blend_func == Some([BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha])
}

/// `isGlowTextureName`.
fn is_glow_texture_name(map: &str) -> bool {
    let lower = map.to_lowercase();
    lower.contains("_glow") || lower.contains("_add") || lower.contains("_bloom") || lower.ends_with("glow")
}

/// The glow overlay pattern's stages (`analyzeShaderPattern`'s
/// `glow_overlay`): base × lightmap + glow, the glow added or, under
/// `blendFunc blend`, mixed in by its alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlowOverlay {
    /// The glow stage's image, as the shader names it.
    pub glow_map: String,
    /// `glowIsAlphaBlend`.
    pub alpha_blend: bool,
}

/// `analyzeShaderPattern`, as far as `loadTextures` asks it: the glow stage
/// of a shader whose pattern is `glow_overlay`.
#[must_use]
pub fn analyze_glow_overlay(shader: &Q3Shader) -> Option<GlowOverlay> {
    let stages = &shader.stages;
    let mut base = None;
    let mut lightmap = false;
    let mut glow = None;
    for (i, stage) in stages.iter().enumerate() {
        if stage.map == "$lightmap" {
            lightmap = true;
            continue;
        }
        if stage.map == "$whiteimage" {
            continue;
        }
        if base.is_none() {
            base = Some(i);
        } else if is_additive_blend(stage) || is_glow_texture_name(&stage.map) {
            glow = Some((i, is_alpha_blend(stage)));
        }
    }
    let texture_stages = stages
        .iter()
        .filter(|s| !s.map.is_empty() && s.map != "$lightmap" && s.map != "$whiteimage")
        .count();
    base?;
    // `nolightmap_emissive` comes first.
    if shader.has_surface_parm("nolightmap") && texture_stages == 1 {
        return None;
    }
    let (index, alpha_blend) = glow?;
    (lightmap && texture_stages <= 3).then(|| GlowOverlay {
        glow_map: stages[index].map.clone(),
        alpha_blend,
    })
}

/// Whether `loadTextures` gives the pass the glow overlay ShaderMaterial
/// (when its base and glow images are found): a shader's only pass, without
/// material metadata or conditional blocks.
#[must_use]
pub fn glow_overlay(shader: Option<&Q3Shader>, props: &ResolvedMaterialProps, layered: bool) -> Option<GlowOverlay> {
    let shader = shader?;
    if layered
        || props.has_material_metadata
        || props.resolved_branch != ResolvedBranch::Default
        || shader.conditional_blocks.as_ref().is_some_and(|b| !b.is_empty())
    {
        return None;
    }
    analyze_glow_overlay(shader)
}

/// `loadTextures`' filter blend branch: a multiply pass without an alpha
/// test draws with `createFilterBlendMaterial` (its image unlit and
/// unanimated, both sides, no depth writes, `zOffset` -4).
#[must_use]
pub fn uses_filter_blend_material(props: &ResolvedMaterialProps) -> bool {
    props.alpha_mode == AlphaMode::Multiply && !props.alpha_cutoff.is_some_and(|c| c > 0.0)
}

/// `loadTextures`' `BspVertexLightingPlugin` branch: an opaque fullbright
/// material-metadata surface lit by its vertex colour keeps lighting on,
/// so game lights add to it.
#[must_use]
pub fn uses_vertex_lighting_plugin(props: &ResolvedMaterialProps) -> bool {
    props.has_material_metadata
        && props.uses_vertex_rgb == Some(true)
        && props.is_fullbright
        && props.alpha_mode == AlphaMode::Opaque
}

/// Whether the StandardMaterial takes `useEmissiveAsIllumination` (an
/// additive pass, or a fullbright one without vertex colour that is opaque,
/// additive or tinted grey), which a `tcGen environment` pass's colour
/// follows.
#[must_use]
pub fn emissive_as_illumination(props: &ResolvedMaterialProps) -> bool {
    if props.alpha_mode == AlphaMode::Add {
        return true;
    }
    if !props.is_fullbright || props.uses_vertex_rgb == Some(true) {
        return false;
    }
    let grey = props
        .rgb_gen
        .filter(|g| g.kind == crate::loader::shader::q3_shader_types::RgbGenType::Const)
        .and_then(|g| g.color)
        .is_some_and(|c| c[0] == c[1] && c[1] == c[2]);
    props.alpha_mode == AlphaMode::Opaque || grey
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::shader::q3_shader_parser::Q3ShaderParser;

    fn shader(script: &str) -> Q3Shader {
        Q3ShaderParser::new().parse_shader_file(script).unwrap().remove(0)
    }

    #[test]
    fn glow_overlay_pattern() {
        // textures/lights/neonlamp_2500 (washoi_dm7).
        let lamp = shader(
            "textures/lights/neonlamp_2500 {\n{ map $lightmap\nrgbGen identity }\n{ map textures/lights/neonlamp.png\nblendFunc filter }\n{ map textures/lights/neonlamp.blend.png\nblendfunc add }\n}",
        );
        assert_eq!(
            analyze_glow_overlay(&lamp),
            Some(GlowOverlay {
                glow_map: "textures/lights/neonlamp.blend.png".into(),
                alpha_blend: false
            })
        );
        let props = ResolvedMaterialProps::default();
        assert!(glow_overlay(Some(&lamp), &props, false).is_some());
        assert!(glow_overlay(Some(&lamp), &props, true).is_none());
        let material = ResolvedMaterialProps {
            has_material_metadata: true,
            ..ResolvedMaterialProps::default()
        };
        assert!(glow_overlay(Some(&lamp), &material, false).is_none());
        // Without a lightmap stage, or with one image and nolightmap, it is not.
        let unlit = shader("x/a {\n{ map x/a.png }\n{ map x/a_glow.png\nblendFunc add }\n}");
        assert!(analyze_glow_overlay(&unlit).is_none());
        let emissive = shader("x/b {\nsurfaceparm nolightmap\n{ map $lightmap }\n{ map x/b.png }\n}");
        assert!(analyze_glow_overlay(&emissive).is_none());
        let blended = shader("x/c {\n{ map $lightmap }\n{ map x/c.png }\n{ map x/c_glow.png\nblendFunc blend }\n}");
        assert!(analyze_glow_overlay(&blended).is_some_and(|g| g.alpha_blend));
    }

    #[test]
    fn special_materials() {
        let filter = ResolvedMaterialProps {
            alpha_mode: AlphaMode::Multiply,
            ..ResolvedMaterialProps::default()
        };
        assert!(uses_filter_blend_material(&filter));
        let cutout = ResolvedMaterialProps {
            alpha_cutoff: Some(0.5),
            ..filter.clone()
        };
        assert!(!uses_filter_blend_material(&cutout));
        let vertex = ResolvedMaterialProps {
            has_material_metadata: true,
            uses_vertex_rgb: Some(true),
            is_fullbright: true,
            ..ResolvedMaterialProps::default()
        };
        assert!(uses_vertex_lighting_plugin(&vertex));
        assert!(!emissive_as_illumination(&vertex));
        let glass = ResolvedMaterialProps {
            alpha_mode: AlphaMode::Add,
            is_fullbright: true,
            ..ResolvedMaterialProps::default()
        };
        assert!(emissive_as_illumination(&glass));
        let lamp = ResolvedMaterialProps {
            is_fullbright: true,
            ..ResolvedMaterialProps::default()
        };
        assert!(emissive_as_illumination(&lamp));
    }

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
