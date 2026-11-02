//! SHAG `src/engine/loader/bsp/mesh/tangents.ts`: Qfusion's image-space
//! tangent frame for BSP surfaces with normal maps.

use crate::shared::js::js_hypot;

/// `buildBspTangents`: per vertex, the tangent (xyz, Gram-Schmidt against the
/// normal) and the bitangent's handedness (w), as Babylon's `TangentKind`.
#[must_use]
pub fn build_bsp_tangents(positions: &[f32], normals: &[f32], uvs: &[f32], indices: &[u32]) -> Vec<f32> {
    let count = positions.len() / 3;
    let mut s = vec![0.0f64; count * 3];
    let mut t = vec![0.0f64; count * 3];
    let p = |i: usize| f64::from(positions[i]);
    let uv = |i: usize| f64::from(uvs[i]);
    for tri in indices.as_chunks::<3>().0.iter() {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let du1 = uv(b * 2) - uv(a * 2);
        let dv1 = uv(b * 2 + 1) - uv(a * 2 + 1);
        let du2 = uv(c * 2) - uv(a * 2);
        let dv2 = uv(c * 2 + 1) - uv(a * 2 + 1);
        let area = du1 * dv2 - du2 * dv1;
        // Math.sign: zero (and NaN) skip the triangle.
        if area == 0.0 || area.is_nan() {
            continue;
        }
        let sign = area.signum();
        for k in 0..3 {
            let e1 = p(b * 3 + k) - p(a * 3 + k);
            let e2 = p(c * 3 + k) - p(a * 3 + k);
            let sv = sign * (dv2 * e1 - dv1 * e2);
            let tv = -sign * (du1 * e2 - du2 * e1);
            for v in [a, b, c] {
                s[v * 3 + k] += sv;
                t[v * 3 + k] += tv;
            }
        }
    }
    let mut result = vec![0.0f32; count * 4];
    for i in 0..count {
        let x = f64::from(normals[i * 3]);
        let y = f64::from(normals[i * 3 + 1]);
        let z = f64::from(normals[i * 3 + 2]);
        let dot = s[i * 3] * x + s[i * 3 + 1] * y + s[i * 3 + 2] * z;
        let mut tx = s[i * 3] - dot * x;
        let mut ty = s[i * 3 + 1] - dot * y;
        let mut tz = s[i * 3 + 2] - dot * z;
        let mut length = js_hypot(&[tx, ty, tz]);
        if length < 1e-10 {
            // Degenerate UVs still need a finite frame perpendicular to the normal.
            if z.abs() < 0.9 {
                (tx, ty, tz) = (-y, x, 0.0);
            } else {
                (tx, ty, tz) = (z, 0.0, -x);
            }
            length = js_hypot(&[tx, ty, tz]);
            if length == 0.0 || length.is_nan() {
                length = 1.0;
            }
        }
        tx /= length;
        ty /= length;
        tz /= length;
        let handed = (y * tz - z * ty) * t[i * 3] + (z * tx - x * tz) * t[i * 3 + 1] + (x * ty - y * tx) * t[i * 3 + 2];
        result[i * 4..i * 4 + 4].copy_from_slice(&[
            tx as f32,
            ty as f32,
            tz as f32,
            if handed < 0.0 { -1.0 } else { 1.0 },
        ]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against `buildBspTangents` in V8: a quad, a mirrored-UV triangle and
    /// a degenerate-UV triangle.
    #[test]
    fn matches_shag() {
        let positions = [
            0., 0., 0., 64., 0., 0., 64., 0., 64., 0., 0., 64., 0., 10., 0., 0., 10., 32., 32., 10., 0., 5., 5., 5.,
            6., 5., 5., 5., 6., 5.,
        ];
        let normals = [
            0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 0., 1., 0., 0., 1.,
            0., 0., 1.,
        ];
        let uvs = [
            0., 0., 1., 0., 1., 1., 0., 1., 0., 0., 0.5, 0., 0., 0.5, 0.3, 0.3, 0.3, 0.3, 0.3, 0.3,
        ];
        let indices = [0, 1, 2, 0, 2, 3, 4, 5, 6, 7, 8, 9];
        let expected = [
            1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1., 0., 0., 1., -1., 0., 0., 1., -1., 0., 0.,
            1., -1., 1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1.,
        ];
        assert_eq!(build_bsp_tangents(&positions, &normals, &uvs, &indices), expected);
    }
}
