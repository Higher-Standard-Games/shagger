// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// - packages/dev/core/src/Meshes/mesh.vertexData.ts: VertexData.ComputeNormals
//   (without the facet options), VertexData.transform and VertexData.merge;
// - packages/dev/core/src/Meshes/mesh.ts: Mesh.MergeMeshes (one material,
//   no submeshes), bakeTransformIntoVertices and convertToFlatShadedMesh
//   (_convertToUnIndexedMesh with _getFlattenedNormals, for a left-handed
//   scene, where a mesh's side orientation is counter-clockwise and normals
//   are not flipped).
//
// Changes: arrays are f32 positions, normals and uvs plus u32 indices in one
// struct; the arithmetic runs in f64 and is stored in f32 as Babylon's
// Float32Array vertex buffers store it.

//! A mesh's vertex arrays, as Babylon's `VertexData` holds them.

use glam::{DMat4, DVec3};

/// Positions, normals and UVs per vertex, and triangle indices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VertexData {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u32>,
}

impl VertexData {
    /// How many vertices.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    fn position(&self, i: usize) -> DVec3 {
        DVec3::new(
            f64::from(self.positions[i * 3]),
            f64::from(self.positions[i * 3 + 1]),
            f64::from(self.positions[i * 3 + 2]),
        )
    }

    /// `VertexData.ComputeNormals`: each vertex the normalized sum of its
    /// faces' normals, a face's normal `(p1 - p2) x (p3 - p2)`.
    pub fn compute_normals(&mut self) {
        let mut normals = vec![0.0f64; self.positions.len()];
        for face in self.indices.chunks_exact(3) {
            let [a, b, c] = [face[0] as usize, face[1] as usize, face[2] as usize];
            let n = face_normal(self.position(a), self.position(b), self.position(c));
            for v in [a, b, c] {
                normals[v * 3] += n.x;
                normals[v * 3 + 1] += n.y;
                normals[v * 3 + 2] += n.z;
            }
        }
        for n in normals.chunks_exact_mut(3) {
            let mut length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if length == 0.0 {
                length = 1.0;
            }
            n[0] /= length;
            n[1] /= length;
            n[2] /= length;
        }
        self.normals = normals.into_iter().map(|v| v as f32).collect();
    }

    /// `VertexData.transform`: positions by the matrix, normals by it as
    /// directions, and the winding flipped when it mirrors.
    pub fn transform(&mut self, matrix: DMat4) {
        self.transform_with(matrix, false);
    }

    /// `Mesh.bakeTransformIntoVertices`: [`Self::transform`], with the
    /// normals normalized.
    pub fn bake(&mut self, matrix: DMat4) {
        self.transform_with(matrix, true);
    }

    fn transform_with(&mut self, matrix: DMat4, normalize: bool) {
        for p in self.positions.chunks_exact_mut(3) {
            let v = matrix.transform_point3(DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])));
            p.copy_from_slice(&[v.x as f32, v.y as f32, v.z as f32]);
        }
        for n in self.normals.chunks_exact_mut(3) {
            let mut v = matrix.transform_vector3(DVec3::new(f64::from(n[0]), f64::from(n[1]), f64::from(n[2])));
            if normalize {
                v = v.normalize_or_zero();
            }
            n.copy_from_slice(&[v.x as f32, v.y as f32, v.z as f32]);
        }
        if matrix.determinant() < 0.0 {
            for face in self.indices.chunks_exact_mut(3) {
                face.swap(0, 2);
            }
        }
    }

    /// `VertexData.merge`: appends another mesh's arrays.
    pub fn merge(&mut self, other: &VertexData) {
        let base = self.vertex_count() as u32;
        self.indices.extend(other.indices.iter().map(|i| i + base));
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
    }

    /// `Mesh.MergeMeshes`: each part placed by its world matrix, merged
    /// into one.
    #[must_use]
    pub fn merge_meshes(parts: Vec<(VertexData, DMat4)>) -> VertexData {
        let mut out = VertexData::default();
        for (mut part, world) in parts {
            if world != DMat4::IDENTITY {
                part.transform(world);
            }
            out.merge(&part);
        }
        out
    }

    /// `Mesh.convertToFlatShadedMesh`: every triangle gets its own three
    /// vertices, with the face's normal `normalize((p1 - p2) x (p3 - p2))`.
    #[must_use]
    pub fn flat_shaded(&self) -> VertexData {
        let mut out = VertexData::default();
        for face in self.indices.chunks_exact(3) {
            let [a, b, c] = [face[0] as usize, face[1] as usize, face[2] as usize];
            let n = face_normal(self.position(a), self.position(b), self.position(c)).normalize_or_zero();
            for v in [a, b, c] {
                out.positions.extend_from_slice(&self.positions[v * 3..v * 3 + 3]);
                out.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
                if self.uvs.len() >= (v + 1) * 2 {
                    out.uvs.extend_from_slice(&self.uvs[v * 2..v * 2 + 2]);
                }
            }
        }
        out.indices = (0..out.vertex_count() as u32).collect();
        out
    }
}

fn face_normal(p1: DVec3, p2: DVec3, p3: DVec3) -> DVec3 {
    let a = p1 - p2;
    let b = p3 - p2;
    DVec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

/// A Babylon node's local matrix from position, Euler rotation
/// (`RotationYawPitchRoll(y, x, z)`) and scaling, in glam's column-vector
/// form.
#[must_use]
pub fn babylon_local_matrix(position: DVec3, rotation: DVec3, scaling: DVec3) -> DMat4 {
    let q = glam::DQuat::from_euler(glam::EulerRot::YXZ, rotation.y, rotation.x, rotation.z);
    DMat4::from_scale_rotation_translation(scaling, q, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_normals_follow_babylons_winding() {
        // Babylon's plane: (-1,-1), (1,-1), (1,1) faces -Z.
        let v = VertexData {
            positions: vec![-1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0],
            normals: vec![0.0; 9],
            uvs: vec![0.0; 6],
            indices: vec![0, 1, 2],
        };
        assert_eq!(&v.flat_shaded().normals[..3], &[0.0, 0.0, -1.0]);
        let mut smooth = v.clone();
        smooth.compute_normals();
        assert_eq!(&smooth.normals[..3], &[0.0, 0.0, -1.0]);
    }

    #[test]
    fn mirroring_flips_the_winding() {
        let mut v = VertexData {
            positions: vec![0.0; 9],
            normals: vec![0.0; 9],
            uvs: vec![],
            indices: vec![0, 1, 2],
        };
        v.transform(DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        assert_eq!(v.indices, vec![2, 1, 0]);
    }
}
