//! Port of SHAG `src/engine/rendering/water/UnifiedWaterMaterial.ts`: one
//! water material for every liquid surface in every map. A surface the
//! classifier calls liquid stops being translated from its Q3 shader and is
//! drawn by this instead (`unified_water.wgsl`), with per-liquid tuning:
//! slime and lava are this table with different numbers.

use bytemuck::{Pod, Zeroable};

use crate::loader::bsp::liquid_classifier::LiquidKind;
use crate::rendering::world_fog_controller::FogUniforms;

/// `WaterKind`.
pub type WaterKind = LiquidKind;

/// `MAX_WATER_LIGHTS` (`WaterSurfaceLighting.ts`).
pub const MAX_WATER_LIGHTS: usize = 4;
/// `MAX_WATER_IMPULSES` (`WaterDisturbances.ts`).
pub const MAX_WATER_IMPULSES: usize = 12;

/// The shader: the fog evaluation, then the water.
pub const UNIFIED_WATER_WGSL: &str = concat!(include_str!("../fog.wgsl"), include_str!("unified_water.wgsl"));

/// `WaterProfile`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterProfile {
    /// Colour where the liquid is vanishingly thin.
    pub shallow: [f32; 3],
    /// Colour where it is thick enough to hide its own floor.
    pub deep: [f32; 3],
    /// Flat-colour environment fallback when no usable cubemap exists.
    pub sky: [f32; 3],
    pub alpha: f32,
    pub min_alpha: f32,
    pub foam: f32,
    pub foam_width: f32,
    /// Absorption per world unit of thickness.
    pub absorb: f32,
    /// UV per world unit.
    pub scale: f32,
    /// World units per second.
    pub wave_speed: f32,
    pub wave_strength: f32,
    pub reflectivity: f32,
    pub emissive: f32,
    pub specular: f32,
    pub caustics: f32,
    pub absorb_tint: [f32; 3],
}

/// `WATER_PROFILES`.
#[must_use]
pub fn water_profile(kind: WaterKind) -> WaterProfile {
    match kind {
        WaterKind::Water => WaterProfile {
            shallow: [0.13, 0.42, 0.47],
            deep: [0.02, 0.12, 0.19],
            sky: [0.3, 0.4, 0.48],
            alpha: 0.8,
            min_alpha: 0.06,
            foam: 0.45,
            foam_width: 28.0,
            absorb: 0.008,
            scale: 0.011,
            wave_speed: 2.0,
            wave_strength: 0.6,
            reflectivity: 0.22,
            emissive: 0.0,
            specular: 0.3,
            caustics: 0.5,
            absorb_tint: [1.0, 0.42, 0.28],
        },
        WaterKind::Slime => WaterProfile {
            shallow: [0.29, 0.72, 0.1],
            deep: [0.06, 0.22, 0.03],
            sky: [0.42, 0.68, 0.22],
            alpha: 0.97,
            min_alpha: 0.5,
            foam: 0.4,
            foam_width: 14.0,
            absorb: 0.03,
            scale: 0.008,
            wave_speed: 8.0,
            wave_strength: 0.6,
            reflectivity: 0.35,
            emissive: 0.22,
            specular: 0.5,
            caustics: 0.0,
            absorb_tint: [1.0, 0.35, 1.4],
        },
        WaterKind::Lava => WaterProfile {
            shallow: [1.0, 0.46, 0.06],
            deep: [0.5, 0.08, 0.0],
            sky: [1.0, 0.72, 0.25],
            alpha: 1.0,
            min_alpha: 1.0,
            foam: 0.0,
            foam_width: 0.0,
            absorb: 0.5,
            scale: 0.006,
            wave_speed: 6.0,
            wave_strength: 0.3,
            reflectivity: 0.25,
            emissive: 0.85,
            specular: 0.15,
            caustics: 0.0,
            absorb_tint: [1.0, 1.0, 1.0],
        },
    }
}

/// `scrollRateFor`: the profile's world-unit speed as a UV rate.
#[must_use]
pub fn scroll_rate_for(profile: &WaterProfile) -> f32 {
    profile.wave_speed * profile.scale
}

