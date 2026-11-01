//! SHAG `src/engine/loader/shader/Q3ShaderRegistry.ts`: the parsed shader
//! scripts of the mounted packages, looked up by texture name, and the
//! material each one resolves to.
//!
//! Not a singleton here: the caller owns the registry and passes the
//! [`Pk3Loader`] it was loaded from. The TypeScript caches resolved props by
//! key; resolution is cheap and pure, so this port resolves every time.
//! Stage identity (the TypeScript compares stage objects) is
//! [`Q3ShaderStage::id`].

use std::collections::{HashMap, HashSet};

use super::q3_shader_parser::Q3ShaderParser;
use super::q3_shader_types::{
    AlphaFunc, AlphaGenType, AlphaMode, BlendFactor, CullMode, DeformType, Q3MaterialDirective, Q3Shader,
    Q3ShaderStage, ResolveMaterialOptions, ResolvedBranch, ResolvedMaterialProps, RgbGenType, TcGenMode,
};
use crate::loader::pk3::pk3_loader::{Pk3Loader, locale_compare};
use crate::loader::texture::texture_extensions::{PACKAGED_TEXTURE_EXTENSION, strip_texture_extension};

/// `MaterialChannelResolution`.
#[derive(Debug, Clone, Default)]
struct MaterialChannelResolution {
    value: Option<String>,
    explicitly_blank: bool,
}

/// `NormalizedMaterialDirective`.
#[derive(Debug, Clone)]
struct NormalizedMaterialDirective {
    tokens: Vec<String>,
    diffuse: MaterialChannelResolution,
    normal: MaterialChannelResolution,
    gloss: MaterialChannelResolution,
    glow: MaterialChannelResolution,
}

const IMPLICIT_TEXTURE_EXTENSIONS: [&str; 6] = [PACKAGED_TEXTURE_EXTENSION, ".ktx", ".tga", ".png", ".jpg", ".jpeg"];
const IMPLICIT_NORMAL_SUFFIXES: [&str; 3] = ["_norm", "_normal", "_bump"];
const IMPLICIT_GLOSS_SUFFIXES: [&str; 2] = ["_gloss", "_spec"];
const IMPLICIT_GLOW_SUFFIXES: [&str; 2] = ["_glow", "_add"];

/// `normalizeShaderName`: lower case, no image extension.
#[must_use]
pub fn normalize_shader_name(texture_name: &str) -> String {
    strip_texture_extension(&texture_name.to_lowercase()).to_owned()
}

/// `normalizeTextureReference`: forward slashes.
fn normalize_texture_reference(texture_path: &str) -> String {
    texture_path.replace('\\', "/")
}

/// `addTexturePrefixes`.
fn add_texture_prefixes(path: &str) -> Vec<String> {
    let normalized = normalize_texture_reference(path);
    let lower = normalized.to_lowercase();
    let mut candidates = vec![normalized.clone()];
    if !lower.starts_with("textures/") && !lower.starts_with("models/") {
        candidates.push(format!("textures/{normalized}"));
    }
    candidates
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

/// `dedupePaths`.
fn dedupe_paths(paths: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        let normalized = normalize_texture_reference(&path);
        if normalized.is_empty() || !seen.insert(normalized.to_lowercase()) {
            continue;
        }
        deduped.push(normalized);
    }
    deduped
}

/// `isExplicitBlankToken`.
fn is_explicit_blank_token(token: &str) -> bool {
    let normalized = token.to_lowercase();
    normalized == "-" || normalized == "$blankbumpimage"
}

/// `isLightmapStage`.
fn is_lightmap_stage(stage: &Q3ShaderStage) -> bool {
    stage.map.eq_ignore_ascii_case("$lightmap")
}

/// `isGeneratedCubemapStage`: `map $cubemap`, a reflection of the map
/// captured where the surface stands.
#[must_use]
pub fn is_generated_cubemap_stage(stage: &Q3ShaderStage) -> bool {
    stage.map.eq_ignore_ascii_case("$cubemap")
}

