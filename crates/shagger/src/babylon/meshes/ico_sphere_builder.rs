// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// packages/dev/core/src/Meshes/Builders/icoSphereBuilder.ts
// (CreateIcoSphereVertexData).
//
// Changes: positions and indices only (SHAG's explosion fireball keeps no
// normals or UVs), one radius on every axis, the default (front) side
// orientation.

//! Babylon's `MeshBuilder.CreateIcoSphere` vertex data.

use crate::babylon::maths::math_vector::Vector3;

use super::vertex_data::VertexData;

/// `Vector3.Lerp`.
fn lerp(a: Vector3, b: Vector3, t: f64) -> Vector3 {
    Vector3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t)
}

/// `CreateIcoSphereVertexData({ radius, subdivisions })`: an icosahedron's
/// faces each cut into `subdivisions`² triangles, pushed out onto the
/// sphere, every triangle with its own three vertices.
#[must_use]
pub fn create_ico_sphere(radius: f64, subdivisions: u32) -> VertexData {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    #[rustfmt::skip]
    let ico_vertices: [f64; 36] = [
        -1.0, t, -0.0, 1.0, t, 0.0, -1.0, -t, 0.0, 1.0, -t, 0.0,
        0.0, -1.0, -t, 0.0, 1.0, -t, 0.0, -1.0, t, 0.0, 1.0, t,
        t, 0.0, 1.0, t, 0.0, -1.0, -t, 0.0, 1.0, -t, 0.0, -1.0,
    ];
    #[rustfmt::skip]
    let ico_indices: [usize; 60] = [
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 12, 22, 23, 1, 5, 20, 5, 11, 4, 23, 22, 13, 22, 18, 6, 7, 1, 8, 14, 21, 4, 14, 4, 2, 16, 13, 6, 15, 6, 19, 3, 8, 9, 4, 21, 5, 13, 17,
        23, 6, 13, 22, 19, 6, 18, 9, 8, 1,
    ];
    let unalias: [usize; 24] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 2, 3, 3, 3, 4, 7, 8, 9, 9, 10, 11,
    ];
    let s = f64::from(subdivisions.max(1));
    let mut out = VertexData::default();
    for face in 0..20 {
        let corner: [Vector3; 3] = std::array::from_fn(|v| {
            let id = unalias[ico_indices[3 * face + v]];
            Vector3::new(ico_vertices[3 * id], ico_vertices[3 * id + 1], ico_vertices[3 * id + 2]).normalize()
        });
        let mut vertex = |i1: f64, i2: f64| {
            let x0 = lerp(corner[0], corner[2], i2 / s);
            let x1 = lerp(corner[1], corner[2], i2 / s);
            #[allow(clippy::float_cmp)]
            let p = if s == i2 {
                corner[2]
            } else {
                lerp(x0, x1, i1 / (s - i2))
            }
            .normalize();
            out.positions
                .extend_from_slice(&[(p.x * radius) as f32, (p.y * radius) as f32, (p.z * radius) as f32]);
            out.indices.push(out.indices.len() as u32);
        };
        for i2 in 0..subdivisions {
            for i1 in 0..subdivisions - i2 {
                let (a, b) = (f64::from(i1), f64::from(i2));
                vertex(a, b);
                vertex(a + 1.0, b);
                vertex(a, b + 1.0);
                if i1 + i2 + 1 < subdivisions {
                    vertex(a + 1.0, b);
                    vertex(a + 1.0, b + 1.0);
                    vertex(a, b + 1.0);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vertex_lies_on_the_sphere() {
        let ico = create_ico_sphere(2.5, 3);
        assert_eq!(ico.vertex_count(), 20 * 9 * 3);
        for p in ico.positions.chunks_exact(3) {
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!((r - 2.5).abs() < 1e-5);
        }
    }
}
