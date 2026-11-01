//! SHAG `src/engine/loader/shader/Q3ShaderParser.ts`: `.shader` scripts,
//! with Warsow's templates and `if` blocks.
//!
//! The TypeScript keeps the templates in one module-level map shared by every
//! file it parses, so a template defined in one script expands in a later
//! one. Here that map lives in [`Q3ShaderParser`]; parse every script of a
//! load with the same parser to get the same result.
//!
//! JavaScript habits kept on purpose:
//! - `parseFloat(x) || d` gives `d` for 0 as well as for NaN ([`or`]);
//! - a token is "present" only when it is not empty, since `""` is falsy
//!   (a quoted `""` is a token);
//! - template parameters are substituted with `String.prototype.replace`,
//!   so `$$`, `$&`, `` $` `` and `$'` in a parameter are replacement patterns.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use shagger_shared::js::{is_js_whitespace, js_parse_float, js_string_to_number};

use super::q3_shader_types::{
    AlphaFunc, AlphaGen, AlphaGenType, AnimMap, BlendFactor, Condition, CullMode, DEFAULT_SKY_HEIGHT, DeformType,
    DeformVertexes, DepthFunc, DistanceRamp, FogParms, Q3MaterialDirective, Q3Shader, Q3ShaderConditionalBlock,
    Q3ShaderStage, RgbGen, RgbGenType, SkyParms, Sort, TcGen, TcGenMode, TcGenVector, TcMod, WaveForm, WaveFunc,
};

/// Where the TypeScript throws: a `template` directive with nothing after it
/// (`templateName.toLowerCase()` on `undefined`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderParseError(pub String);

impl std::fmt::Display for ShaderParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShaderParseError {}

/// The parser and the templates it has collected so far.
#[derive(Debug, Clone, Default)]
pub struct Q3ShaderParser {
    /// Lower-cased template name → body tokens.
    templates: HashMap<String, Vec<String>>,
}

type Tokens = [String];

fn tok(tokens: &Tokens, pos: usize) -> Option<&str> {
    tokens.get(pos).map(String::as_str)
}

/// `if (tokens[pos])`: there, and not empty.
fn present(tokens: &Tokens, pos: usize) -> bool {
    tok(tokens, pos).is_some_and(|t| !t.is_empty())
}

/// `parseFloat(tokens[pos])`, NaN past the end.
fn pf(tokens: &Tokens, pos: usize) -> f64 {
    tok(tokens, pos).map_or(f64::NAN, js_parse_float)
}

/// `x || d` for a number: `d` when `x` is 0, -0 or NaN.
fn or(x: f64, d: f64) -> f64 {
    if x == 0.0 || x.is_nan() { d } else { x }
}

fn lower(tokens: &Tokens, pos: usize) -> Option<String> {
    tok(tokens, pos).map(str::to_lowercase)
}