/// JavaScript truthiness of an optional string.
fn truthy(value: Option<&String>) -> bool {
    value.is_some_and(|s| !s.is_empty())
}

/// `isRenderableStage`.
fn is_renderable_stage(stage: &Q3ShaderStage) -> bool {
    truthy(stage.cube_map.as_ref())
        || truthy(stage.material.as_ref().and_then(|m| m.diffuse.as_ref()))
        || stage.anim_map.as_ref().is_some_and(|a| !a.frames.is_empty())
        || (!stage.map.is_empty() && !is_lightmap_stage(stage))
}

/// `isAdditiveGlowStage`: an additive image stage after a `material` stage,
/// which a collapsed material draws as its glow.
fn is_additive_glow_stage(stage: &Q3ShaderStage) -> bool {
    (!stage.map.is_empty() || stage.anim_map.as_ref().is_some_and(|a| !a.frames.is_empty()))
        && !is_lightmap_stage(stage)
        && stage.blend_func == Some([BlendFactor::One, BlendFactor::One])
}

/// `hasFilterBlend`.
fn has_filter_blend(stage: &Q3ShaderStage) -> bool {
    stage.blend_func == Some([BlendFactor::DstColor, BlendFactor::Zero])
}

/// `Q3ShaderRegistry`.
#[derive(Debug, Default)]
pub struct Q3ShaderRegistry {
    /// Shader name (lower case, no extension) to its definition.
    shaders: HashMap<String, Q3Shader>,
    /// Script files read so far.
    processed_shader_files: HashSet<String>,
    /// The parser keeps `template` definitions across files, as the
    /// TypeScript module does.
    parser: Q3ShaderParser,
    loaded: bool,
}

