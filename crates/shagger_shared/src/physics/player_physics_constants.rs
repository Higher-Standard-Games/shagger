//! The player hull (SHAG `src/engine/physics/PlayerPhysicsConstants.ts`).

pub const PLAYER_RADIUS: f64 = 16.0;
pub const PLAYER_MINS_Z: f64 = -24.0;
pub const PLAYER_MAXS_Z: f64 = 40.0;
pub const PLAYER_HEIGHT: f64 = PLAYER_MAXS_Z - PLAYER_MINS_Z;
pub const PLAYER_ORIGIN_HEIGHT: f64 = -PLAYER_MINS_Z;
pub const DEFAULT_VIEWHEIGHT: f64 = 34.0;
pub const CROUCH_VIEWHEIGHT: f64 = 12.0;
pub const DEAD_VIEWHEIGHT: f64 = -16.0;
pub const STEPSIZE: f64 = 18.0;
pub const CROUCH_MINS_Z: f64 = -4.0;
pub const CROUCH_HEIGHT: f64 = PLAYER_MAXS_Z - CROUCH_MINS_Z;

#[must_use]
pub fn feet_to_origin(feet_y: f64) -> f64 {
    feet_y + PLAYER_ORIGIN_HEIGHT
}

#[must_use]
pub fn origin_to_feet(origin_y: f64) -> f64 {
    origin_y - PLAYER_ORIGIN_HEIGHT
}

#[must_use]
pub fn origin_to_eyes(origin_y: f64, viewheight: f64) -> f64 {
    origin_y + viewheight
}

/// `getPlayerBounds`, Babylon axes (y up): `(mins, maxs)`.
#[must_use]
pub fn player_bounds(height: f64, radius: f64) -> ([f64; 3], [f64; 3]) {
    let mins_z = -(height * (24.0 / 56.0));
    let maxs_z = height + mins_z;
    ([-radius, mins_z, -radius], [radius, maxs_z, radius])
}

/// `getPlayerBoundsQ3`, Q3 axes (z up).
#[must_use]
pub fn player_bounds_q3(height: f64, radius: f64) -> ([f64; 3], [f64; 3]) {
    let mins_z = -(height * (24.0 / 56.0));
    let maxs_z = height + mins_z;
    ([-radius, -radius, mins_z], [radius, radius, maxs_z])
}
