//! SHAG `src/engine/loader/bsp/BspLightingProfile.ts`: qfusion's overbright
//! settings for a map, from its worldspawn keys.

use super::bsp_loader::BspFile;
use shagger_shared::js::{is_js_whitespace, js_string_to_number, js_trim};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspLightingProfile {
    pub gain: f64,
    pub ambient: [f64; 3],
    pub deluxe: bool,
}

/// `scaleBspLighting`.
#[must_use]
pub fn scale_bsp_lighting(profile: BspLightingProfile, gain: f64) -> BspLightingProfile {
    BspLightingProfile {
        gain,
        ambient: profile.ambient.map(|channel| (channel * gain) / profile.gain),
        ..profile
    }
}

/// `bspLightingProfile`.
#[must_use]
pub fn bsp_lighting_profile(bsp: &BspFile) -> BspLightingProfile {
    let world = bsp
        .common
        .entities
        .iter()
        .find(|e| e.classname.to_lowercase() == "worldspawn");
    // `new Map(...)` over lower-cased keys: a later duplicate wins.
    let key = |name: &str| -> Option<&str> {
        world?
            .properties
            .iter()
            .filter(|(k, _)| k.to_lowercase() == name)
            .last()
            .map(|(_, v)| v)
    };
    let positive = |value: Option<&str>, fallback: f64| {
        let n = value.map_or(f64::NAN, js_string_to_number);
        if n.is_finite() && n > 0.0 { n } else { fallback }
    };
    let gain = positive(key("_lightingintensity"), 2.0);
    let ambient = positive(key("_ambient").or_else(|| key("ambient")), 0.0);
    let text = key("_color").or_else(|| key("color")).unwrap_or("1 1 1");
    // `.trim().split(/\s+/).map(Number)`: runs of white space split once, and
    // an empty string gives [""], which is 0.
    let trimmed = js_trim(text);
    let mut color: Vec<f64> = if trimmed.is_empty() {
        vec![0.0]
    } else {
        trimmed
            .split(is_js_whitespace)
            .filter(|p| !p.is_empty())
            .map(js_string_to_number)
            .collect()
    };
    if color.len() != 3 || color.iter().any(|n| !n.is_finite() || *n < 0.0) || color.iter().all(|n| *n == 0.0) {
        color = vec![1.0, 1.0, 1.0];
    }
    let used: Vec<i32> = bsp
        .common
        .faces
        .iter()
        .map(|f| f.lightmap_index)
        .filter(|&i| i >= 0)
        .collect();
    BspLightingProfile {
        gain,
        ambient: [0, 1, 2].map(|i| (color[i] * ambient * gain) / 255.0),
        deluxe: bsp.common.header.is_fbsp
            && !used.is_empty()
            && used
                .iter()
                .all(|&i| i % 2 == 0 && (i as usize) + 1 < bsp.lightmaps.len()),
    }
}