impl Q3ShaderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `isLoaded`.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// `loadShaders`: read the `scripts/*.shader` files not read before, in
    /// owning package order; a later definition of a name replaces an
    /// earlier one. Returns the number of shaders known. A script that does
    /// not parse is skipped whole, with a warning.
    pub fn load_shaders(&mut self, pk3: &mut Pk3Loader) -> usize {
        let mut shader_files: Vec<String> = pk3
            .list_files("scripts/*.shader")
            .into_iter()
            .map(str::to_owned)
            .collect();
        // Array.prototype.sort is stable, like sort_by.
        shader_files.sort_by(|a, b| {
            locale_compare(
                pk3.package_for_file(a).unwrap_or(""),
                pk3.package_for_file(b).unwrap_or(""),
            )
        });
        for shader_file in shader_files {
            if !self.processed_shader_files.insert(shader_file.clone()) {
                continue;
            }
            let Some(data) = pk3.get_file(&shader_file) else {
                continue;
            };
            // TextDecoder("utf-8"): lossy, and drops a byte order mark.
            let text = String::from_utf8_lossy(&data);
            let content = text.strip_prefix('\u{feff}').unwrap_or(&text);
            match self.parser.parse_shader_file(content) {
                Ok(parsed) => {
                    for shader in parsed {
                        self.shaders.insert(normalize_shader_name(&shader.name), shader);
                    }
                }
                Err(err) => {
                    eprintln!("[Q3ShaderRegistry] Failed to parse {shader_file}: {err}");
                }
            }
        }
        self.loaded = true;
        self.shaders.len()
    }

    /// `getShader`: the shader for a texture name, trying `_` as `/` for
    /// names like `models_mapobjects_lamp_lamp_halo`.
    #[must_use]
    pub fn get_shader(&self, texture_name: &str) -> Option<&Q3Shader> {
        let name = normalize_shader_name(texture_name);
        if let Some(shader) = self.shaders.get(&name) {
            return Some(shader);
        }
        if name.contains('_') && !name.contains('/') {
            let parts: Vec<&str> = name.split('_').collect();
            for path_segments in (1..parts.len()).rev() {
                let candidate = format!(
                    "{}/{}",
                    parts[..path_segments].join("/"),
                    parts[path_segments..].join("_")
                );
                if let Some(shader) = self.shaders.get(&candidate) {
                    return Some(shader);
                }
            }
            if let Some(shader) = self.shaders.get(&parts.join("/")) {
                return Some(shader);
            }
        }
        None
    }

    /// `hasShader`.
    #[must_use]
    pub fn has_shader(&self, texture_name: &str) -> bool {
        self.get_shader(texture_name).is_some()
    }

    /// `getShaderCount`.
    #[must_use]
    pub fn shader_count(&self) -> usize {
        self.shaders.len()
    }

    /// `getShaderNames`.
    pub fn shader_names(&self) -> impl Iterator<Item = &str> {
        self.shaders.keys().map(String::as_str)
    }

    /// `resolveMaterialProps`: what a surface with this texture draws with.
    #[must_use]
    pub fn resolve_material_props(
        &self,
        pk3: &Pk3Loader,
        texture_name: &str,
        options: &ResolveMaterialOptions,
    ) -> ResolvedMaterialProps {
        let defaults = ResolvedMaterialProps::default();
        let Some(shader) = self.get_shader(texture_name) else {
            let Some(synthetic) = create_implicit_material_directive(pk3, texture_name) else {
                return defaults;
            };
            let mut props = defaults;
            apply_normalized_material_to_props(&mut props, normalize_material_directive(&synthetic));
            return props;
        };

        let mut props = defaults;
        let (stages, branch) = get_resolved_stages(shader, options.deluxe, options.texture_cube_map != Some(false));
        let selected: Vec<&Q3ShaderStage> = match options.pass_index {
            None => stages,
            Some(i) => stages.into_iter().skip(i).take(1).collect(),
        };
        let active: Vec<&Q3ShaderStage> = if options.without_generated_cubemap {
            selected
                .into_iter()
                .filter(|s| !is_generated_cubemap_stage(s))
                .collect()
        } else {
            selected
        };
        let material_index = active.iter().position(|s| s.material.is_some());
        let material_stage = material_index.map(|i| active[i]);
        // A collapsed material uses its own UV/blend state. An earlier
        // environment background pass must not turn the material's diffuse
        // map into a reflection.
        let primary = material_stage.or_else(|| get_primary_stage(&active));
        let classic = is_classic_lightmap_composition(&active);

        props.resolved_branch = branch;
        props.soft_particle = shader.soft_particle;
        props.gloss_intensity = shader.gloss_intensity;
        props.gloss_exponent = shader.gloss_exponent;
        props.primary_stage = primary.cloned();
        props.blend_func = primary.and_then(|s| s.blend_func);
        let rgb_type = primary.and_then(|s| s.rgb_gen).map(|g| g.kind);
        props.uses_vertex_rgb = Some(
            matches!(rgb_type, Some(RgbGenType::Vertex | RgbGenType::OneMinusVertex))
                || (options.world_surface && rgb_type == Some(RgbGenType::LightingDiffuse)),
        );
        props.uses_vertex_alpha = Some(primary.and_then(|s| s.alpha_gen).map(|g| g.kind) == Some(AlphaGenType::Vertex));
        props.rgb_gen = primary.and_then(|s| s.rgb_gen);
        props.environment_map = primary.and_then(|s| s.cube_map.clone());
        if primary.is_some_and(is_generated_cubemap_stage) {
            props.generated_cubemap = Some(true);
        }

        if shader.has_surface_parm("sky") || shader.sky_parms.is_some() {
            props.is_sky = true;
            props.use_lightmap = false;
            props.sky_parms.clone_from(&shader.sky_parms);
        }

        match shader.cull {
            CullMode::None | CullMode::Disable => {
                props.back_face_culling = false;
                props.two_sided_lighting = true;
            }
            // Babylon has no front-face culling; the TypeScript culls back.
            CullMode::Front | CullMode::Back => {}
        }

        if shader.polygon_offset == Some(true) {
            props.polygon_offset = true;
        }
        if shader.has_surface_parm("trans") || shader.has_surface_parm("alphashadow") {
            props.is_transparent = true;
        }
        // Lamp surfaces that don't use the lightmap draw at full brightness.
        if shader.has_surface_parm("nolightmap") {
            props.is_fullbright = true;
            props.use_lightmap = false;
        }
        if let Some(light) = shader.surface_light
            && light > 0.0
        {
            props.surface_light = light;
        }

        if let (Some(stage), Some(index)) = (material_stage, material_index)
            && let Some(material) = &stage.material
        {
            apply_normalized_material_to_props(&mut props, normalize_material_directive(material));
            let glow = active[index + 1..].iter().copied().find(|s| is_additive_glow_stage(s));
            if let Some(glow) = glow
                && !truthy(props.glow_texture.as_ref())
            {
                let reference = if glow.map.is_empty() {
                    glow.anim_map
                        .as_ref()
                        .and_then(|a| a.frames.first())
                        .cloned()
                        .unwrap_or_default()
                } else {
                    glow.map.clone()
                };
                props.glow_texture = Some(normalize_texture_reference(&reference));
                props.glow_texture_explicitly_blank = false;
                props.glow_stage = Some(glow.clone());
            }
        }

        if !active.is_empty() {
            if !truthy(props.diffuse_texture.as_ref())
                && let Some(stage) = primary
                && !stage.map.is_empty()
                && !is_lightmap_stage(stage)
                && props.generated_cubemap != Some(true)
            {
                props.diffuse_texture = Some(normalize_texture_reference(&stage.map));
            }
            if !truthy(props.diffuse_texture.as_ref())
                && let Some(frame) = primary.and_then(|s| s.anim_map.as_ref()).and_then(|a| a.frames.first())
                && !frame.is_empty()
            {
                props.diffuse_texture = Some(normalize_texture_reference(frame));
            }

            props.use_lightmap = !shader.has_surface_parm("nolightmap")
                && (material_stage.is_some() || active.iter().any(|s| is_lightmap_stage(s)));

            // Alpha mode from the representative stage, unless this is the
            // classic base × lightmap composition the lightmap already gives.
            if let Some([src, dst]) = primary.and_then(|s| s.blend_func)
                && !classic
            {
                use BlendFactor as B;
                let mode = match (src, dst) {
                    (B::One, B::One) => Some((AlphaMode::Add, false)),
                    (B::SrcAlpha, B::OneMinusSrcAlpha) => Some((AlphaMode::Blend, true)),
                    (B::DstColor, B::Zero) => Some((AlphaMode::Multiply, false)),
                    (B::SrcAlpha, B::One) | (B::One, B::OneMinusSrcColor) => Some((AlphaMode::Add, false)),
                    (B::Zero, B::OneMinusSrcColor) => Some((AlphaMode::Multiply, true)),
                    (B::One, d) if d != B::Zero => Some((AlphaMode::Add, false)),
                    _ => None,
                };
                if let Some((mode, keep_depth_write)) = mode {
                    props.alpha_mode = mode;
                    props.is_transparent = true;
                    if !keep_depth_write {
                        props.depth_write = false;
                    }
                }
            }

            if let Some(func) = primary.and_then(|s| s.alpha_func) {
                props.is_transparent = true;
                props.alpha_cutoff = Some(match func {
                    AlphaFunc::Gt0 => 0.01,
                    AlphaFunc::Ge128 | AlphaFunc::Lt128 => 0.5,
                });
            }

            if let Some(tc_mod) = primary.and_then(|s| s.tc_mod.as_ref())
                && !tc_mod.is_empty()
            {
                props.needs_animation = true;
                props.tc_mod = Some(tc_mod.clone());
            }
            if primary.is_some_and(|s| s.anim_map.is_some()) {
                props.needs_animation = true;
            }
            if primary.and_then(|s| s.rgb_gen).map(|g| g.kind) == Some(RgbGenType::Wave)
                || primary.and_then(|s| s.alpha_gen).map(|g| g.kind) == Some(AlphaGenType::Wave)
            {
                props.needs_animation = true;
            }
            if let Some(tc_gen) = primary.and_then(|s| s.tc_gen) {
                props.tc_gen = Some(tc_gen);
            }
            if let Some(depth_write) = primary.and_then(|s| s.depth_write) {
                props.depth_write = depth_write;
            }
        }

        for deform in shader.deform_vertexes.iter().flatten() {
            match deform.kind {
                DeformType::Autosprite => props.autosprite = true,
                DeformType::Autosprite2 => props.autosprite2 = true,
                _ => {}
            }
        }

        if let Some(pass_index) = options.pass_index {
            if pass_index > 0 {
                props.depth_write = primary.and_then(|s| s.depth_write).unwrap_or(false);
            }
            if !props.use_lightmap {
                props.is_fullbright = true;
            }
        }
        props
    }

    /// `hasGeneratedCubemap`: whether the selected stages include a
    /// `map $cubemap` stage.
    #[must_use]
    pub fn has_generated_cubemap(&self, texture_name: &str, options: &ResolveMaterialOptions) -> bool {
        self.get_shader(texture_name).is_some_and(|shader| {
            get_resolved_stages(shader, options.deluxe, options.texture_cube_map != Some(false))
                .0
                .into_iter()
                .any(is_generated_cubemap_stage)
        })
    }

    /// `resolveMaterialPasses`: repeated material layers in script order,
    /// or the one collapsed material.
    #[must_use]
    pub fn resolve_material_passes(
        &self,
        pk3: &Pk3Loader,
        texture_name: &str,
        options: &ResolveMaterialOptions,
    ) -> Vec<ResolvedMaterialProps> {
        let stages: Vec<&Q3ShaderStage> = self
            .get_shader(texture_name)
            .map(|shader| get_resolved_stages(shader, options.deluxe, options.texture_cube_map != Some(false)).0)
            .unwrap_or_default();
        let with_pass = |pass_index: usize| ResolveMaterialOptions {
            pass_index: Some(pass_index),
            ..*options
        };

        // A `map $cubemap` stage beside other stages draws as its own pass,
        // blended by its blendFunc, where it stands in the script; the other
        // stages keep their usual single-material composition.
        if stages.iter().any(|s| is_generated_cubemap_stage(s))
            && stages
                .iter()
                .any(|s| is_renderable_stage(s) && !is_generated_cubemap_stage(s))
        {
            // findIndex gives -1 when every stage is a cubemap stage.
            let first = stages
                .iter()
                .position(|s| !is_generated_cubemap_stage(s))
                .map_or(-1, |i| i as i64);
            let mut passes = vec![(
                first,
                self.resolve_material_props(
                    pk3,
                    texture_name,
                    &ResolveMaterialOptions {
                        without_generated_cubemap: true,
                        ..*options
                    },
                ),
            )];
            for (pass_index, stage) in stages.iter().enumerate() {
                if is_generated_cubemap_stage(stage) {
                    passes.push((
                        pass_index as i64,
                        self.resolve_material_props(pk3, texture_name, &with_pass(pass_index)),
                    ));
                }
            }
            passes.sort_by_key(|(at, _)| *at);
            return passes.into_iter().map(|(_, props)| props).collect();
        }

        let first = stages.first().copied();
        let environment_background = first
            .is_some_and(|s| truthy(s.cube_map.as_ref()) || s.tc_gen.is_some_and(|g| g.mode == TcGenMode::Environment));
        // A collapsed material has one glow slot: additive stages beyond it,
        // or any additive stage when the material names its own glow, draw
        // as passes. findIndex's -1 slices from 0.
        let material_index = stages.iter().position(|s| s.material.is_some());
        let after_material = material_index.map_or(0, |i| i + 1);
        let material = material_index.and_then(|i| stages[i].material.as_ref());
        let additive_after_material = stages[after_material..]
            .iter()
            .filter(|s| is_additive_glow_stage(s))
            .count();
        let glow_slots =
            usize::from(!material.is_some_and(|m| truthy(normalize_material_directive(m).glow.value.as_ref())));
        let material_count = stages.iter().filter(|s| s.material.is_some()).count();
        let every_layerable = stages.iter().enumerate().all(|(index, stage)| {
            let has_image = truthy(stage.cube_map.as_ref())
                || stage.material.is_some()
                || !stage.map.is_empty()
                || stage.anim_map.is_some();
            let blend_ok = if index == 0 {
                stage
                    .blend_func
                    .is_none_or(|b| b == [BlendFactor::One, BlendFactor::Zero])
            } else if stage.material.is_some() {
                stage.blend_func == Some([BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha])
            } else {
                matches!(
                    stage.blend_func,
                    Some([BlendFactor::One | BlendFactor::SrcAlpha, BlendFactor::One])
                )
            };
            let tc_gen_ok =
                stage.tc_gen.is_none_or(|g| g.mode == TcGenMode::Base) || (index == 0 && environment_background);
            has_image && !is_lightmap_stage(stage) && blend_ok && tc_gen_ok
        });
        if (!environment_background && material_count < 2 && additive_after_material <= glow_slots)
            || (!environment_background && !first.is_some_and(|s| s.material.is_some()))
            || !every_layerable
        {
            return vec![self.resolve_material_props(pk3, texture_name, options)];
        }
        (0..stages.len())
            .map(|pass_index| self.resolve_material_props(pk3, texture_name, &with_pass(pass_index)))
            .collect()
    }
}