impl Q3ShaderParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `parseShaderFile`: every shader in one script, in order.
    ///
    /// # Errors
    /// Where the TypeScript throws (see [`ShaderParseError`]).
    pub fn parse_shader_file(&mut self, content: &str) -> Result<Vec<Q3Shader>, ShaderParseError> {
        let tokens = tokenize(content);
        let mut shaders = Vec::new();

        // First pass: collect templates.
        let mut pos = 0;
        while pos < tokens.len() {
            if tokens[pos] == "{" || tokens[pos] == "}" {
                pos += 1;
                continue;
            }
            let name = &tokens[pos];
            pos += 1;
            if name.is_empty() || pos >= tokens.len() {
                break;
            }
            if tokens[pos] != "{" {
                continue;
            }
            pos += 1;
            let body_start = pos;
            let mut depth = 1;
            while pos < tokens.len() && depth > 0 {
                if tokens[pos] == "{" {
                    depth += 1;
                } else if tokens[pos] == "}" {
                    depth -= 1;
                }
                pos += 1;
            }
            // `pos - 1` excludes the closing brace; an unclosed body runs to the end
            // less its last token, as `slice(bodyStart, pos - 1)` does.
            let body_end = (pos - 1).max(body_start);
            if name.to_lowercase().ends_with("_template") {
                self.templates
                    .insert(name.to_lowercase(), tokens[body_start..body_end].to_vec());
            }
        }

        // Second pass: shaders, with templates expanded.
        pos = 0;
        while pos < tokens.len() {
            if tokens[pos] == "{" || tokens[pos] == "}" {
                pos += 1;
                continue;
            }
            let name = tokens[pos].clone();
            pos += 1;
            if name.is_empty() || pos >= tokens.len() {
                break;
            }
            if tokens[pos] != "{" {
                continue;
            }
            pos += 1;
            if name.to_lowercase().ends_with("_template") {
                let mut depth = 1;
                while pos < tokens.len() && depth > 0 {
                    if tokens[pos] == "{" {
                        depth += 1;
                    } else if tokens[pos] == "}" {
                        depth -= 1;
                    }
                    pos += 1;
                }
                continue;
            }
            if lower(&tokens, pos).as_deref() == Some("template") {
                pos += 1;
                let template_name = tok(&tokens, pos)
                    .ok_or_else(|| ShaderParseError(format!("{name}: `template` without a name")))?
                    .to_lowercase();
                pos += 1;
                let mut params = Vec::new();
                while pos < tokens.len() && tokens[pos] != "}" {
                    params.push(tokens[pos].clone());
                    pos += 1;
                }
                pos += 1;
                if let Some(body) = self.templates.get(&template_name) {
                    let expanded = expand_template(body, &params);
                    let (shader, _) = parse_shader_body(&name, &expanded, 0);
                    shaders.push(shader);
                }
            } else {
                let (shader, next) = parse_shader_body(&name, &tokens, pos);
                shaders.push(shader);
                pos = next;
            }
        }
        Ok(shaders)
    }
}

/// `expandTemplate`: `$1`, `$2`, ... replaced by the parameters, one
/// parameter after another over each token.
fn expand_template(body: &[String], params: &[String]) -> Vec<String> {
    body.iter()
        .map(|token| {
            let mut result = token.clone();
            for (i, param) in params.iter().enumerate() {
                result = js_replace_all(&result, &format!("${}", i + 1), param);
            }
            result
        })
        .collect()
}

/// `haystack.replace(/needle/g, replacement)` for a needle with no regex
/// specials beyond the escaped `$`, and no capture groups: the replacement's
/// `$$`, `$&`, `` $` `` and `$'` patterns apply; any other `$` is literal.
#[must_use]
pub fn js_replace_all(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_owned();
    }
    let mut out = String::new();
    let mut last = 0;
    for (at, matched) in haystack.match_indices(needle) {
        out.push_str(&haystack[last..at]);
        let mut chars = replacement.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            match chars.peek().map(|&(_, n)| n) {
                Some('$') => {
                    out.push('$');
                    chars.next();
                }
                Some('&') => {
                    out.push_str(matched);
                    chars.next();
                }
                Some('`') => {
                    out.push_str(&haystack[..at]);
                    chars.next();
                }
                Some('\'') => {
                    out.push_str(&haystack[at + matched.len()..]);
                    chars.next();
                }
                _ => out.push('$'),
            }
        }
        last = at + matched.len();
    }
    out.push_str(&haystack[last..]);
    out
}

