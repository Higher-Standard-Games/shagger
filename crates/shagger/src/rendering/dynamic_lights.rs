//! The scene's dynamic point lights (SHAG's `PointLightPool` lights in
//! `DynamicLightCluster`), as Babylon's `ClusteredLightContainer` hands
//! them to materials: position and range, diffuse and specular already
//! scaled by intensity. The browser culls them by screen tile and depth
//! slice; every light still stops at its range for the standard
//! materials, so here each surface simply loops over all of them.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// Lights a frame passes to the shaders (SHAG's pool holds 8).
pub const MAX_DYNAMIC_LIGHTS: usize = 16;

/// The `ClusteredLightContainer`'s cap on a light's range.
pub const CLUSTER_MAX_RANGE: f32 = 16383.0;

/// One point light, in Babylon space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicLight {
    pub position: Vec3,
    /// `diffuse × getScaledIntensity()` (a point light's photometric scale
    /// is 1).
    pub diffuse: Vec3,
    pub specular: Vec3,
    pub range: f32,
}

impl DynamicLight {
    /// A pooled light: `diffuse` colour, `specular` = 0.3 × colour (as
    /// `PointLightPool.acquire` sets it), `intensity` and `range`.
    #[must_use]
    pub fn pooled(position: Vec3, color: Vec3, intensity: f32, range: f32) -> Self {
        Self {
            position,
            diffuse: color * intensity,
            specular: color * 0.3 * intensity,
            range,
        }
    }
}

/// The uniform block both world and model shaders declare.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DynamicLightBlock {
    /// x: how many lights are live.
    pub count: [u32; 4],
    /// Per light: position and range, diffuse, specular.
    pub lights: [[f32; 4]; 3 * MAX_DYNAMIC_LIGHTS],
}

impl Default for DynamicLightBlock {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl DynamicLightBlock {
    /// Packs up to [`MAX_DYNAMIC_LIGHTS`] lights.
    #[must_use]
    pub fn pack(lights: &[DynamicLight]) -> Self {
        let mut block = Self::zeroed();
        let live = &lights[..lights.len().min(MAX_DYNAMIC_LIGHTS)];
        block.count[0] = live.len() as u32;
        for (i, light) in live.iter().enumerate() {
            let p = light.position;
            block.lights[3 * i] = [p.x, p.y, p.z, light.range.min(CLUSTER_MAX_RANGE)];
            block.lights[3 * i + 1] = light.diffuse.extend(0.0).into();
            block.lights[3 * i + 2] = light.specular.extend(0.0).into();
        }
        block
    }
}
