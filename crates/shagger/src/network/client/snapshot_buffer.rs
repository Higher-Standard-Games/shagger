//! The interpolation buffer's output (SHAG
//! `src/engine/network/client/SnapshotBuffer.ts`). Only
//! `InterpolatedEntity`, what the player renderer reads, is ported so far.

/// One entity as the buffer interpolates it for a frame, in Babylon's
/// space (y up) with the yaw Babylon turns the body by.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InterpolatedEntity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    /// View pitch in radians, as the player aims.
    pub pitch: Option<f64>,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub grounded: bool,
    pub crouching: bool,
    pub sliding: bool,
    pub wall_jumping: bool,
    pub aiming: Option<bool>,
    pub crouch_view_offset: Option<f64>,
    /// Strafe keys: -1 left, 0 none, 1 right.
    pub strafe_input: Option<i8>,
    /// Forward keys: -1 back, 0 none, 1 forward.
    pub forward_input: Option<i8>,
    pub slide_phase: Option<f64>,
    pub ledge_grab_phase: Option<f64>,
    /// Held powerups as a bitmask over the game's `POWERUP_KINDS`.
    pub powerup_mask: Option<u32>,
    /// How deep in liquid: 0 dry, 1 wading, 2 swimming, 3 submerged.
    pub water_level: Option<u8>,
    pub on_ladder: Option<bool>,
}