/// `tokenize`: `//` and `/* */` comments dropped, braces as tokens of their
/// own, quoted strings as one token (possibly empty), everything else split
/// on white space and braces.
#[must_use]
pub fn tokenize(content: &str) -> Vec<String> {
    let c: Vec<char> = content.chars().collect();
    let len = c.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < len {
        let ch = c[i];
        if is_js_whitespace(ch) {
            i += 1;
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < len && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < len && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if ch == '{' || ch == '}' {
            tokens.push(ch.to_string());
            i += 1;
            continue;
        }
        if ch == '"' {
            i += 1;
            let mut s = String::new();
            while i < len && c[i] != '"' {
                s.push(c[i]);
                i += 1;
            }
            i += 1;
            tokens.push(s);
            continue;
        }
        let mut token = String::new();
        while i < len && !is_js_whitespace(c[i]) && c[i] != '{' && c[i] != '}' {
            token.push(c[i]);
            i += 1;
        }
        if !token.is_empty() {
            tokens.push(token);
        }
    }
    tokens
}

/// `isDirective`.
fn is_directive(token: &str) -> bool {
    matches!(
        token.to_lowercase().as_str(),
        "surfaceparm"
            | "glossintensity"
            | "glossexponent"
            | "cull"
            | "sort"
            | "nomipmaps"
            | "nopicmip"
            | "polygonoffset"
            | "softparticle"
            | "portal"
            | "entitymergable"
            | "deformvertexes"
            | "fogparms"
            | "skyparms"
            | "skyparmssides"
            | "qer_editorimage"
            | "if"
            | "endif"
    )
}

/// `isStageDirective`.
fn is_stage_directive(token: &str) -> bool {
    matches!(
        token.to_lowercase().as_str(),
        "map"
            | "clampmap"
            | "cubemap"
            | "surroundmap"
            | "animmap"
            | "animclampmap"
            | "material"
            | "celshade"
            | "blendfunc"
            | "alphafunc"
            | "rgbgen"
            | "alphagen"
            | "tcmod"
            | "tcgen"
            | "depthwrite"
            | "depthfunc"
            | "detail"
    )
}

/// A parameter of a shader directive: not a brace and not another directive.
fn is_parameter(tokens: &Tokens, pos: usize) -> bool {
    tok(tokens, pos).is_some_and(|t| t != "{" && t != "}" && !is_directive(t))
}

/// A parameter of a stage directive: not the stage's end, not another directive.
fn is_stage_parameter(tokens: &Tokens, pos: usize) -> bool {
    tok(tokens, pos).is_some_and(|t| t != "}" && !is_stage_directive(t))
}

/// `parseShaderBody`.
fn parse_shader_body(name: &str, tokens: &Tokens, start: usize) -> (Q3Shader, usize) {
    let mut shader = Q3Shader::new(name);
    let mut pos = start;
    let mut depth = 1;
    while pos < tokens.len() && depth > 0 {
        let token = tokens[pos].to_lowercase();
        if token == "{" {
            let (stage, next) = parse_stage(tokens, pos + 1);
            shader.stages.push(stage.clone());
            shader.unconditional_stages.push(stage);
            pos = next;
            continue;
        }
        if token == "}" {
            depth -= 1;
            pos += 1;
            continue;
        }
        match token.as_str() {
            "surfaceparm" => {
                if present(tokens, pos + 1) {
                    let parm = tokens[pos + 1].to_lowercase();
                    if !shader.has_surface_parm(&parm) {
                        shader.surface_parms.push(parm);
                    }
                }
                pos += 2;
            }
            "cull" => {
                if present(tokens, pos + 1) {
                    shader.cull = match tokens[pos + 1].to_lowercase().as_str() {
                        "none" | "disable" | "twosided" => CullMode::None,
                        "front" => CullMode::Front,
                        _ => CullMode::Back,
                    };
                }
                pos += 2;
            }
            "sort" => {
                if present(tokens, pos + 1) {
                    let value = tokens[pos + 1].to_lowercase();
                    let n = js_parse_float(&value);
                    shader.sort = Some(if n.is_nan() { Sort::Name(value) } else { Sort::Number(n) });
                }
                pos += 2;
            }
            "glossintensity" | "glossexponent" => {
                let value = tok(tokens, pos + 1).map_or(f64::NAN, js_string_to_number);
                if value.is_finite() && value > 0.0 {
                    if token == "glossintensity" {
                        shader.gloss_intensity = Some(value);
                    } else {
                        shader.gloss_exponent = Some(value);
                    }
                }
                pos += if value.is_finite() { 2 } else { 1 };
            }
            "nomipmaps" => {
                shader.nomipmaps = Some(true);
                pos += 1;
            }
            "nopicmip" => {
                shader.nopicmip = Some(true);
                pos += 1;
            }
            "nofog" => {
                shader.no_fog = Some(true);
                pos += 1;
            }
            "softparticle" => {
                shader.soft_particle = Some(true);
                pos += 1;
            }
            "polygonoffset" => {
                shader.polygon_offset = Some(true);
                pos += 1;
            }
            "portal" => {
                shader.portal = Some(true);
                pos += 1;
            }
            "entitymergable" => {
                shader.entity_mergable = Some(true);
                pos += 1;
            }
            "deformvertexes" => {
                pos += 1;
                if pos >= tokens.len() {
                    continue;
                }
                let kind = tokens[pos].to_lowercase();
                pos += 1;
                let list = shader.deform_vertexes.get_or_insert_with(Vec::new);
                match kind.as_str() {
                    "autosprite" => list.push(DeformVertexes {
                        kind: DeformType::Autosprite,
                    }),
                    "autosprite2" => list.push(DeformVertexes {
                        kind: DeformType::Autosprite2,
                    }),
                    _ => {
                        while is_parameter(tokens, pos) {
                            pos += 1;
                        }
                    }
                }
            }
            "fogparms" => {
                pos += 1;
                if tok(tokens, pos) == Some("(") {
                    pos += 1;
                }
                let mut rgb = [0.5, 0.5, 0.5];
                for channel in &mut rgb {
                    if is_parameter(tokens, pos) && tok(tokens, pos) != Some(")") {
                        *channel = or(pf(tokens, pos), 0.0);
                        pos += 1;
                    }
                }
                if tok(tokens, pos) == Some(")") {
                    pos += 1;
                }
                let mut depth_for_opaque = 512.0;
                if is_parameter(tokens, pos) {
                    depth_for_opaque = or(pf(tokens, pos), 512.0);
                    pos += 1;
                }
                shader.fog_parms = Some(FogParms {
                    color: rgb,
                    depth_for_opaque,
                });
            }
            "skyparms" => {
                pos += 1;
                let mut words = ["-".to_owned(), "-".to_owned(), "-".to_owned()];
                for word in &mut words {
                    if is_parameter(tokens, pos) {
                        word.clone_from(&tokens[pos]);
                        pos += 1;
                    }
                }
                let [farbox, cloud, nearbox] = words;
                let cloud_height = if cloud == "-" {
                    0.0
                } else {
                    or(js_parse_float(&cloud), 0.0)
                };
                shader.sky_parms = Some(SkyParms {
                    farbox: if farbox == "-" { String::new() } else { farbox },
                    cloud_height,
                    nearbox: if nearbox == "-" { String::new() } else { nearbox },
                    sky_height: if cloud_height > 0.0 {
                        cloud_height
                    } else {
                        DEFAULT_SKY_HEIGHT
                    },
                    sides: None,
                });
            }
            "skyparmssides" => {
                pos += 1;
                let mut words: Vec<String> = Vec::new();
                while is_parameter(tokens, pos) {
                    words.push(tokens[pos].clone());
                    pos += 1;
                }
                let sides = resolve_sky_sides(&words[..words.len().min(6)]);
                let height = words.get(6).filter(|w| !w.is_empty() && *w != "-");
                let cloud_height = height.map_or(0.0, |w| or(js_parse_float(w), 0.0));
                shader.sky_parms = Some(SkyParms {
                    farbox: sides.as_ref().map_or_else(String::new, |s| s[0].clone()),
                    cloud_height,
                    nearbox: words
                        .get(7)
                        .filter(|w| !w.is_empty() && *w != "-")
                        .cloned()
                        .unwrap_or_default(),
                    sky_height: if cloud_height > 0.0 {
                        cloud_height
                    } else {
                        DEFAULT_SKY_HEIGHT
                    },
                    sides,
                });
            }
            "qer_editorimage" => {
                if present(tokens, pos + 1) {
                    shader.editor_image = Some(tokens[pos + 1].clone());
                }
                pos += 2;
            }
            "if" => {
                let (block, next) = parse_conditional_block(tokens, pos + 1);
                shader.stages.extend(block.stages.iter().cloned());
                shader.conditional_blocks.get_or_insert_with(Vec::new).push(block);
                pos = next;
            }
            _ => {
                if token == "q3map_surfacelight" || token == "lightingintensity" {
                    if pos + 1 < tokens.len() {
                        let v = js_string_to_number(&tokens[pos + 1]);
                        if v.is_finite() && v > 0.0 {
                            shader.surface_light = Some(v);
                        }
                    }
                    pos += 1;
                    while is_parameter(tokens, pos) {
                        pos += 1;
                    }
                } else if token.starts_with("q3map") || token.starts_with("qer_") {
                    pos += 1;
                    while is_parameter(tokens, pos) {
                        pos += 1;
                    }
                } else {
                    pos += 1;
                }
            }
        }
    }
    (shader, pos)
}

/// `parseConditionalBlock`: stages up to `endif`; nested blocks' stages are
/// flattened into this one.
fn parse_conditional_block(tokens: &Tokens, start: usize) -> (Q3ShaderConditionalBlock, usize) {
    let (condition, mut pos) = parse_conditional_condition(tokens, start);
    let mut block = Q3ShaderConditionalBlock {
        condition,
        stages: Vec::new(),
    };
    while pos < tokens.len() {
        let token = tokens[pos].to_lowercase();
        if token == "endif" {
            pos += 1;
            break;
        }
        if token == "{" {
            let (stage, next) = parse_stage(tokens, pos + 1);
            block.stages.push(stage);
            pos = next;
            continue;
        }
        if token == "if" {
            let (nested, next) = parse_conditional_block(tokens, pos + 1);
            block.stages.extend(nested.stages);
            pos = next;
            continue;
        }
        pos += 1;
    }
    (block, pos)
}

/// `parseConditionalCondition`.
fn parse_conditional_condition(tokens: &Tokens, start: usize) -> (Condition, usize) {
    let first = lower(tokens, start);
    let second = lower(tokens, start + 1);
    match (first.as_deref(), second.as_deref()) {
        (Some("!"), Some("deluxe")) => (Condition::NotDeluxe, start + 2),
        (Some("!deluxe"), _) => (Condition::NotDeluxe, start + 1),
        (Some("deluxe"), _) => (Condition::Deluxe, start + 1),
        (Some("texturecubemap"), _) => (Condition::TextureCubeMap, start + 1),
        (Some("!texturecubemap"), _) => (Condition::NotTextureCubeMap, start + 1),
        (Some("!"), Some("texturecubemap")) => (Condition::NotTextureCubeMap, start + 2),
        _ => (Condition::Unsupported, start + 1),
    }
}

/// Stage identities (see [`Q3ShaderStage`]); 0 is never handed out.
static NEXT_STAGE_ID: AtomicU64 = AtomicU64::new(1);

/// `parseStage`: up to and past the stage's closing brace.
fn parse_stage(tokens: &Tokens, start: usize) -> (Q3ShaderStage, usize) {
    let mut stage = Q3ShaderStage {
        id: NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed),
        ..Q3ShaderStage::default()
    };
    let mut pos = start;
    while pos < tokens.len() && tokens[pos] != "}" {
        let token = tokens[pos].to_lowercase();
        match token.as_str() {
            "map" => {
                if present(tokens, pos + 1) {
                    stage.map.clone_from(&tokens[pos + 1]);
                }
                pos += 2;
            }
            "clampmap" => {
                if present(tokens, pos + 1) {
                    stage.map.clone_from(&tokens[pos + 1]);
                    stage.clamp_map = Some(true);
                }
                pos += 2;
            }
            "surroundmap" | "cubemap" => {
                if present(tokens, pos + 1) {
                    stage.cube_map = Some(tokens[pos + 1].clone());
                    stage.tc_gen = Some(TcGen::mode(if token == "surroundmap" {
                        TcGenMode::Surround
                    } else {
                        TcGenMode::Environment
                    }));
                }
                pos += 2;
            }
            "material" => {
                let (material, next) = parse_material_directive(tokens, pos + 1);
                if let Some(diffuse) = material.diffuse.as_ref().filter(|d| !d.is_empty()) {
                    stage.map.clone_from(diffuse);
                }
                stage.material = Some(material);
                pos = next;
            }
            "celshade" => {
                pos += 1;
                let mut args: Vec<&str> = Vec::new();
                while is_stage_parameter(tokens, pos) {
                    args.push(&tokens[pos]);
                    pos += 1;
                }
                let real = |t: Option<&&str>| t.is_some_and(|t| !t.is_empty() && *t != "-" && !t.starts_with('$'));
                stage.map = if real(args.first()) {
                    args[0].to_owned()
                } else if real(args.get(2)) {
                    args[2].to_owned()
                } else {
                    args.first().map_or_else(String::new, |a| (*a).to_owned())
                };
            }
            "animmap" | "animclampmap" => {
                if present(tokens, pos + 1) {
                    let frequency = pf(tokens, pos + 1);
                    let mut frames = Vec::new();
                    pos += 2;
                    while is_stage_parameter(tokens, pos) {
                        frames.push(tokens[pos].clone());
                        pos += 1;
                    }
                    if let Some(first) = frames.first() {
                        stage.map.clone_from(first);
                    }
                    stage.anim_map = Some(AnimMap { frequency, frames });
                    if token == "animclampmap" {
                        stage.clamp_map = Some(true);
                    }
                } else {
                    pos += 1;
                }
            }
            "blendfunc" => {
                stage.blend_func = Some(parse_blend_func(tokens, pos + 1));
                pos += 2;
                if present(tokens, pos) && is_stage_parameter(tokens, pos) {
                    pos += 1;
                }
            }
            "alphafunc" => {
                if present(tokens, pos + 1) {
                    stage.alpha_func = match tokens[pos + 1].to_uppercase().as_str() {
                        "GT0" => Some(AlphaFunc::Gt0),
                        "GE128" => Some(AlphaFunc::Ge128),
                        "LT128" => Some(AlphaFunc::Lt128),
                        _ => stage.alpha_func,
                    };
                }
                pos += 2;
            }
            "rgbgen" => {
                stage.rgb_gen = Some(parse_rgb_gen(tokens, pos + 1));
                pos += 2;
                while is_stage_parameter(tokens, pos) {
                    pos += 1;
                }
            }
            "alphagen" => {
                stage.alpha_gen = Some(parse_alpha_gen(tokens, pos + 1));
                pos += 2;
                while is_stage_parameter(tokens, pos) {
                    pos += 1;
                }
            }
            "tcmod" => {
                if let Some(m) = parse_tc_mod(tokens, pos + 1) {
                    stage.tc_mod.get_or_insert_with(Vec::new).push(m);
                }
                pos += 2;
                while is_stage_parameter(tokens, pos) {
                    pos += 1;
                }
            }
            "depthwrite" => {
                stage.depth_write = Some(true);
                pos += 1;
            }
            "depthfunc" => {
                if present(tokens, pos + 1) {
                    stage.depth_func = Some(if tokens[pos + 1].to_lowercase() == "equal" {
                        DepthFunc::Equal
                    } else {
                        DepthFunc::Lequal
                    });
                }
                pos += 2;
            }
            "detail" => {
                stage.detail = Some(true);
                pos += 1;
            }
            "tcgen" => {
                stage.tc_gen = Some(parse_tc_gen(tokens, pos + 1));
                pos += 2;
                while is_stage_parameter(tokens, pos) {
                    pos += 1;
                }
            }
            _ => pos += 1,
        }
    }
    if tok(tokens, pos) == Some("}") {
        pos += 1;
    }
    (stage, pos)
}

