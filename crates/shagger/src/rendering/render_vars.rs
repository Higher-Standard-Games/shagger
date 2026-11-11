//! SHAG `src/engine/rendering/RenderVars.ts`: the `r_*` renderer variables,
//! their defaults, ranges and words, registered as cvars. What each one does
//! to the picture lands with the renderer work that reads it (milestone 11);
//! the settings screen and the console already read and write them.
//!
//! `register_render_vars` also registers the cvars SHAG's
//! `PostProcessingManager.ts` owns: `r_cel_outlines` and the colour-blindness
//! correction (`r_colorcorrection`, its strength, `r_colorsimulation`).

use crate::core::cvar::{CvarOptions, CvarType, Cvars};

/// `RenderVar.category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderVarCategory {
    Material,
    Lighting,
    Display,
    Texture,
    Debug,
}

/// `RenderVar`, without its live value (the cvar holds it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderVarDef {
    pub name: &'static str,
    pub default_value: f64,
    pub min: f64,
    pub max: f64,
    pub description: &'static str,
    pub category: RenderVarCategory,
}

/// `DEFAULT_VARS`.
pub const DEFAULT_VARS: [RenderVarDef; 60] = [
    RenderVarDef {
        name: "r_deluxe",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Use paired FBSP light directions and deluxe shader branches at next map load (0/1)",
        category: RenderVarCategory::Material,
    },
    RenderVarDef {
        name: "r_bsp_pbr",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Use generated PBR materials for rich BSP companion-map surfaces on next map/material build (0/1)",
        category: RenderVarCategory::Material,
    },
    RenderVarDef {
        name: "r_normalmap",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Normal/bump map intensity multiplier",
        category: RenderVarCategory::Material,
    },
    RenderVarDef {
        name: "r_glossmap",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Specular/gloss map intensity multiplier",
        category: RenderVarCategory::Material,
    },
    RenderVarDef {
        name: "r_lightmap",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Lightmap intensity multiplier",
        category: RenderVarCategory::Material,
    },
    RenderVarDef {
        name: "r_ambient",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Ambient light intensity multiplier",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_emissive",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Emissive/glow intensity multiplier",
        category: RenderVarCategory::Material,
    },
    RenderVarDef {
        name: "r_anisotropy",
        default_value: 4.0,
        min: 1.0,
        max: 16.0,
        description: "Anisotropic filtering level (1-16)",
        category: RenderVarCategory::Texture,
    },
    RenderVarDef {
        name: "r_picmip",
        default_value: 0.0,
        min: 0.0,
        max: 8.0,
        description: "Map surface texture reduction (next map): 0 = full, 1 = half size, 8 = flat color.",
        category: RenderVarCategory::Texture,
    },
    RenderVarDef {
        name: "r_ktx2",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Prefer generated KTX2 texture sidecars when available (0/1). Applies on next texture load.",
        category: RenderVarCategory::Texture,
    },
    RenderVarDef {
        name: "r_webgpu",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Experimental WebGPU renderer (restart required). Use ?renderer=webgpu while the compatibility gate is open.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_bsp_freeze",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Freeze world matrices + bounding info on static BSP world meshes (skips per-frame transform/bounds recompute). Applies on next map load (0/1).",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_bsp_cellsize",
        default_value: 0.0,
        min: 0.0,
        max: 4096.0,
        description: "Experimental opaque BSP spatial grouping on next map load: 0 off; positive sizes clamp to 128-4096 map units. Try 1024, compare with PVS on.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_godrays",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Master gate for volumetric god rays (0 disables; maps opt in via sun entity / worldspawn) (0/1)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_godrays_exposure",
        default_value: 0.32,
        min: 0.0,
        max: 2.0,
        description: "God-ray overall intensity (live-tunes the active pass)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_godrays_decay",
        default_value: 0.965,
        min: 0.5,
        max: 1.0,
        description: "God-ray per-sample falloff (live-tunes the active pass)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_godrays_weight",
        default_value: 0.5,
        min: 0.0,
        max: 2.0,
        description: "God-ray per-sample weight (live-tunes the active pass)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_godrays_density",
        default_value: 0.92,
        min: 0.0,
        max: 1.0,
        description: "God-ray sample density (live-tunes the active pass)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_godrays_samples",
        default_value: 60.0,
        min: 8.0,
        max: 200.0,
        description: "God-ray radial sample count / quality (applies on next activation)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_arealights",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Enable textured area lights from map `light_area` entities (0/1). Applies on next map load.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_arealights_max",
        default_value: 4.0,
        min: 0.0,
        max: 32.0,
        description: "Max area lights per map (they consume real per-surface light slots). Applies on next map load.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_arealights_auto",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Auto-place area lights on the biggest emissive (fullbright) BSP surfaces, filling the r_arealights_max budget after explicit light_area entities. Experimental (0/1). Applies on next map load.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_arealights_intensity",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Brightness multiplier for all area lights. Applies on next map load.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_wireframe",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Wireframe mode toggle (0/1)",
        category: RenderVarCategory::Debug,
    },
    RenderVarDef {
        name: "r_ssr",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Screen-space reflections on glossy surfaces (0/1). GPU-expensive; off by default.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_ssr_strength",
        default_value: 0.4,
        min: 0.0,
        max: 2.0,
        description: "SSR reflection strength (subtle by default). Tunes live when r_ssr is on.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_softhalos",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Soft halos (0/1): light halos and flares fade where they meet walls instead of ending in a hard line. Costs a depth pass of the meshes near them.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_glow",
        default_value: 0.0,
        min: 0.0,
        max: 2.0,
        description: "Emissive glow-layer intensity (0 = off, default). Glows signs/strips/energy on top of bloom.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_bloom",
        default_value: 0.0,
        min: 0.0,
        max: 3.0,
        description: "Bloom intensity multiplier (applies to post-processing pipeline)",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_exposure",
        default_value: 1.0,
        min: 0.0,
        max: 3.0,
        description: "Exposure multiplier (applies to post-processing pipeline)",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_contrast",
        default_value: 1.0,
        min: 0.0,
        max: 3.0,
        description: "Contrast multiplier (applies to post-processing pipeline)",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_lightintensity",
        default_value: 1.0,
        min: 0.0,
        max: 4.0,
        description: "Global light intensity multiplier",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_lightmapscale",
        default_value: 1.0,
        min: 0.1,
        max: 3.0,
        description: "Scales baked lightmap gain on top of the per-format default (takes effect on map load)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Draw liquid surfaces with the unified water material. 0 falls back to the legacy Q3/Warsow shader translation (applies on next map load).",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_quality",
        default_value: 2.0,
        min: 1.0,
        max: 3.0,
        description: "Water quality. All tiers use cached room reflections and calm reactive waves. 1 = no capture pass; 2 = depth tint; 3 = depth plus half-resolution refraction. Splash detail initializes on map load. No recurring reflection capture.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_debug",
        default_value: 0.0,
        min: 0.0,
        max: 6.0,
        description: "Water debug: 1 = absorption, 2 = thickness, 3 = environment, 4 = cached room, 5 = reactive slopes, 6 = undistorted refraction capture (red = unavailable). 0 = normal rendering.",
        category: RenderVarCategory::Debug,
    },
    RenderVarDef {
        name: "r_water_highlights",
        default_value: 0.6,
        min: 0.0,
        max: 2.0,
        description: "Water highlights from authored static map lights; 0 disables. No weapon/projectile lights.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_roughness",
        default_value: 0.3,
        min: 0.12,
        max: 0.8,
        description: "Water point-highlight roughness: higher gives broader, softer highlights (not cubemap blur).",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_reflection",
        default_value: 1.3,
        min: 0.0,
        max: 2.0,
        description: "Water cached reflection weighting. 1 = previous baseline; 1.3 = default; 0 disables reflected contribution.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_probe_height",
        default_value: 64.0,
        min: 0.0,
        max: 128.0,
        description: "Static water probe lift above its waterline anchor, clamped below solid geometry. Map reload required; 0 restores the low probe.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_blur",
        default_value: 0.3,
        min: 0.0,
        max: 1.0,
        description: "Cached water reflection softness. 0 = single sharp sample; higher = wider five-tap angular filter. Updates live.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_detail",
        default_value: 0.8,
        min: 0.0,
        max: 1.0,
        description: "Fine water wave strength, faded at distance/grazing angles. 0 leaves broad waves and player ripples. Updates live.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_contact",
        default_value: 0.25,
        min: 0.0,
        max: 1.0,
        description: "Subtle neutral bank-contact shade from depth (quality 2/3, above water). No coloured dots or extra opacity. 0 disables. Updates live.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_water_actors",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Experimental near-water actor reflection overlay (quality 3). One small live pass, two nearby actors; 0 disables. Updates live.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_underwater_distortion",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Underwater view-warp strength. At 0 the motion stops entirely and tint, fog and audio still say you are underwater — nothing is communicated by movement alone.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_dynamiclight",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Dynamic lights (0/1): projectile, explosion, impact, charge and muzzle-flash lights.",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_marks",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Wall marks (0/1): bullet holes and scorches where shots hit.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_gibs",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Gibs (0/1): the shards a player breaks into on death.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_teleport_ghost",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Teleport afterimage (0/1): the silhouette left where a player teleported from.",
        category: RenderVarCategory::Display,
    },
    RenderVarDef {
        name: "r_showtris",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Force wireframe on entire scene (0/1)",
        category: RenderVarCategory::Debug,
    },
    RenderVarDef {
        name: "r_daynight",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Enable day/night cycle for BSP maps (0/1)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_time",
        default_value: 0.5,
        min: 0.0,
        max: 1.0,
        description: "Time of day (0=midnight, 0.25=sunrise, 0.5=noon, 0.75=sunset)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_animate",
        default_value: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Animate time progression (0/1)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_speed",
        default_value: 1.0,
        min: 0.1,
        max: 10.0,
        description: "Animation speed multiplier",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_length",
        default_value: 1440.0,
        min: 60.0,
        max: 86400.0,
        description: "Full day cycle length in seconds (default 24 min)",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_lm_min",
        default_value: 0.05,
        min: 0.0,
        max: 1.0,
        description: "Minimum lightmap intensity at night",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_lm_max",
        default_value: 1.0,
        min: 0.0,
        max: 2.0,
        description: "Maximum lightmap intensity at noon",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_sun_intensity",
        default_value: 0.3,
        min: 0.0,
        max: 2.0,
        description: "Sun light intensity on BSP geometry",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_moon_intensity",
        default_value: 0.15,
        min: 0.0,
        max: 1.0,
        description: "Moon light intensity at night",
        category: RenderVarCategory::Lighting,
    },
    RenderVarDef {
        name: "r_daynight_sky_tint",
        default_value: 1.0,
        min: 0.0,
        max: 1.0,
        description: "Enable skybox tinting for day/night (0/1)",
        category: RenderVarCategory::Lighting,
    },
];

