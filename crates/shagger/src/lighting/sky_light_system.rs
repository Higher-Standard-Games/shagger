//! SHAG `src/engine/lighting/SkyLightSystem.ts`, BSP mode: the fixed lights a
//! BSP map is lit by, as `updateLighting` sets them with the day cycle
//! paused, and the world fill light (`createWorldFillLight`).
//!
//! The BSP surfaces leave these out (their lighting is baked), so they
//! reach the models: a map pickup is lit by the fill, the sun, the moon
//! (off in BSP mode) and the ambient light, with the specular colours
//! `StandardMaterial` meshes take.

use glam::Vec3;

use crate::rendering::model_renderer::ModelLight;

/// The lights a model in a BSP map is lit by. `bsp_pbr` is the `r_bsp_pbr`
/// render variable (on by default), which brightens the sun and dims the
/// ambient light.
#[must_use]
pub fn bsp_mode_lights(bsp_pbr: bool) -> Vec<ModelLight> {
    vec![
        // createWorldFillLight
        ModelLight::Hemispheric {
            direction: Vec3::new(0.3, 1.0, 0.5),
            diffuse: Vec3::new(1.0, 0.95, 0.9),
            // A HemisphericLight's default specular.
            specular: Vec3::ONE,
            ground: Vec3::new(0.3, 0.3, 0.35),
            intensity: 1.2,
        },
        // The sun: a fixed angle, for normal map relief.
        ModelLight::Directional {
            direction: Vec3::new(-0.5, -1.0, -0.2).normalize(),
            diffuse: Vec3::ONE,
            specular: if bsp_pbr { Vec3::splat(0.16) } else { Vec3::splat(0.1) },
            intensity: if bsp_pbr { 1.15 } else { 1.0 },
        },
        // The ambient light: neutral, for lightmaps.
        ModelLight::Hemispheric {
            direction: Vec3::Y,
            diffuse: Vec3::ONE,
            specular: Vec3::ZERO,
            ground: if bsp_pbr { Vec3::splat(0.18) } else { Vec3::splat(0.5) },
            intensity: if bsp_pbr { 0.35 } else { 0.8 },
        },
    ]
}