/// `parseMaterialDirective`.
fn parse_material_directive(tokens: &Tokens, start: usize) -> (Q3MaterialDirective, usize) {
    let mut raw = Vec::new();
    let mut pos = start;
    while is_stage_parameter(tokens, pos) {
        raw.push(tokens[pos].clone());
        pos += 1;
    }
    let material = Q3MaterialDirective {
        diffuse: raw.first().cloned(),
        normal: raw.get(1).cloned(),
        gloss: raw.get(2).cloned(),
        glow: raw.get(3).cloned(),
        extras: (raw.len() > 4).then(|| raw[4..].to_vec()),
        raw_tokens: raw,
    };
    (material, pos)
}

/// `parseBlendFunc`.
fn parse_blend_func(tokens: &Tokens, pos: usize) -> [BlendFactor; 2] {
    match lower(tokens, pos).as_deref() {
        Some("add") => [BlendFactor::One, BlendFactor::One],
        Some("blend") => [BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha],
        Some("filter") => [BlendFactor::DstColor, BlendFactor::Zero],
        _ => [
            parse_blend_factor(tok(tokens, pos)),
            parse_blend_factor(tok(tokens, pos + 1)),
        ],
    }
}

/// `parseBlendFactor`: unknown or missing is `GL_ONE`.
fn parse_blend_factor(token: Option<&str>) -> BlendFactor {
    token
        .filter(|t| !t.is_empty())
        .and_then(BlendFactor::from_gl_name)
        .unwrap_or(BlendFactor::One)
}