/// `unified_water.wgsl`'s `Water`: every uniform the TypeScript sets.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct WaterUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    /// `uCameraPosition`, `uTime`.
    pub camera: [f32; 4],
    /// `uCameraForward`, `uCameraFar`.
    pub forward: [f32; 4],
    /// `uScreenSize`, `uDebug`, `uOutputLinear`.
    pub screen: [f32; 4],
    /// `uShallowColor`, `uAlpha`.
    pub shallow: [f32; 4],
    /// `uDeepColor`, `uMinAlpha`.
    pub deep: [f32; 4],
    /// `uSkyColor`, `uEmissive`.
    pub sky: [f32; 4],
    /// `uSunDirection`, `uSpecular`.
    pub sun: [f32; 4],
    /// `uAbsorbTint`, `uAbsorb`.
    pub absorb: [f32; 4],
    /// `uScale`, `uScroll`, `uWaveStrength`, `uReflectivity`.
    pub wave: [f32; 4],
    /// `uHasDepth`, `uHasRefraction`, `uRefractionIsLinear`, `uCalmWater`.
    pub flags: [f32; 4],
    /// `uHasEnvironment`, `uLocalProbe`, `uEnvironmentYSign`, `uEnvironmentIsLinear`.
    pub env: [f32; 4],
    /// `uFoam`, `uFoamWidth`, `uCaustics`, `uHighlightStrength`.
    pub foam: [f32; 4],
    /// `uRoughness`, `uReflectionStrength`, `uReflectionBlur`, `uDetailStrength`.
    pub tuning: [f32; 4],
    /// `uContactStrength`, `uHasActorReflection`, `uActorReflectionLinear`.
    pub actor_flags: [f32; 4],
    pub depth_projection: [f32; 4],
    pub probe_position: [f32; 4],
    pub probe_min: [f32; 4],
    pub probe_max: [f32; 4],
    pub light_mask_min: [f32; 4],
    pub light_mask_max: [f32; 4],
    pub actor_matrix: [[f32; 4]; 4],
    pub actor_texel: [f32; 4],
    pub lights: [[f32; 4]; MAX_WATER_LIGHTS],
    pub light_colours: [[f32; 4]; MAX_WATER_LIGHTS],
    pub impulses: [[f32; 4]; MAX_WATER_IMPULSES * 2],
    pub fog: FogUniforms,
}

/// `createUnifiedWaterMaterial`'s settings for one liquid and profile
/// ("clear" and "deep" retune water).
#[must_use]
pub fn unified_water_uniforms(kind: WaterKind, profile_name: &str, sun_direction: Option<[f32; 3]>) -> WaterUniforms {
    let p = water_profile(kind);
    let water = kind == WaterKind::Water;
    let sun = sun_direction.unwrap_or([0.4, 0.8, 0.3]);
    let length = (sun[0] * sun[0] + sun[1] * sun[1] + sun[2] * sun[2]).sqrt();
    let sun = sun.map(|c| if length > 0.0 { c / length } else { c });
    let (mut absorb, mut alpha) = (p.absorb, p.alpha);
    if water && profile_name == "clear" {
        (absorb, alpha) = (0.0045, 0.65);
    } else if water && profile_name == "deep" {
        (absorb, alpha) = (0.035, 0.95);
    }
    let mut u = WaterUniforms::zeroed();
    u.forward = [0.0, 0.0, 1.0, 10000.0];
    u.screen = [1920.0, 1080.0, 0.0, 0.0];
    u.shallow = [p.shallow[0], p.shallow[1], p.shallow[2], alpha];
    u.deep = [p.deep[0], p.deep[1], p.deep[2], p.min_alpha];
    u.sky = [p.sky[0], p.sky[1], p.sky[2], p.emissive];
    u.sun = [sun[0], sun[1], sun[2], if water { 0.0 } else { p.specular }];
    u.absorb = [p.absorb_tint[0], p.absorb_tint[1], p.absorb_tint[2], absorb];
    u.wave = [p.scale, scroll_rate_for(&p), p.wave_strength, p.reflectivity];
    u.flags = [0.0, 0.0, 0.0, if water { 1.0 } else { 0.0 }];
    u.env = [0.0, 0.0, 1.0, 0.0];
    u.foam = [if water { 0.0 } else { p.foam }, p.foam_width, 0.0, 0.6];
    u.tuning = [0.3, 1.3, 0.3, 0.8];
    u.actor_flags = [0.25, 0.0, 0.0, 0.0];
    u.depth_projection = [1.0, 0.0, 0.0, 0.0];
    u.light_mask_max = [1.0, 1.0, 1.0, 0.0];
    u.fog = FogUniforms::default();
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_move_visibly() {
        // About 2 world units a second for water, as the TypeScript's test asks.
        let water = water_profile(WaterKind::Water);
        assert!((scroll_rate_for(&water) / water.scale - 2.0).abs() < 1e-6);
        let u = unified_water_uniforms(WaterKind::Water, "deep", None);
        assert_eq!((u.absorb[3], u.shallow[3]), (0.035, 0.95));
        assert_eq!(u.flags[3], 1.0);
        let lava = unified_water_uniforms(WaterKind::Lava, "default", None);
        assert_eq!((lava.flags[3], lava.sun[3], lava.foam[0]), (0.0, 0.15, 0.0));
        assert_eq!(std::mem::size_of::<WaterUniforms>() % 16, 0);
    }
}
