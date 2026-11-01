//! Simulation value types (SHAG `src/shared/server/simulation/types/index.ts`).

/// `Vec3`: a plain `{ x, y, z }` in game space (y up).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}
