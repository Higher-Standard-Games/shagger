// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, packages/dev/core/src/Maths/math.vector.ts
// (class Vector3), Copyright Babylon.js contributors, licensed under the
// Apache License 2.0 (see ../LICENSE).
//
// Changes: only the methods SHAG's collision code calls, as plain value
// methods on a Copy struct. The arithmetic and its order are Babylon's, so
// results match bit for bit.

//! `Vector3`, as SHAG's physics uses it.

// Babylon's method names (`add`, `subtract`, ...), kept so ports read alike.
#![allow(clippy::should_implement_trait)]

/// Babylon's `Vector3`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// `Vector3.Zero()`.
    #[must_use]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    /// `add`.
    #[must_use]
    pub fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    /// `addInPlace`.
    pub fn add_in_place(&mut self, o: Self) {
        *self = self.add(o);
    }

    /// `subtract`.
    #[must_use]
    pub fn subtract(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    /// `scale`.
    #[must_use]
    pub fn scale(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    /// `lengthSquared`.
    #[must_use]
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// `length`.
    #[must_use]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// `normalize`: scales by the reciprocal of the length, and leaves a zero
    /// or unit-length vector untouched.
    #[must_use]
    pub fn normalize(self) -> Self {
        let len = self.length();
        #[allow(clippy::float_cmp)]
        if len == 0.0 || len == 1.0 {
            return self;
        }
        self.scale(1.0 / len)
    }

    /// `Vector3.Dot`.
    #[must_use]
    pub fn dot(a: Self, b: Self) -> f64 {
        a.x * b.x + a.y * b.y + a.z * b.z
    }

    /// `Vector3.Cross`.
    #[must_use]
    pub fn cross(l: Self, r: Self) -> Self {
        Self::new(l.y * r.z - l.z * r.y, l.z * r.x - l.x * r.z, l.x * r.y - l.y * r.x)
    }

    /// `Vector3.DistanceSquared`.
    #[must_use]
    pub fn distance_squared(a: Self, b: Self) -> f64 {
        let (x, y, z) = (a.x - b.x, a.y - b.y, a.z - b.z);
        x * x + y * y + z * z
    }

    /// `Vector3.Distance`.
    #[must_use]
    pub fn distance(a: Self, b: Self) -> f64 {
        Self::distance_squared(a, b).sqrt()
    }
}
