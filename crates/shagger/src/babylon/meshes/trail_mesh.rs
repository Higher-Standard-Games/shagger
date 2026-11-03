// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, packages/dev/core/src/Meshes/trailMesh.ts
// (class TrailMesh), Copyright Babylon.js contributors, licensed under the
// Apache License 2.0 (see ../LICENSE).
//
// Changes: the mesh is its vertex data, not a scene mesh; the generator's
// world matrix is passed to `new` and to each `update` (Babylon reads it
// from the generator node before every render). Only the constructor's
// plain form (diameter, length, auto start) and `update` are ported. The
// arithmetic runs in f64 and is stored in f32, as a Float32Array keeps it.

//! `TrailMesh`: a tapering tube of rings that follows a node, each frame
//! shifting every ring one along and putting a new one at the node.

use glam::{DMat4, DVec3};

use super::vertex_data::VertexData;

/// `TrailMesh`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrailMesh {
    pub diameter: f64,
    length: usize,
    segments: usize,
    /// `_sectionPolygonPointsCount`.
    sections: usize,
    pub data: VertexData,
}

/// `Lerp`.
fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

impl TrailMesh {
    /// `new TrailMesh(name, generator, scene, diameter, length)`: every
    /// ring at the generator's position, facing +Z.
    #[must_use]
    pub fn new(generator: DMat4, diameter: f64, length: usize) -> Self {
        let sections = 4;
        let segments = length;
        let center = generator.w_axis.truncate();
        let alpha = (2.0 * std::f64::consts::PI) / sections as f64;
        let mut positions = Vec::new();
        let mut uvs = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let ring = |positions: &mut Vec<f32>| {
            for i in 0..=sections {
                let angle = if i == sections { 0.0 } else { i as f64 * alpha };
                positions.extend([
                    (center.x + angle.cos() * diameter) as f32,
                    (center.y + angle.sin() * diameter) as f32,
                    center.z as f32,
                ]);
            }
        };
        ring(&mut positions);
        for i in 0..=sections {
            uvs.extend([i as f32 / sections as f32, 0.0]);
        }
        for i in 1..=segments {
            ring(&mut positions);
            for j in 0..=sections {
                uvs.extend([j as f32 / sections as f32, i as f32 / segments as f32]);
            }
            let l = (positions.len() / 3 - 2 * (sections + 1)) as u32;
            let s = sections as u32;
            for j in 0..=s {
                indices.extend([l + j, l + j + s, l + j + s + 1]);
                indices.extend([l + j, l + j + s + 1, l + j + 1]);
            }
        }
        let mut data = VertexData {
            positions,
            normals: Vec::new(),
            uvs,
            indices,
        };
        data.compute_normals();
        // `autoStart` leaves the first frame's update to the render loop.
        Self {
            diameter,
            length,
            segments,
            sections,
            data,
        }
    }

    /// `_updateSectionVectors`: the ring and its normals around the
    /// generator, in its frame.
    fn section_vectors(&self, generator: DMat4) -> Vec<(DVec3, DVec3)> {
        let alpha = (2.0 * std::f64::consts::PI) / self.sections as f64;
        (0..=self.sections)
            .map(|i| {
                let angle = if i == self.sections { 0.0 } else { i as f64 * alpha };
                let v = DVec3::new(angle.cos() * self.diameter, angle.sin() * self.diameter, 0.0);
                let n = DVec3::new(angle.cos(), angle.sin(), 0.0);
                (generator.transform_point3(v), generator.transform_vector3(n))
            })
            .collect()
    }

    /// `update`: every ring takes the next one's place, shrunk along its
    /// normals (the taper), and the newest ring goes where the generator is.
    pub fn update(&mut self, generator: DMat4) {
        let section = self.section_vectors(generator);
        let index = 3 * (self.sections + 1);
        let amount = self.segments as f64 / self.length as f64;
        let length = self.length as f64;
        let positions = &mut self.data.positions;
        let normals = &mut self.data.normals;
        for i in index..positions.len() {
            positions[i - index] = (lerp(f64::from(positions[i - index]), f64::from(positions[i]), amount)
                - (f64::from(normals[i]) / length) * self.diameter) as f32;
        }
        for i in index..normals.len() {
            normals[i - index] = lerp(f64::from(normals[i - index]), f64::from(normals[i]), amount) as f32;
        }
        let l = positions.len() - index;
        for (i, (v, n)) in section.iter().enumerate() {
            positions[l + 3 * i] = v.x as f32;
            positions[l + 3 * i + 1] = v.y as f32;
            positions[l + 3 * i + 2] = v.z as f32;
            normals[l + 3 * i] = n.x as f32;
            normals[l + 3 * i + 1] = n.y as f32;
            normals[l + 3 * i + 2] = n.z as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rings_start_at_the_generator() {
        let trail = TrailMesh::new(DMat4::from_translation(DVec3::new(10.0, 20.0, 30.0)), 8.0, 30);
        assert_eq!(trail.data.vertex_count(), 31 * 5);
        assert_eq!(trail.data.indices.len(), 30 * 5 * 6);
        assert_eq!(&trail.data.positions[..3], &[18.0, 20.0, 30.0]);
        assert_eq!(trail.data.uvs[trail.data.uvs.len() - 1], 1.0);
    }

    #[test]
    fn the_newest_ring_follows_and_the_rest_shift() {
        let mut trail = TrailMesh::new(DMat4::IDENTITY, 1.0, 3);
        let before = trail.data.positions[15..18].to_vec();
        trail.update(DMat4::from_translation(DVec3::new(0.0, 0.0, 100.0)));
        let n = trail.data.positions.len();
        // The head ring sits around the generator.
        assert_eq!(&trail.data.positions[n - 15..n - 12], &[1.0, 0.0, 100.0]);
        // The ring behind took the old head's place, pulled in by the taper.
        let normal_x = trail.data.normals[15];
        let expected = before[0] - normal_x / 3.0;
        assert!((trail.data.positions[0] - expected).abs() < 1e-5);
    }
}
