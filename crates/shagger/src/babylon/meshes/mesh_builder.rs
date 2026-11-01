// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// packages/dev/core/src/Meshes/Builders/boxBuilder.ts (CreateBoxVertexData),
// sphereBuilder.ts (CreateSphereVertexData), torusBuilder.ts
// (CreateTorusVertexData), cylinderBuilder.ts (CreateCylinderVertexData) and
// planeBuilder.ts (CreatePlaneVertexData).
//
// Changes: only the options SHAG's procedural pickups pass. The default
// (front) side orientation, default face UVs and colours, no box wrap, no
// sphere arc, slice or index dedup, no cylinder subdivisions, rings, arc or
// enclose, both caps. UVs follow `useOpenGLOrientationForUV` false.

//! Babylon's `MeshBuilder` vertex data for the shapes SHAG builds meshes
//! from.

use glam::{DMat4, DVec3};

use super::vertex_data::VertexData;

fn push3(out: &mut Vec<f32>, v: DVec3) {
    out.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32]);
}

/// `CreateBoxVertexData({ width, height, depth })`.
#[must_use]
pub fn create_box(width: f64, height: f64, depth: f64) -> VertexData {
    let indices = vec![
        0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11, 12, 13, 14, 12, 14, 15, 16, 17, 18, 16, 18, 19, 20,
        21, 22, 20, 22, 23,
    ];
    let normals: [f64; 72] = [
        0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 1., 0., 0.,
        1., 0., 0., 1., 0., 0., 1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0., 0., 0., 1., 0., 0., 1., 0.,
        0., 1., 0., 0., 1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0.,
    ];
    let base: [f64; 72] = [
        1., -1., 1., -1., -1., 1., -1., 1., 1., 1., 1., 1., 1., 1., -1., -1., 1., -1., -1., -1., -1., 1., -1., -1., 1.,
        1., -1., 1., -1., -1., 1., -1., 1., 1., 1., 1., -1., 1., 1., -1., -1., 1., -1., -1., -1., -1., 1., -1., -1.,
        1., 1., -1., 1., -1., 1., 1., -1., 1., 1., 1., 1., -1., 1., 1., -1., -1., -1., -1., -1., -1., -1., 1.,
    ];
    let scale = [width / 2.0, height / 2.0, depth / 2.0];
    let positions = base
        .iter()
        .enumerate()
        .map(|(i, v)| (v * scale[i % 3]) as f32)
        .collect();
    let mut uvs = Vec::new();
    for _ in 0..6 {
        uvs.extend_from_slice(&[1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]);
    }
    VertexData {
        positions,
        normals: normals.iter().map(|&v| v as f32).collect(),
        uvs,
        indices,
    }
}

/// `CreateSphereVertexData({ diameter, segments })`.
#[must_use]
pub fn create_sphere(diameter: f64, segments: u32) -> VertexData {
    let radius = DVec3::splat(diameter / 2.0);
    let z_steps = 2 + segments;
    let y_steps = 2 * z_steps;
    let mut out = VertexData::default();
    for zs in 0..=z_steps {
        let nz = f64::from(zs) / f64::from(z_steps);
        let angle_z = nz * std::f64::consts::PI;
        for ys in 0..=y_steps {
            let ny = f64::from(ys) / f64::from(y_steps);
            let angle_y = ny * std::f64::consts::PI * 2.0;
            let after_z = DMat4::from_rotation_z(-angle_z).transform_point3(DVec3::Y);
            let complete = DMat4::from_rotation_y(angle_y).transform_point3(after_z);
            push3(&mut out.positions, complete * radius);
            push3(&mut out.normals, (complete / radius).normalize());
            out.uvs.extend_from_slice(&[ny as f32, nz as f32]);
        }
        if zs > 0 {
            let count = (out.positions.len() / 3) as i64;
            let ys = i64::from(y_steps);
            let mut first = count - 2 * (ys + 1);
            while first + ys + 2 < count {
                let f = first as u32;
                let y = y_steps;
                out.indices
                    .extend_from_slice(&[f, f + 1, f + y + 1, f + y + 1, f + 1, f + y + 2]);
                first += 1;
            }
        }
    }
    out
}

/// `CreateTorusVertexData({ diameter, thickness, tessellation })`.
#[must_use]
pub fn create_torus(diameter: f64, thickness: f64, tessellation: u32) -> VertexData {
    let mut out = VertexData::default();
    let stride = tessellation + 1;
    let t = f64::from(tessellation);
    for i in 0..=tessellation {
        let u = f64::from(i) / t;
        let outer = f64::from(i) * std::f64::consts::PI * 2.0 / t - std::f64::consts::FRAC_PI_2;
        // Translation, then the turn about Y.
        let transform = DMat4::from_rotation_y(outer) * DMat4::from_translation(DVec3::new(diameter / 2.0, 0.0, 0.0));
        for j in 0..=tessellation {
            let v = 1.0 - f64::from(j) / t;
            let inner = f64::from(j) * std::f64::consts::PI * 2.0 / t + std::f64::consts::PI;
            let normal = DVec3::new(inner.cos(), inner.sin(), 0.0);
            push3(
                &mut out.positions,
                transform.transform_point3(normal * (thickness / 2.0)),
            );
            push3(&mut out.normals, transform.transform_vector3(normal));
            out.uvs.extend_from_slice(&[u as f32, v as f32]);
            let next_i = (i + 1) % stride;
            let next_j = (j + 1) % stride;
            out.indices.extend_from_slice(&[
                i * stride + j,
                i * stride + next_j,
                next_i * stride + j,
                i * stride + next_j,
                next_i * stride + next_j,
                next_i * stride + j,
            ]);
        }
    }
    out
}