const SKY_SIDE_NAMES: [&str; 6] = ["rt", "bk", "lf", "ft", "up", "dn"];

/// `resolveSkySides`: the six `skyparmssides` sides as images; a side named
/// `rt`, `bk`, ... takes that side's image. `None` unless all six resolve.
#[must_use]
pub fn resolve_sky_sides(words: &[String]) -> Option<Vec<String>> {
    if words.len() < 6 {
        return None;
    }
    let refs: Vec<Option<usize>> = words
        .iter()
        .map(|w| SKY_SIDE_NAMES.iter().position(|n| *n == w.to_lowercase()))
        .collect();
    let images: Vec<&str> = words
        .iter()
        .zip(&refs)
        .map(|(w, r)| if r.is_none() { w.as_str() } else { "" })
        .collect();
    let sides: Vec<String> = images
        .iter()
        .zip(&refs)
        .map(|(image, r)| {
            if image.is_empty() {
                r.and_then(|r| images.get(r).copied()).unwrap_or("").to_owned()
            } else {
                (*image).to_owned()
            }
        })
        .collect();
    sides.iter().all(|s| !s.is_empty()).then_some(sides)
}

/// `constNumbers`: up to `count` numbers from `start`, with or without
/// brackets; stops at `)` or at the first token that is not a finite number.
fn const_numbers(tokens: &Tokens, start: usize, count: usize) -> Vec<f64> {
    let mut values = Vec::new();
    let mut i = start;
    while i < tokens.len() && values.len() < count {
        if tokens[i] == "(" {
            i += 1;
            continue;
        }
        if tokens[i] == ")" {
            break;
        }
        let v = js_parse_float(&tokens[i]);
        if !v.is_finite() {
            break;
        }
        values.push(v);
        i += 1;
    }
    values
}