/// `registerCvars`: an int for `r_anisotropy`, `r_picmip` and the whole
/// 0..1 switches, a float otherwise; debug vars are not saved.
pub fn register_render_vars(cvars: &mut Cvars) {
    for def in &DEFAULT_VARS {
        let fractional_water = matches!(def.name, "r_water_blur" | "r_water_detail" | "r_water_contact");
        let fractional = fractional_water || def.default_value.fract() != 0.0;
        let is_int = matches!(def.name, "r_anisotropy" | "r_picmip") || (def.max == 1.0 && !fractional);
        let mut options = CvarOptions::describe(def.description)
            .range(def.min, def.max)
            .kind(if is_int { CvarType::Int } else { CvarType::Float });
        options.flags.persist = def.category != RenderVarCategory::Debug;
        cvars.register(def.name, def.default_value, options);
    }
    // PostProcessingManager.ts.
    let int = |d: &str, max: f64| CvarOptions::describe(d).range(0.0, max).kind(CvarType::Int);
    cvars.register(
        "r_colorcorrection",
        0,
        int(
            "Colour-blindness correction: 0 off, 1 protanopia, 2 deuteranopia, 3 tritanopia",
            3.0,
        ),
    );
    cvars.register(
        "r_colorcorrection_strength",
        1,
        CvarOptions::describe("How strongly r_colorcorrection shifts colours (0-1)")
            .range(0.0, 1.0)
            .kind(CvarType::Float),
    );
    let mut simulation = int(
        "Show the screen as colour-blind eyes see it, for checking maps: 1 protanopia, 2 deuteranopia, 3 tritanopia",
        3.0,
    );
    simulation.flags.persist = false;
    cvars.register("r_colorsimulation", 0, simulation);
    cvars.register(
        "r_cel_outlines",
        0,
        CvarOptions::describe("Enable cel-shading outlines (0/1)")
            .range(0.0, 1.0)
            .kind(CvarType::Int),
    );
}

/// `RenderVars.get`: a var's value, its default when unregistered.
#[must_use]
pub fn render_var(cvars: &Cvars, name: &str) -> f64 {
    if cvars.exists(name) {
        cvars.get_number(name)
    } else {
        DEFAULT_VARS
            .iter()
            .find(|d| d.name == name)
            .map_or(0.0, |d| d.default_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vars_register_with_their_types() {
        let mut cvars = Cvars::new(None, false);
        register_render_vars(&mut cvars);
        assert_eq!(render_var(&cvars, "r_anisotropy"), 4.0);
        assert_eq!(cvars.get_cvar("r_anisotropy").unwrap().kind, CvarType::Int);
        assert_eq!(cvars.get_cvar("r_bloom").unwrap().kind, CvarType::Float);
        assert_eq!(cvars.get_cvar("r_godrays").unwrap().kind, CvarType::Int);
        cvars.set("r_glow", 0.35);
        assert_eq!(render_var(&cvars, "r_glow"), 0.35);
        assert!(cvars.exists("r_cel_outlines"));
    }
}