/// `CreatePlaneVertexData({ width, height })`: in XY, facing -Z.
#[must_use]
pub fn create_plane(width: f64, height: f64) -> VertexData {
    let (w, h) = ((width / 2.0) as f32, (height / 2.0) as f32);
    VertexData {
        positions: vec![-w, -h, 0.0, w, -h, 0.0, w, h, 0.0, -w, h, 0.0],
        normals: vec![0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0],
        uvs: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

/// `CreateCylinderVertexData({ height, diameterTop, diameterBottom,
/// tessellation })`, capped at both ends.
#[must_use]
pub fn create_cylinder(height: f64, diameter_top: f64, diameter_bottom: f64, tessellation: u32) -> VertexData {
    let top = if diameter_top == 0.0 { 0.00001 } else { diameter_top };
    let bottom = if diameter_bottom == 0.0 {
        0.00001
    } else {
        diameter_bottom
    };
    let t = f64::from(tessellation);
    let angle_step = std::f64::consts::PI * 2.0 / t;
    let tan = (bottom - top) / 2.0 / height;
    let mut out = VertexData::default();
    for i in 0..=1u32 {
        let h = f64::from(i);
        let radius = (h * (top - bottom) + bottom) / 2.0;
        for j in 0..=tessellation {
            let angle = f64::from(j) * angle_step;
            let vertex = DVec3::new(
                (-angle).cos() * radius,
                -height / 2.0 + h * height,
                (-angle).sin() * radius,
            );
            let normal = DVec3::new(
                vertex.x,
                (vertex.x * vertex.x + vertex.z * vertex.z).sqrt() * tan,
                vertex.z,
            )
            .normalize();
            push3(&mut out.positions, vertex);
            push3(&mut out.normals, normal);
            out.uvs.extend_from_slice(&[(f64::from(j) / t) as f32, h as f32]);
        }
    }
    let e = tessellation;
    for j in 0..tessellation {
        let i0 = j;
        let i1 = (e + 1) + j;
        let i2 = j + 1;
        let i3 = (e + 1) + j + 1;
        out.indices.extend_from_slice(&[i0, i1, i2, i3, i2, i1]);
    }
    for is_top in [false, true] {
        let radius = if is_top { top / 2.0 } else { bottom / 2.0 };
        let base = (out.positions.len() / 3) as u32;
        let offset = if is_top { height / 2.0 } else { -height / 2.0 };
        let ny = if is_top { 1.0 } else { -1.0 };
        push3(&mut out.positions, DVec3::new(0.0, offset, 0.0));
        out.normals.extend_from_slice(&[0.0, ny, 0.0]);
        out.uvs.extend_from_slice(&[0.5, 0.5]);
        for i in 0..=tessellation {
            let angle = std::f64::consts::PI * 2.0 * f64::from(i) / t;
            let (cos, sin) = ((-angle).cos(), (-angle).sin());
            push3(&mut out.positions, DVec3::new(cos * radius, offset, sin * radius));
            out.normals.extend_from_slice(&[0.0, ny, 0.0]);
            out.uvs
                .extend_from_slice(&[(cos * 0.5 + 0.5) as f32, (sin * 0.5 + 0.5) as f32]);
        }
        for i in 0..tessellation {
            if is_top {
                out.indices.extend_from_slice(&[base, base + i + 2, base + i + 1]);
            } else {
                out.indices.extend_from_slice(&[base, base + i + 1, base + i + 2]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_have_babylons_counts() {
        let b = create_box(2.0, 4.0, 6.0);
        assert_eq!((b.vertex_count(), b.indices.len()), (24, 36));
        assert_eq!(&b.positions[..3], &[1.0, -2.0, 3.0]);
        let s = create_sphere(2.0, 24);
        // (2 + 24 + 1) rings of (2 * 26 + 1) vertices.
        assert_eq!(s.vertex_count(), 27 * 53);
        assert_eq!(s.indices.len(), 26 * 52 * 6);
        let t = create_torus(21.0, 0.9, 28);
        assert_eq!((t.vertex_count(), t.indices.len()), (29 * 29, 29 * 29 * 6));
        let c = create_cylinder(3.5, 20.0, 20.0, 8);
        assert_eq!(c.vertex_count(), 18 + 2 * 10);
        assert_eq!(c.indices.len(), 8 * 6 + 2 * 8 * 3);
    }

    #[test]
    fn torus_and_sphere_points_sit_where_babylon_puts_them() {
        let t = create_torus(2.0, 0.5, 4);
        // i = 0: outer angle -90 deg turns (1, 0, 0) to (0, 0, 1); j = 0
        // starts on the inside (inner angle 180 deg).
        let p = &t.positions[..3];
        assert!((p[0] - 0.0).abs() < 1e-6 && (p[2] - 0.75).abs() < 1e-6, "{p:?}");
        let s = create_sphere(2.0, 1);
        assert_eq!(&s.positions[..3], &[0.0, 1.0, 0.0]);
    }
}
