//! SHAG `src/engine/loader/bsp/mesh/coordinates.ts`.
//!
//! Q3 is Z-up and right-handed (X right, Y forward, Z up); Babylon, and so the
//! renderer, is Y-up and left-handed (X right, Y up, Z forward). The swap
//! changes handedness, so triangle winding is reversed too.

/// `q3ToBabylon`: `(x, y, z)` to `(x, z, y)`.
#[must_use]
pub fn q3_to_babylon<T>(x: T, y: T, z: T) -> [T; 3] {
    [x, z, y]
}

/// `reverseTriangleWinding`: `[i0, i1, i2, ...]` to `[i0, i2, i1, ...]`.
/// A trailing partial triangle is padded as the TypeScript pads it with
/// `undefined`; here it is dropped.
#[must_use]
pub fn reverse_triangle_winding(indices: &[u32]) -> Vec<u32> {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|t| [t[0], t[2], t[1]])
        .collect()
}
