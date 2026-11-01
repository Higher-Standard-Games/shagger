//! The usercmd clamp the server and the predicting client share
//! (SHAG `src/shared/server/simulation/InputSanitizer.ts`).

use crate::js::{js_hypot, js_max, js_min, to_int32};
use std::f64::consts::PI;

pub const KNOWN_BUTTON_MASK: u32 = 0xffff;

const HALF_PI: f64 = PI / 2.0;
const MAX_WEAPON_SLOT: i32 = 9;

/// `SanitizedMovement`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SanitizedMovement {
    pub direction: [f64; 3],
    pub pitch: f64,
    pub yaw: f64,
    pub buttons: u32,
}

/// `SanitizedCommand`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SanitizedCommand {
    pub movement: SanitizedMovement,
    pub weapon_slot: i32,
    pub sequence: f64,
    pub view_tick: i32,
}

/// The fields `sanitizeMovement` reads; `None` is a missing field.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MovementFields {
    pub direction: Option<[f64; 3]>,
    pub pitch: Option<f64>,
    pub yaw: Option<f64>,
    pub buttons: Option<f64>,
}

/// `wrapAngle`: into (-pi, pi] with one add or subtract, as the TypeScript does.
#[must_use]
pub fn wrap_angle(angle: f64) -> f64 {
    let two_pi = PI * 2.0;
    let mut result = angle % two_pi;
    if result > PI {
        result -= two_pi;
    } else if result <= -PI {
        result += two_pi;
    }
    result
}

/// `sanitizeMovement`: `None` rejects a non-finite direction, pitch or yaw.
#[must_use]
pub fn sanitize_movement(input: MovementFields) -> Option<SanitizedMovement> {
    let dx = input.direction.map_or(0.0, |d| d[0]);
    let dz = input.direction.map_or(0.0, |d| d[2]);
    let pitch = input.pitch.unwrap_or(0.0);
    let yaw = input.yaw.unwrap_or(0.0);
    if !dx.is_finite() || !dz.is_finite() || !pitch.is_finite() || !yaw.is_finite() {
        return None;
    }
    let len = js_hypot(&[dx, dz]);
    let scale = if len > 1.0 { 1.0 / len } else { 1.0 };
    Some(SanitizedMovement {
        direction: [dx * scale, 0.0, dz * scale],
        pitch: js_max(-HALF_PI, js_min(HALF_PI, pitch)),
        yaw: wrap_angle(yaw),
        buttons: (to_int32(input.buttons.unwrap_or(0.0)) as u32) & KNOWN_BUTTON_MASK,
    })
}

/// `sanitizeCommand`: the movement, the weapon slot bounded to 0..=9, and the
/// view tick floored at 0.
#[must_use]
pub fn sanitize_command(
    movement: MovementFields,
    weapon_slot: Option<f64>,
    seq: Option<f64>,
    tick: Option<f64>,
) -> Option<SanitizedCommand> {
    let movement = sanitize_movement(movement)?;
    Some(SanitizedCommand {
        movement,
        weapon_slot: to_int32(weapon_slot.unwrap_or(0.0)).clamp(0, MAX_WEAPON_SLOT),
        sequence: seq.unwrap_or(0.0),
        view_tick: to_int32(tick.unwrap_or(0.0)).max(0),
    })
}

/// `sanitizeMovementOrZero`: a zero movement in place of a rejected one.
#[must_use]
pub fn sanitize_movement_or_zero(input: MovementFields) -> SanitizedMovement {
    sanitize_movement(input).unwrap_or(SanitizedMovement {
        direction: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        buttons: 0,
    })
}
