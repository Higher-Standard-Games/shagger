//! Port of SHAG `src/engine/rendering/WorldFogController.ts` and the uniform
//! values of `AtmosphericFogShader.ts`: the world fog to draw, from the
//! map's worldspawn fog and the `r_fog` cvars as Warfork's r_main.c decides
//! it, and the six vectors every fogged shader reads.
//!
//! The TypeScript hands the fog to Babylon's scene fog and a material
//! plugin; here the renderers take [`FogUniforms`] in their per-frame
//! uniforms and run Warfork's `fog.glsl` evaluation themselves
//! (`fog.wgsl`). Fog volumes and the underwater view, which borrow the
//! scene fog, are not ported yet.

use crate::core::cvar::{CvarOptions, CvarType, CvarValue, Cvars};
use crate::loader::bsp::world_fog::{FogHeight, FogSky, FogSun, WorldFogConfig, leading_numbers, world_fog_to_engine};
use crate::loader::shader::q3_shader_types::DEFAULT_SKY_HEIGHT;
use shagger_shared::js::js_hypot;

/// `FogCvars`.
#[derive(Debug, Clone, PartialEq)]
pub struct FogCvars {
    pub enabled: bool,
    pub distance: f64,
    pub color: String,
    pub min_dist: f64,
    pub height_clear: f64,
    pub height_full: f64,
    pub sun_dir: String,
    pub sun_color: String,
    pub sun_inscatter: f64,
    pub sky_blend: f64,
}

/// "Not set" sentinels, as Warfork's cvar defaults.
const INHERIT_HEIGHT_CLEAR: f64 = 99999.0;
const INHERIT_HEIGHT_FULL: f64 = -99999.0;
/// `r_fog_color`'s default.
const OVERRIDE_COLOR: &str = "0.75 0.75 0.75";

/// `resolveWorldFog`: the fog to draw, Quake units and axes; `None` for none.
#[must_use]
pub fn resolve_world_fog(map: Option<&WorldFogConfig>, cvars: &FogCvars, developer: bool) -> Option<WorldFogConfig> {
    if !cvars.enabled {
        return None;
    }
    if !(developer && cvars.distance > 0.0) {
        return map.copied();
    }
    let color = leading_numbers(Some(&cvars.color), 3);
    if map.is_none() && color.len() < 3 {
        return None;
    }
    let mut fog = map.copied().unwrap_or(WorldFogConfig {
        color: [0.0; 3],
        density: 0.0,
        min_dist: 0.0,
        height: None,
        sun: None,
        sky: None,
    });
    fog.density = 3.0 / cvars.distance;
    if color.len() >= 3 {
        fog.color = [color[0], color[1], color[2]];
    }
    if cvars.min_dist >= 0.0 {
        fog.min_dist = cvars.min_dist;
    }
    if cvars.height_clear < 90000.0 {
        fog.height = Some(FogHeight {
            clear: cvars.height_clear,
            full: fog.height.map_or(0.0, |h| h.full),
        });
    }
    if cvars.height_full > -90000.0 {
        fog.height = Some(FogHeight {
            clear: fog.height.map_or(0.0, |h| h.clear),
            full: cvars.height_full,
        });
    }
    let mut sun = fog.sun.unwrap_or(FogSun {
        dir: [0.577, 0.577, 0.577],
        color: [1.0, 0.9, 0.7],
        intensity: 1.5,
        exponent: 16.0,
    });
    let mut sun_changed = false;
    if cvars.sun_inscatter >= 0.0 {
        sun.intensity = cvars.sun_inscatter;
        sun_changed = true;
    }
    let dir = leading_numbers(Some(&cvars.sun_dir), 3);
    if dir.len() == 3 {
        let length = js_hypot(&dir);
        let length = if length == 0.0 || length.is_nan() { 1.0 } else { length };
        sun.dir = [dir[0] / length, dir[1] / length, dir[2] / length];
        sun_changed = true;
    }
    let sun_color = leading_numbers(Some(&cvars.sun_color), 4);
    if sun_color.len() >= 3 {
        sun.color = [sun_color[0], sun_color[1], sun_color[2]];
        sun.exponent = sun_color.get(3).copied().filter(|e| *e > 0.0).unwrap_or(16.0);
        sun_changed = true;
    }
    if sun_changed {
        fog.sun = Some(sun);
    }
    if cvars.sky_blend >= 0.0 {
        fog.sky = if cvars.sky_blend > 0.0 {
            Some(fog.sky.unwrap_or(FogSky {
                horizon_bias: 0.05,
                horizon_scale: 2.0,
                zenith_falloff: 3.0,
            }))
        } else {
            None
        };
    }
    Some(fog)
}

