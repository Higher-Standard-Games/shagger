//! SHAG `src/engine/viewport/SpeedConstants.ts`.

/// Base ground speed (320 since the beta; it was 280).
pub const BASE_SPEED: f64 = 320.0;
pub const MAX_SPEED: f64 = BASE_SPEED * 2.0;

/// `getSpeedRatio`: 0 at walking speed, 1 at twice it.
#[must_use]
pub fn get_speed_ratio(horizontal_speed: f64) -> f64 {
    if horizontal_speed <= BASE_SPEED {
        return 0.0;
    }
    if horizontal_speed >= MAX_SPEED {
        return 1.0;
    }
    (horizontal_speed - BASE_SPEED) / (MAX_SPEED - BASE_SPEED)
}