/// `getResolvedStages`: the unconditional stages plus the conditional blocks
/// the options select, in script order.
fn get_resolved_stages(
    shader: &Q3Shader,
    deluxe: bool,
    texture_cube_map: bool,
) -> (Vec<&Q3ShaderStage>, ResolvedBranch) {
    use super::q3_shader_types::Condition;
    let blocks = match &shader.conditional_blocks {
        Some(blocks) if !blocks.is_empty() => blocks,
        _ => {
            return (shader.unconditional_stages.iter().collect(), ResolvedBranch::Default);
        }
    };
    let (requested_condition, requested_branch) = if deluxe {
        (Condition::Deluxe, ResolvedBranch::Deluxe)
    } else {
        (Condition::NotDeluxe, ResolvedBranch::NotDeluxe)
    };
    let cube_condition = if texture_cube_map {
        Condition::TextureCubeMap
    } else {
        Condition::NotTextureCubeMap
    };
    // A Set in insertion order.
    let mut selected: Vec<&Q3ShaderStage> = Vec::new();
    fn add<'a>(selected: &mut Vec<&'a Q3ShaderStage>, stage: &'a Q3ShaderStage) {
        if !selected.iter().any(|s| s.id == stage.id) {
            selected.push(stage);
        }
    }
    for stage in &shader.unconditional_stages {
        add(&mut selected, stage);
    }
    let mut selected_deluxe = false;
    for block in blocks {
        if block.condition == requested_condition || block.condition == cube_condition {
            for stage in &block.stages {
                add(&mut selected, stage);
            }
            if block.condition == requested_condition {
                selected_deluxe = true;
            }
        }
    }
    let mut stages: Vec<&Q3ShaderStage> = shader
        .stages
        .iter()
        .filter(|stage| selected.iter().any(|s| s.id == stage.id))
        .collect();
    stages.extend(
        selected
            .iter()
            .filter(|stage| !shader.stages.iter().any(|s| s.id == stage.id)),
    );
    (
        stages,
        if selected_deluxe {
            requested_branch
        } else {
            ResolvedBranch::Default
        },
    )
}

