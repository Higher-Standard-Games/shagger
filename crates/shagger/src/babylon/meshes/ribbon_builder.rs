// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// packages/dev/core/src/Meshes/Builders/ribbonBuilder.ts
// (CreateRibbonVertexData) and mesh.vertexData.ts (VertexData._ComputeSides,
// DOUBLESIDE).
//
// Changes: only what SHAG's projectile trails pass: two open paths of equal
// length, no closeArray or closePath, no custom UVs or colours, default
// front and back UVs, `sideOrientation` DOUBLESIDE.

//! Babylon's `MeshBuilder.CreateRibbon` vertex data.

use glam::DVec3;

use super::vertex_data::VertexData;

/// `CreateRibbonVertexData({ pathArray: [a, b], sideOrientation:
/// DOUBLESIDE })`: the paths' points in order, two triangles per segment,
/// then a copy of every vertex wound the other way with its normal negated.
#[must_use]
pub fn create_ribbon_double_side(path_a: &[DVec3], path_b: &[DVec3]) -> VertexData {
    let n = path_a.len().min(path_b.len());
    let paths = [&path_a[..n], &path_b[..n]];
    let mut data = VertexData::default();
    // us: the distance along each path; vs: across the paths.
    let mut us = [vec![0.0f64], vec![0.0f64]];
    for (p, path) in paths.iter().enumerate() {
        for (j, point) in path.iter().enumerate() {
            data.positions
                .extend_from_slice(&[point.x as f32, point.y as f32, point.z as f32]);
            if j > 0 {
                let d = (*point - path[j - 1]).length() + us[p][j - 1];
                us[p].push(d);
            }
        }
    }
    let u_total = [us[0][n.saturating_sub(1)], us[1][n.saturating_sub(1)]];
    for (p, path_us) in us.iter().enumerate() {
        for i in 0..n {
            let across = (paths[1][i] - paths[0][i]).length();
            let u = if u_total[p] != 0.0 {
                path_us[i] / u_total[p]
            } else {
                0.0
            };
            let v = if across != 0.0 { [0.0, across][p] / across } else { 0.0 };
            data.uvs.extend_from_slice(&[u as f32, v as f32]);
        }
    }
    let shift = n as u32;
    for i in 0..n.saturating_sub(1) as u32 {
        data.indices.extend_from_slice(&[i, i + shift, i + 1]);
        data.indices.extend_from_slice(&[i + shift + 1, i + 1, i + shift]);
    }
    data.compute_normals();
    // DOUBLESIDE.
    let count = data.vertex_count() as u32;
    data.positions.extend_from_within(..);
    let front = data.indices.len();
    for f in 0..front / 3 {
        let t = [data.indices[f * 3], data.indices[f * 3 + 1], data.indices[f * 3 + 2]];
        data.indices
            .extend_from_slice(&[t[2] + count, t[1] + count, t[0] + count]);
    }
    let back: Vec<f32> = data.normals.iter().map(|v| -v).collect();
    data.normals.extend(back);
    data.uvs.extend_from_within(..);
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_paths_make_a_double_sided_strip() {
        let a = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];
        let b = a.map(|p| p + DVec3::Y);
        let r = create_ribbon_double_side(&a, &b);
        assert_eq!(r.vertex_count(), 12);
        assert_eq!(&r.indices[..6], &[0, 3, 1, 4, 1, 3]);
        assert_eq!(&r.indices[12..15], &[7, 9, 6]);
        assert_eq!(r.uvs.len(), 24);
        assert_eq!(&r.uvs[..6], &[0.0, 0.0, 0.5, 0.0, 1.0, 0.0]);
        assert_eq!(&r.uvs[6..8], &[0.0, 1.0]);
        assert_eq!(r.normals[2], -r.normals[18 + 2]);
    }
}