/// `registerCvars`: `r_fog` and the developer overrides.
pub fn register_fog_cvars(cvars: &mut Cvars) {
    cvars.register(
        "r_fog",
        true,
        CvarOptions::describe("World fog from the map (0/1)").kind(CvarType::Bool),
    );
    let dev = |d: &'static str, kind: CvarType| {
        let mut o = CvarOptions::describe(d).kind(kind);
        o.flags.persist = false;
        o
    };
    cvars.register(
        "r_fog_distance",
        0,
        dev(
            "Developer: fog ~95% opaque at this distance; above 0 the r_fog_* cvars replace the map's fog",
            CvarType::Float,
        )
        .range(0.0, f64::INFINITY),
    );
    cvars.register(
        "r_fog_color",
        OVERRIDE_COLOR,
        dev("Developer: fog colour \"r g b\" (0-1)", CvarType::String),
    );
    cvars.register(
        "r_fog_mindist",
        -1,
        dev("Developer: fog start distance; -1 keeps the map's", CvarType::Float),
    );
    cvars.register(
        "r_fog_height_clear",
        INHERIT_HEIGHT_CLEAR,
        dev(
            "Developer: height where fog is clear; 99999 keeps the map's",
            CvarType::Float,
        ),
    );
    cvars.register(
        "r_fog_height_full",
        INHERIT_HEIGHT_FULL,
        dev(
            "Developer: height where fog is full; -99999 keeps the map's",
            CvarType::Float,
        ),
    );
    cvars.register(
        "r_fog_sun_dir",
        "",
        dev("Developer: sun direction \"x y z\" (Quake axes)", CvarType::String),
    );
    cvars.register(
        "r_fog_sun_color",
        "",
        dev("Developer: sun glow \"r g b [exponent]\"", CvarType::String),
    );
    cvars.register(
        "r_fog_sun_inscatter",
        -1,
        dev("Developer: sun glow intensity; -1 keeps the map's", CvarType::Float),
    );
    cvars.register(
        "r_fog_sky_blend",
        -1,
        dev("Developer: sky horizon tint 0/1; -1 keeps the map's", CvarType::Float),
    );
}

/// The `r_fog*` cvars as they stand (`cvars()`).
#[must_use]
pub fn fog_cvars(cvars: &Cvars) -> FogCvars {
    let number = |name: &str, fallback: f64| match cvars.get(name) {
        Some(CvarValue::Number(n)) => *n,
        _ => fallback,
    };
    FogCvars {
        enabled: cvars.get("r_fog").is_none() || cvars.get_bool("r_fog"),
        distance: number("r_fog_distance", 0.0),
        color: cvars.get_string("r_fog_color"),
        min_dist: number("r_fog_mindist", -1.0),
        height_clear: number("r_fog_height_clear", INHERIT_HEIGHT_CLEAR),
        height_full: number("r_fog_height_full", INHERIT_HEIGHT_FULL),
        sun_dir: cvars.get_string("r_fog_sun_dir"),
        sun_color: cvars.get_string("r_fog_sun_color"),
        sun_inscatter: number("r_fog_sun_inscatter", -1.0),
        sky_blend: number("r_fog_sky_blend", -1.0),
    }
}

/// The six vectors of `ATM_FOG_UNIFORMS`, in order: colour (a 1 when on),
/// distances (start, sky height, -, density), height band, sky tint, sun
/// colour and exponent, sun direction and intensity.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FogUniforms {
    pub color: [f32; 4],
    pub dist: [f32; 4],
    pub height: [f32; 4],
    pub sky: [f32; 4],
    pub sun_color: [f32; 4],
    pub sun_params: [f32; 4],
}

impl Default for FogUniforms {
    /// No fog: `atmFogUniformValues(null)`.
    fn default() -> Self {
        Self {
            color: [0.0; 4],
            dist: [0.0; 4],
            height: [0.0; 4],
            sky: [0.0; 4],
            sun_color: [0.0, 0.0, 0.0, 1.0],
            sun_params: [0.0; 4],
        }
    }
}

/// The map's fog, the cvars over it, and what the shaders get.
#[derive(Debug, Clone, Default)]
pub struct WorldFogController {
    map_fog: Option<WorldFogConfig>,
    sky_height: f64,
    /// Engine axes; `None` for no fog.
    active: Option<WorldFogConfig>,
}

impl WorldFogController {
    /// `setMap`: the map's fog and its sky's height (0: the default).
    #[must_use]
    pub fn new(map_fog: Option<WorldFogConfig>, sky_height: f64) -> Self {
        Self {
            map_fog,
            sky_height: if sky_height > 0.0 {
                sky_height
            } else {
                DEFAULT_SKY_HEIGHT
            },
            active: None,
        }
    }