/// `getPrimaryStage`.
fn get_primary_stage<'a>(stages: &[&'a Q3ShaderStage]) -> Option<&'a Q3ShaderStage> {
    stages
        .iter()
        .find(|s| is_renderable_stage(s) && !is_generated_cubemap_stage(s))
        .or_else(|| stages.iter().find(|s| is_renderable_stage(s)))
        .or_else(|| stages.first())
        .copied()
}

/// `normalizeMaterialDirective`.
fn normalize_material_directive(material: &Q3MaterialDirective) -> NormalizedMaterialDirective {
    let tokens: Vec<String> = material
        .raw_tokens
        .iter()
        .map(|t| normalize_texture_reference(t))
        .collect();
    let resolve = |token: Option<&String>| match token {
        None => MaterialChannelResolution::default(),
        Some(t) if t.is_empty() => MaterialChannelResolution::default(),
        Some(t) if is_explicit_blank_token(t) => MaterialChannelResolution {
            value: None,
            explicitly_blank: true,
        },
        Some(t) => MaterialChannelResolution {
            value: Some(t.clone()),
            explicitly_blank: false,
        },
    };
    NormalizedMaterialDirective {
        diffuse: resolve(tokens.first()),
        normal: resolve(tokens.get(1)),
        gloss: resolve(tokens.get(2)),
        glow: resolve(tokens.get(3)),
        tokens,
    }
}