fn distance_ramp(tokens: &Tokens, pos: usize) -> DistanceRamp {
    let v = const_numbers(tokens, pos, 4);
    let at = |i: usize, d: f64| v.get(i).copied().unwrap_or(d);
    DistanceRamp {
        from: at(0, 0.0),
        to: at(1, 1.0),
        near: at(2, 0.0),
        far: at(3, 1.0),
    }
}

/// `parseRgbGen`.
fn parse_rgb_gen(tokens: &Tokens, pos: usize) -> RgbGen {
    let kind = match lower(tokens, pos).as_deref() {
        Some("identitylighting") => RgbGenType::IdentityLighting,
        Some("vertex" | "exactvertex") => RgbGenType::Vertex,
        Some("oneminusvertex") => RgbGenType::OneMinusVertex,
        Some("entity") => RgbGenType::Entity,
        Some("oneminusentity") => RgbGenType::OneMinusEntity,
        Some("lightingdiffuse") => RgbGenType::LightingDiffuse,
        Some("wave") => {
            if lower(tokens, pos + 1).as_deref() == Some("distanceramp") {
                return RgbGen {
                    distance_ramp: Some(distance_ramp(tokens, pos + 2)),
                    ..RgbGen::of(RgbGenType::DistanceRamp)
                };
            }
            return RgbGen {
                wave: Some(parse_wave_func(tokens, pos + 1)),
                ..RgbGen::of(RgbGenType::Wave)
            };
        }
        Some("const") => {
            let c = const_numbers(tokens, pos + 1, 3);
            return RgbGen {
                color: Some(if c.len() == 3 {
                    [c[0], c[1], c[2]]
                } else {
                    [1.0, 1.0, 1.0]
                }),
                ..RgbGen::of(RgbGenType::Const)
            };
        }
        _ => RgbGenType::Identity,
    };
    RgbGen::of(kind)
}

