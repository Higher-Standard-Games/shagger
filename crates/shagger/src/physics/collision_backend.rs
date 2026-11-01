//! Trace types (SHAG `src/engine/physics/CollisionBackend.ts`). Babylon axes.

use crate::babylon::maths::math_vector::Vector3;

/// `TraceResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceResult {
    pub fraction: f64,
    pub end_pos: Vector3,
    pub normal: Option<Vector3>,
    pub start_solid: bool,
    pub all_solid: bool,
    /// Raw Q3 content flags of what was hit.
    pub contents: i32,
    /// -1 when no brush was hit (facets leave it at -1).
    pub hit_brush_index: i64,
    pub hit_texture_name: String,
    pub hit_surface_flags: i32,
}

/// `TraceOptions`. `None` takes the player hull and SOLID | PLAYERCLIP.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TraceOptions {
    pub radius: Option<f64>,
    pub height: Option<f64>,
    pub content_mask: Option<i32>,
}

/// `Contents` as `CollisionBackend.ts` lists them (raw Q3 values).
pub mod contents {
    pub const SOLID: i32 = 0x1;
    pub const LAVA: i32 = 0x8;
    pub const SLIME: i32 = 0x10;
    pub const WATER: i32 = 0x20;
    pub const FOG: i32 = 0x40;
    pub const PLAYERCLIP: i32 = 0x10000;
    pub const MONSTERCLIP: i32 = 0x20000;
    pub const TRIGGER: i32 = 0x4000_0000;
}
