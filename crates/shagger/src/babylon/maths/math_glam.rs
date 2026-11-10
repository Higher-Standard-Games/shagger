// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, packages/dev/core/src/Maths/math.vector.ts
// (Matrix.decompose, Quaternion.FromUnitVectorsToRef,
// Quaternion.RotationQuaternionFromAxisToRef, Quaternion.RotationAxisToRef),
// Copyright Babylon.js contributors, licensed under the Apache License 2.0
// (see ../LICENSE).
//
// Changes: written over glam's f32 types for the presentation code (player
// models and what they hold), where Babylon's results matter but not to the
// bit. Babylon multiplies matrices left to right as rows; glam's `a * b`
// applies `b` first, so Babylon's `A.multiply(B)` is `B * A` here.
// Quaternion products are Hamilton products in both.

//! Babylon's matrix and quaternion helpers the player model code calls.

use glam::{Mat3, Mat4, Quat, Vec3};

/// Babylon's default `Epsilon`.
pub const EPSILON: f32 = 0.001;

/// `Matrix.decompose`: scale, rotation and translation. A reflection (a
/// determinant of zero or less) goes into the Y scale, as Babylon puts it,
/// where glam would negate X. A zero scale gives the identity rotation.
#[must_use]
pub fn decompose(m: Mat4) -> (Vec3, Quat, Vec3) {
    let translation = m.w_axis.truncate();
    let mut scale = Vec3::new(
        m.x_axis.truncate().length(),
        m.y_axis.truncate().length(),
        m.z_axis.truncate().length(),
    );
    if m.determinant() <= 0.0 {
        scale.y = -scale.y;
    }
    if scale.x == 0.0 || scale.y == 0.0 || scale.z == 0.0 {
        return (scale, Quat::IDENTITY, translation);
    }
    let rotation = Mat3::from_cols(
        m.x_axis.truncate() / scale.x,
        m.y_axis.truncate() / scale.y,
        m.z_axis.truncate() / scale.z,
    );
    (scale, from_rotation_matrix(rotation), translation)
}

/// `Quaternion.FromRotationMatrixToRef`.
#[must_use]
pub fn from_rotation_matrix(m: Mat3) -> Quat {
    let (m11, m12, m13) = (m.x_axis.x, m.y_axis.x, m.z_axis.x);
    let (m21, m22, m23) = (m.x_axis.y, m.y_axis.y, m.z_axis.y);
    let (m31, m32, m33) = (m.x_axis.z, m.y_axis.z, m.z_axis.z);
    let trace = m11 + m22 + m33;
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        Quat::from_xyzw((m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s)
    } else if m11 > m22 && m11 > m33 {
        let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
        Quat::from_xyzw(0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s)
    } else if m22 > m33 {
        let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
        Quat::from_xyzw((m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s)
    } else {
        let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
        Quat::from_xyzw((m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s)
    }
}

/// `Quaternion.FromUnitVectorsToRef`: the turn from one unit vector to
/// another; opposite vectors turn about an axis square to `from`.
#[must_use]
pub fn from_unit_vectors(from: Vec3, to: Vec3) -> Quat {
    let r = from.dot(to) + 1.0;
    let q = if r < EPSILON {
        if from.x.abs() > from.z.abs() {
            Quat::from_xyzw(-from.y, from.x, 0.0, 0.0)
        } else {
            Quat::from_xyzw(0.0, -from.z, from.y, 0.0)
        }
    } else {
        let c = from.cross(to);
        Quat::from_xyzw(c.x, c.y, c.z, r)
    };
    normalize(q)
}

/// `Quaternion.normalize`: a zero quaternion stays zero.
#[must_use]
pub fn normalize(q: Quat) -> Quat {
    let length = q.length();
    if length == 0.0 { q } else { q * (1.0 / length) }
}

/// `Quaternion.RotationQuaternionFromAxisToRef`: the rotation whose X, Y and
/// Z axes are the given ones (each normalized first).
#[must_use]
pub fn rotation_from_axes(x: Vec3, y: Vec3, z: Vec3) -> Quat {
    let n = |v: Vec3| {
        let l = v.length();
        if l == 0.0 { v } else { v / l }
    };
    from_rotation_matrix(Mat3::from_cols(n(x), n(y), n(z)))
}

/// `Quaternion.RotationAxisToRef`: `angle` radians about `axis`, which need
/// not be unit length.
#[must_use]
pub fn rotation_axis(axis: Vec3, angle: f32) -> Quat {
    let s = (angle / 2.0).sin() / axis.length();
    Quat::from_xyzw(axis.x * s, axis.y * s, axis.z * s, (angle / 2.0).cos())
}

/// `Quaternion.FromEulerAngles(x, y, z)`, Babylon's yaw-pitch-roll order.
#[must_use]
pub fn from_euler_angles(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(glam::EulerRot::YXZ, y, x, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_puts_a_reflection_in_y() {
        let m = Mat4::from_scale_rotation_translation(Vec3::new(2.0, 3.0, 4.0), Quat::from_rotation_y(0.7), Vec3::ONE);
        let (s, r, t) = decompose(m);
        assert!((s - Vec3::new(2.0, 3.0, 4.0)).length() < 1e-5);
        assert!(r.angle_between(Quat::from_rotation_y(0.7)) < 1e-5);
        assert_eq!(t, Vec3::ONE);
        // A mirrored X reads as a mirrored Y turned half way round Z.
        let (s, r, _) = decompose(Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert_eq!(s, Vec3::new(1.0, -1.0, 1.0));
        let back = Mat4::from_scale_rotation_translation(s, r, Vec3::ZERO);
        assert!((back.x_axis.x + 1.0).abs() < 1e-6 && (back.y_axis.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn unit_vectors_turn_onto_each_other() {
        let q = from_unit_vectors(Vec3::Y, Vec3::X);
        assert!((q * Vec3::Y - Vec3::X).length() < 1e-6);
        let flip = from_unit_vectors(Vec3::Y, -Vec3::Y);
        assert!((flip * Vec3::Y + Vec3::Y).length() < 1e-6);
    }

    #[test]
    fn axes_and_angles() {
        let q = rotation_from_axes(Vec3::Z, Vec3::Y, -Vec3::X);
        assert!((q * Vec3::X - Vec3::Z).length() < 1e-6);
        let r = rotation_axis(Vec3::new(0.0, 2.0, 0.0), 0.5);
        assert!(r.angle_between(Quat::from_rotation_y(0.5)) < 1e-6);
    }
}