/// `parseAlphaGen`.
fn parse_alpha_gen(tokens: &Tokens, pos: usize) -> AlphaGen {
    let kind = match lower(tokens, pos).as_deref() {
        Some("vertex") => AlphaGenType::Vertex,
        Some("oneminusvertex") => AlphaGenType::OneMinusVertex,
        Some("entity") => AlphaGenType::Entity,
        Some("oneminusentity") => AlphaGenType::OneMinusEntity,
        Some("lightingspecular") => AlphaGenType::LightingSpecular,
        Some("portal") => AlphaGenType::Portal,
        Some("wave") => {
            if lower(tokens, pos + 1).as_deref() == Some("distanceramp") {
                return AlphaGen {
                    distance_ramp: Some(distance_ramp(tokens, pos + 2)),
                    ..AlphaGen::of(AlphaGenType::DistanceRamp)
                };
            }
            return AlphaGen {
                wave: Some(parse_wave_func(tokens, pos + 1)),
                ..AlphaGen::of(AlphaGenType::Wave)
            };
        }
        Some("const") => {
            let v = const_numbers(tokens, pos + 1, 1);
            return AlphaGen {
                value: Some(v.first().copied().unwrap_or(1.0)),
                ..AlphaGen::of(AlphaGenType::Const)
            };
        }
        _ => AlphaGenType::Identity,
    };
    AlphaGen::of(kind)
}

