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

/// The vector arithmetic map collision runs through (`physics`). Each sum
/// runs x, y, z, left to right, as the TypeScript's does, so a trace or a
/// slide comes out the same to the last bit. Named methods, not operator
/// traits, keep each call's order visible where the backend makes it.
#[allow(clippy::should_implement_trait)]
impl Vec3 {
    #[must_use]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    #[must_use]
    pub fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    #[must_use]
    pub fn subtract(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    #[must_use]
    pub fn scale(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    #[must_use]
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    #[must_use]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Scaled by one over its length. A zero vector, or one already of
    /// length exactly 1, comes back unchanged.
    #[must_use]
    pub fn normalize(self) -> Self {
        let length = self.length();
        #[allow(clippy::float_cmp)]
        if length == 0.0 || length == 1.0 {
            return self;
        }
        self.scale(1.0 / length)
    }

    #[must_use]
    pub fn dot(a: Self, b: Self) -> f64 {
        a.x * b.x + a.y * b.y + a.z * b.z
    }

    #[must_use]
    pub fn cross(a: Self, b: Self) -> Self {
        Self::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
    }

    #[must_use]
    pub fn distance_squared(a: Self, b: Self) -> f64 {
        let (x, y, z) = (a.x - b.x, a.y - b.y, a.z - b.z);
        x * x + y * y + z * z
    }

    #[must_use]
    pub fn distance(a: Self, b: Self) -> f64 {
        Self::distance_squared(a, b).sqrt()
    }
}