/// `applyNormalizedMaterialToProps`.
fn apply_normalized_material_to_props(props: &mut ResolvedMaterialProps, material: NormalizedMaterialDirective) {
    props.has_material_metadata = true;
    props.material_tokens = Some(material.tokens);
    props.normal_texture_explicitly_blank = material.normal.explicitly_blank;
    props.gloss_texture_explicitly_blank = material.gloss.explicitly_blank;
    props.glow_texture_explicitly_blank = material.glow.explicitly_blank;
    if material.diffuse.value.is_some() {
        props.diffuse_texture = material.diffuse.value;
    }
    if material.normal.value.is_some() {
        props.normal_texture = material.normal.value;
    }
    if material.gloss.value.is_some() {
        props.gloss_texture = material.gloss.value;
    }
    if material.glow.value.is_some() {
        props.glow_texture = material.glow.value;
    }
}

/// `createImplicitMaterialDirective`: a `material` line for a texture with no
/// script, from the image files that sit beside it (`_norm`, `_gloss`,
/// `_glow`).
fn create_implicit_material_directive(pk3: &Pk3Loader, texture_name: &str) -> Option<Q3MaterialDirective> {
    let candidate_bases = dedupe_paths(
        build_texture_name_fallbacks(&normalize_shader_name(texture_name))
            .iter()
            .flat_map(|c| add_texture_prefixes(c)),
    );
    let diffuse = find_first_existing_texture_path(pk3, &candidate_bases)?;
    let diffuse_base = normalize_shader_name(&diffuse);
    let with_suffixes =
        |suffixes: &[&str]| -> Vec<String> { suffixes.iter().map(|s| format!("{diffuse_base}{s}")).collect() };
    let normal = find_first_existing_texture_path(pk3, &with_suffixes(&IMPLICIT_NORMAL_SUFFIXES));
    let gloss = find_first_existing_texture_path(pk3, &with_suffixes(&IMPLICIT_GLOSS_SUFFIXES));
    let glow = find_first_existing_texture_path(pk3, &with_suffixes(&IMPLICIT_GLOW_SUFFIXES));

    let mut raw_tokens = vec![diffuse.clone()];
    if normal.is_some() || gloss.is_some() || glow.is_some() {
        raw_tokens.push(normal.clone().unwrap_or_else(|| "-".to_owned()));
    }
    if gloss.is_some() || glow.is_some() {
        raw_tokens.push(gloss.clone().unwrap_or_else(|| "-".to_owned()));
    }
    if let Some(glow) = &glow {
        raw_tokens.push(glow.clone());
    }
    if !pk3.has_file(&diffuse) {
        return None;
    }
    Some(Q3MaterialDirective {
        raw_tokens,
        diffuse: Some(diffuse),
        normal,
        gloss,
        glow,
        extras: None,
    })
}