/// `parseWaveFunc`.
fn parse_wave_func(tokens: &Tokens, pos: usize) -> WaveFunc {
    let func = match lower(tokens, pos).filter(|n| !n.is_empty()).as_deref().unwrap_or("sin") {
        "triangle" => WaveForm::Triangle,
        "square" => WaveForm::Square,
        "sawtooth" => WaveForm::Sawtooth,
        "inversesawtooth" => WaveForm::InverseSawtooth,
        "noise" => WaveForm::Noise,
        _ => WaveForm::Sin,
    };
    WaveFunc {
        func,
        base: or(pf(tokens, pos + 1), 0.0),
        amplitude: or(pf(tokens, pos + 2), 1.0),
        phase: or(pf(tokens, pos + 3), 0.0),
        frequency: or(pf(tokens, pos + 4), 1.0),
    }
}

/// `parseTcMod`.
fn parse_tc_mod(tokens: &Tokens, pos: usize) -> Option<TcMod> {
    let n = |k: usize, d: f64| or(pf(tokens, pos + k), d);
    Some(match lower(tokens, pos).as_deref()? {
        "scroll" => TcMod::Scroll {
            s_speed: n(1, 0.0),
            t_speed: n(2, 0.0),
        },
        "scale" => TcMod::Scale {
            s_scale: n(1, 1.0),
            t_scale: n(2, 1.0),
        },
        "rotate" => TcMod::Rotate {
            degrees_per_second: n(1, 0.0),
        },
        "stretch" => TcMod::Stretch {
            func: parse_wave_func(tokens, pos + 1),
        },
        "turb" => TcMod::Turb {
            base: n(1, 0.0),
            amplitude: n(2, 1.0),
            phase: n(3, 0.0),
            frequency: n(4, 1.0),
        },
        "transform" => TcMod::Transform {
            m00: n(1, 1.0),
            m01: n(2, 0.0),
            m10: n(3, 0.0),
            m11: n(4, 1.0),
            t0: n(5, 0.0),
            t1: n(6, 0.0),
        },
        _ => return None,
    })
}

/// `parseTcGen`.
fn parse_tc_gen(tokens: &Tokens, pos: usize) -> TcGen {
    match lower(tokens, pos).as_deref() {
        Some("lightmap") => TcGen::mode(TcGenMode::Lightmap),
        Some("environment") => TcGen::mode(TcGenMode::Environment),
        Some("vector") => {
            let mut i = pos + 1;
            let read = |i: &mut usize| {
                let v = or(pf(tokens, *i), 0.0);
                *i += 1;
                v
            };
            if tok(tokens, i) == Some("(") {
                i += 1;
            }
            let s = [read(&mut i), read(&mut i), read(&mut i)];
            if tok(tokens, i) == Some(")") {
                i += 1;
            }
            if tok(tokens, i) == Some("(") {
                i += 1;
            }
            let t = [read(&mut i), read(&mut i), read(&mut i)];
            TcGen {
                mode: TcGenMode::Vector,
                vector: Some(TcGenVector {
                    s_vector: s,
                    t_vector: t,
                }),
            }
        }
        _ => TcGen::mode(TcGenMode::Base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_follows_javascript_replacement_patterns() {
        assert_eq!(js_replace_all("a$1b$1", "$1", "x"), "axbx");
        assert_eq!(js_replace_all("a$1b", "$1", "$$"), "a$b");
        assert_eq!(js_replace_all("a$1b", "$1", "[$&]"), "a[$1]b");
        assert_eq!(js_replace_all("a$1b", "$1", "$`"), "aab");
        assert_eq!(js_replace_all("a$1b", "$1", "$'"), "abb");
        assert_eq!(js_replace_all("a$1b", "$1", "$lightmap"), "a$lightmapb");
        assert_eq!(js_replace_all("$10", "$1", "x"), "x0");
    }
}
