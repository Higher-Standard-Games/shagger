// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2,
// packages/dev/core/src/Materials/Textures/texture.ts (Texture.getTextureMatrix
// and _prepareRowForTextureGeneration), Copyright Babylon.js contributors,
// licensed under the Apache License 2.0 (see ../LICENSE).
//
// Changes: only the default, non-homogeneous path with rotation about W
// (uAng and vAng zero, rotation centres 0.5), which is what SHAG's shader
// animation sets; the rows come back as an affine 2D map, in f64, instead of
// a cached Float32 Matrix.

//! The UV transform a Babylon `Texture` applies from its offset, scale and
//! `wAng`.

/// An affine UV map: `uv' = u * u_row + v * v_row + origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureMatrix {
    pub u_row: [f64; 2],
    pub v_row: [f64; 2],
    pub origin: [f64; 2],
}

impl TextureMatrix {
    pub const IDENTITY: Self = Self {
        u_row: [1.0, 0.0],
        v_row: [0.0, 1.0],
        origin: [0.0, 0.0],
    };
}

/// `getTextureMatrix()` for a texture with these offsets, scales and `wAng`.
#[must_use]
pub fn get_texture_matrix(u_offset: f64, v_offset: f64, u_scale: f64, v_scale: f64, w_ang: f64) -> TextureMatrix {
    const CENTER: f64 = 0.5;
    // Matrix.RotationYawPitchRoll(0, 0, wAng): a rotation about Z, applied
    // to row vectors.
    let (s, c) = w_ang.sin_cos();
    // _prepareRowForTextureGeneration(x, y, 0).
    let row = |x: f64, y: f64| {
        let x = x * u_scale - CENTER * u_scale;
        let y = y * v_scale - CENTER * v_scale;
        [
            x * c - y * s + CENTER * u_scale + u_offset,
            x * s + y * c + CENTER * v_scale + v_offset,
        ]
    };
    let t0 = row(0.0, 0.0);
    let t1 = row(1.0, 0.0);
    let t2 = row(0.0, 1.0);
    TextureMatrix {
        u_row: [t1[0] - t0[0], t1[1] - t0[1]],
        v_row: [t2[0] - t0[0], t2[1] - t0[1]],
        origin: t0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_offset() {
        assert_eq!(get_texture_matrix(0.0, 0.0, 1.0, 1.0, 0.0), TextureMatrix::IDENTITY);
        let m = get_texture_matrix(0.25, -0.5, 2.0, 3.0, 0.0);
        assert_eq!(m.u_row, [2.0, 0.0]);
        assert_eq!(m.v_row, [0.0, 3.0]);
        assert_eq!(m.origin, [0.25, -0.5]);
    }

    #[test]
    fn quarter_turn_keeps_the_centre() {
        let m = get_texture_matrix(0.0, 0.0, 1.0, 1.0, std::f64::consts::FRAC_PI_2);
        let centre = [
            0.5 * m.u_row[0] + 0.5 * m.v_row[0] + m.origin[0],
            0.5 * m.u_row[1] + 0.5 * m.v_row[1] + m.origin[1],
        ];
        assert!((centre[0] - 0.5).abs() < 1e-12 && (centre[1] - 0.5).abs() < 1e-12);
    }
}