/// `findFirstExistingTexturePath`.
fn find_first_existing_texture_path(pk3: &Pk3Loader, base_candidates: &[String]) -> Option<String> {
    for base in base_candidates {
        let normalized = normalize_shader_name(base);
        for ext in IMPLICIT_TEXTURE_EXTENSIONS {
            let candidate = format!("{normalized}{ext}");
            if pk3.has_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// `isClassicLightmapComposition`: base × lightmap with a filter blend.
fn is_classic_lightmap_composition(stages: &[&Q3ShaderStage]) -> bool {
    !stages.is_empty()
        && !stages.iter().any(|s| s.material.is_some())
        && stages.iter().any(|s| is_lightmap_stage(s))
        && stages.iter().any(|s| is_renderable_stage(s))
        && stages.iter().any(|s| has_filter_blend(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(script: &str) -> Q3ShaderRegistry {
        let mut registry = Q3ShaderRegistry::new();
        for shader in registry.parser.parse_shader_file(script).unwrap() {
            registry.shaders.insert(normalize_shader_name(&shader.name), shader);
        }
        registry
    }

    #[test]
    fn underscores_fall_back_to_slashes() {
        let r = registry("models/mapobjects/lamp/lamp_halo { { map $lightmap } }");
        assert!(r.has_shader("models_mapobjects_lamp_lamp_halo"));
        assert!(r.has_shader("MODELS/mapobjects/lamp/lamp_halo.tga"));
        assert!(!r.has_shader("models_mapobjects_lamp"));
    }

    #[test]
    fn classic_lightmap_stays_opaque() {
        let r = registry(
            "textures/base/wall {\n{ map $lightmap\nrgbGen identity }\n{ map textures/base/wall.tga\nblendFunc GL_DST_COLOR GL_ZERO }\n}",
        );
        let props = r.resolve_material_props(
            &Pk3Loader::new(),
            "textures/base/wall",
            &ResolveMaterialOptions::default(),
        );
        assert_eq!(props.alpha_mode, AlphaMode::Opaque);
        assert!(props.use_lightmap);
        assert_eq!(props.diffuse_texture.as_deref(), Some("textures/base/wall.tga"));
    }

    /// Branches the shipped data does not reach (the golden vectors cover
    /// the rest): the inverse multiply blend, and implicit `_add` glow maps.
    #[test]
    fn inverse_multiply_keeps_depth_write() {
        let r = registry("textures/x/inv { { map textures/x/inv.tga\nblendFunc GL_ZERO GL_ONE_MINUS_SRC_COLOR } }");
        let props = r.resolve_material_props(&Pk3Loader::new(), "textures/x/inv", &ResolveMaterialOptions::default());
        assert_eq!(props.alpha_mode, AlphaMode::Multiply);
        assert!(props.is_transparent && props.depth_write);
    }

    #[test]
    fn implicit_material_finds_neighbour_maps() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in ["textures/a/b.webp", "textures/a/b_norm.tga", "textures/a/b_add.png"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        let mut pk3 = Pk3Loader::new();
        pk3.add_package("t.pk3", zip.finish().unwrap().into_inner()).unwrap();
        let props = Q3ShaderRegistry::new().resolve_material_props(&pk3, "a_b", &ResolveMaterialOptions::default());
        assert_eq!(
            props.material_tokens.unwrap(),
            [
                "textures/a/b.webp",
                "textures/a/b_norm.tga",
                "-",
                "textures/a/b_add.png"
            ]
        );
        assert!(props.gloss_texture_explicitly_blank);
        assert_eq!(props.glow_texture.as_deref(), Some("textures/a/b_add.png"));
    }
}
