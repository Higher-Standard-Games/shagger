// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// packages/dev/core/src/Materials/standardMaterial.ts (the uniforms
// StandardMaterial binds: vDiffuseColor, vSpecularColor, vEmissiveColor,
// vAmbientColor as ambientColor times the scene's) and fresnelParameters.ts
// (how emissive and opacity Fresnel parameters are packed: emissive left
// colour with power, right colour with bias; opacity as the left and right
// colours' luminance, bias and power).
//
// Changes: a plain struct of the properties SHAG's procedural pickups set,
// packed into one uniform block for `standard.wgsl`.

//! A Babylon `StandardMaterial`'s properties and their uniform block.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// `FresnelParameters`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FresnelParameters {
    pub left_color: Vec3,
    pub right_color: Vec3,
    pub bias: f32,
    pub power: f32,
}

/// How a material uses its one texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextureUse {
    /// `diffuseTexture`.
    pub diffuse: bool,
    /// `useAlphaFromDiffuseTexture`.
    pub alpha_from_diffuse: bool,
    /// `emissiveTexture`.
    pub emissive: bool,
    /// `opacityTexture` with `getAlphaFromRGB`.
    pub opacity_rgb: bool,
}

/// A `StandardMaterial`'s colours, lighting and blending.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StandardMaterialParams {
    pub diffuse_color: Vec3,
    pub specular_color: Vec3,
    pub specular_power: f32,
    pub emissive_color: Vec3,
    /// `ambientColor` times the scene's `ambientColor`.
    pub ambient_color: Vec3,
    pub alpha: f32,
    pub disable_lighting: bool,
    pub back_face_culling: bool,
    /// `transparencyMode` alpha blend (or alpha below 1).
    pub alpha_blend: bool,
    pub disable_depth_write: bool,
    /// `separateCullingPass`: a two-sided blended mesh draws its back faces,
    /// then its front faces.
    pub separate_culling_pass: bool,
    pub emissive_fresnel: Option<FresnelParameters>,
    pub opacity_fresnel: Option<FresnelParameters>,
    pub texture: TextureUse,
}

impl Default for StandardMaterialParams {
    /// `new StandardMaterial()`: white diffuse and specular, power 64.
    fn default() -> Self {
        Self {
            diffuse_color: Vec3::ONE,
            specular_color: Vec3::ONE,
            specular_power: 64.0,
            emissive_color: Vec3::ZERO,
            ambient_color: Vec3::ZERO,
            alpha: 1.0,
            disable_lighting: false,
            back_face_culling: true,
            alpha_blend: false,
            disable_depth_write: false,
            separate_culling_pass: false,
            emissive_fresnel: None,
            opacity_fresnel: None,
            texture: TextureUse::default(),
        }
    }
}

/// `Color3.toLuminance`.
fn luminance(c: Vec3) -> f32 {
    c.x * 0.3 + c.y * 0.59 + c.z * 0.11
}

/// The uniform block `standard.wgsl` reads.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct StandardUniform {
    diffuse: [f32; 4],
    specular: [f32; 4],
    emissive: [f32; 4],
    ambient: [f32; 4],
    params: [f32; 4],
    emissive_left: [f32; 4],
    emissive_right: [f32; 4],
    opacity_parts: [f32; 4],
    fresnel: [f32; 4],
}

impl StandardMaterialParams {
    /// Whether it blends: alpha blending or a Fresnel-driven alpha.
    #[must_use]
    pub fn blends(&self) -> bool {
        self.alpha_blend || self.alpha < 1.0 || self.texture.opacity_rgb || self.opacity_fresnel.is_some()
    }

    #[must_use]
    pub fn uniform(&self) -> StandardUniform {
        let t = self.texture;
        let uses = u32::from(t.diffuse)
            | u32::from(t.emissive) << 1
            | u32::from(t.opacity_rgb) << 2
            | u32::from(t.alpha_from_diffuse) << 3;
        let e = self.emissive_fresnel.unwrap_or(FresnelParameters {
            left_color: Vec3::ONE,
            right_color: Vec3::ZERO,
            bias: 0.0,
            power: 1.0,
        });
        let o = self.opacity_fresnel.unwrap_or(FresnelParameters {
            left_color: Vec3::ONE,
            right_color: Vec3::ZERO,
            bias: 0.0,
            power: 1.0,
        });
        StandardUniform {
            diffuse: self.diffuse_color.extend(self.alpha).into(),
            specular: self.specular_color.extend(self.specular_power).into(),
            emissive: self.emissive_color.extend(0.0).into(),
            ambient: self.ambient_color.extend(0.0).into(),
            params: [
                self.alpha,
                if self.disable_lighting { 0.0 } else { 1.0 },
                uses as f32,
                0.0,
            ],
            emissive_left: e.left_color.extend(e.power).into(),
            emissive_right: e.right_color.extend(e.bias).into(),
            opacity_parts: [luminance(o.left_color), luminance(o.right_color), o.bias, o.power],
            fresnel: [
                if self.emissive_fresnel.is_some() { 1.0 } else { 0.0 },
                if self.opacity_fresnel.is_some() { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ],
        }
    }
}
