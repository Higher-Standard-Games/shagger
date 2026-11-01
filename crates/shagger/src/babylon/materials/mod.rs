// SPDX-License-Identifier: Apache-2.0
// Ported from Babylon.js (packages/dev/core/src/Materials), Copyright Babylon.js contributors.

//! Babylon's `Materials` folder, the parts SHAGGER needs.

pub mod standard_material;
pub mod texture;

/// `StandardMaterial`'s shader (`standard.wgsl`), as source.
pub const STANDARD_WGSL: &str = include_str!("standard.wgsl");