    /// `refresh`: the cvars applied over the map's fog.
    pub fn refresh(&mut self, cvars: &Cvars) {
        let fog = resolve_world_fog(self.map_fog.as_ref(), &fog_cvars(cvars), cvars.get_bool("developer"));
        self.active = fog.as_ref().map(world_fog_to_engine);
    }

    /// `getActive`.
    #[must_use]
    pub fn active(&self) -> Option<&WorldFogConfig> {
        self.active.as_ref()
    }

    /// `isDrawing`.
    #[must_use]
    pub fn is_drawing(&self) -> bool {
        self.active.is_some_and(|f| f.density > 0.0)
    }

    #[must_use]
    pub fn sky_height(&self) -> f64 {
        self.sky_height
    }

    /// `atmFogUniformValues`.
    #[must_use]
    pub fn uniforms(&self) -> FogUniforms {
        let Some(fog) = self.active.filter(|_| self.is_drawing()) else {
            return FogUniforms::default();
        };
        let f = |x: f64| x as f32;
        let (height, sun, sky) = (fog.height, fog.sun, fog.sky);
        FogUniforms {
            color: [f(fog.color[0]), f(fog.color[1]), f(fog.color[2]), 1.0],
            dist: [f(fog.min_dist), f(self.sky_height), 0.0, f(fog.density)],
            height: match height {
                Some(h) => [f(h.clear), f(h.full), f(1.0 / (h.full - h.clear)), 1.0],
                None => [0.0; 4],
            },
            sky: match sky {
                Some(s) => [f(s.horizon_bias), f(s.horizon_scale), f(s.zenith_falloff), 1.0],
                None => [0.0, 0.0, 1.0, 0.0],
            },
            sun_color: match sun {
                Some(s) => [f(s.color[0]), f(s.color[1]), f(s.color[2]), f(s.exponent)],
                None => [0.0, 0.0, 0.0, 1.0],
            },
            sun_params: match sun {
                Some(s) => [f(s.dir[0]), f(s.dir[1]), f(s.dir[2]), f(s.intensity)],
                None => [0.0; 4],
            },
        }
    }

    /// `atmFogFactorAt`: how much fog lies between `eye` and `position`
    /// (0 to 1), for effects that fade on the CPU.
    #[must_use]
    pub fn factor_at(&self, position: [f64; 3], eye: [f64; 3]) -> f64 {
        let Some(fog) = self.active.filter(|_| self.is_drawing()) else {
            return 0.0;
        };
        let dist = js_hypot(&[position[0] - eye[0], position[1] - eye[1], position[2] - eye[2]]);
        let mut factor = 1.0 - (-(dist - fog.min_dist).max(0.0) * fog.density).exp();
        if let Some(h) = fog.height {
            factor *= ((position[1] - h.clear) / (h.full - h.clear)).clamp(0.0, 1.0);
        }
        factor.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cvars() -> Cvars {
        let mut c = Cvars::default();
        c.register("developer", 0, CvarOptions::default());
        register_fog_cvars(&mut c);
        c
    }

    #[test]
    fn resolves_like_warfork() {
        let map = WorldFogConfig {
            color: [0.5, 0.5, 0.5],
            density: 0.002,
            min_dist: 100.0,
            height: None,
            sun: None,
            sky: None,
        };
        let mut c = cvars();
        let mut fog = WorldFogController::new(Some(map), 0.0);
        fog.refresh(&c);
        assert_eq!(fog.active(), Some(&map));
        assert_eq!(fog.uniforms().dist, [100.0, 2048.0, 0.0, 0.002]);
        // The overrides need developer 1.
        assert!(c.set("r_fog_distance", "1000"));
        fog.refresh(&c);
        assert_eq!(fog.active().unwrap().density, 0.002);
        assert!(c.set("developer", "1"));
        fog.refresh(&c);
        assert_eq!(fog.active().unwrap().density, 3.0 / 1000.0);
        assert_eq!(fog.active().unwrap().color, [0.75, 0.75, 0.75]);
        assert!(c.set("r_fog", "0"));
        fog.refresh(&c);
        assert!(!fog.is_drawing());
        assert_eq!(fog.uniforms(), FogUniforms::default());
    }

    #[test]
    fn factor_follows_distance_and_height() {
        let mut fog = WorldFogController::new(
            Some(WorldFogConfig {
                color: [1.0; 3],
                density: 3.0 / 1000.0,
                min_dist: 0.0,
                height: Some(FogHeight {
                    clear: 100.0,
                    full: 0.0,
                }),
                sun: None,
                sky: None,
            }),
            0.0,
        );
        fog.refresh(&cvars());
        let at = |y: f64| fog.factor_at([1000.0, y, 0.0], [0.0, y, 0.0]);
        assert!((at(0.0) - (1.0 - (-3.0f64).exp())).abs() < 1e-12);
        assert_eq!(at(200.0), 0.0);
    }
}
