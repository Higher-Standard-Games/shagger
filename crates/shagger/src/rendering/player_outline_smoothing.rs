//! The outline normals of a rigged body (SHAG `engine/rendering/
//! PlayerOutlineSmoothing.ts`, `smoothPlayerOutlineNormals`).
//!
//! A body's hard edges split a vertex in two with different normals, and an
//! outline pushed out along them tears open along every such seam. The
//! outline draws with normals averaged over the vertices that share a
//! place and the same joints and weights (so they also move together); the
//! lit body keeps its own.

use std::collections::HashMap;

use shagger_shared::js::{js_hypot, js_round};

/// `Math.round(value * 100000)`: the precision the keys compare at.
fn key(value: f32) -> i64 {
    js_round(f64::from(value) * 100_000.0) as i64
}

/// A vertex's place and its joints with their weights, as compared.
type Key = ([i64; 3], Vec<(u32, i64)>);
/// A normal as compared, and as it is.
type DistinctNormal = ([i64; 3], [f32; 3]);

/// `smoothPlayerOutlineNormals`: the averaged normals, or `None` when none
/// of them changes (or the streams disagree in length).
#[must_use]
pub fn smooth_player_outline_normals(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    joints: &[[u32; 4]],
    weights: &[[f32; 4]],
) -> Option<Vec<[f32; 3]>> {
    let count = positions.len();
    if normals.len() != count || joints.len() != count || weights.len() != count {
        return None;
    }
    let mut keys: Vec<Key> = Vec::with_capacity(count);
    // Each place's distinct normals, in the order first seen.
    let mut groups: HashMap<Key, Vec<DistinctNormal>> = HashMap::new();
    for vertex in 0..count {
        let mut influences: Vec<(u32, i64)> = (0..4)
            .filter(|&i| weights[vertex][i] > 0.0)
            .map(|i| (joints[vertex][i], key(weights[vertex][i])))
            .collect();
        // `sort((a, b) => a[0] - b[0])`, stable.
        influences.sort_by_key(|&(joint, _)| joint);
        let p = positions[vertex];
        let k: Key = ([key(p[0]), key(p[1]), key(p[2])], influences);
        let normal = normals[vertex];
        let rounded = [key(normal[0]), key(normal[1]), key(normal[2])];
        let group = groups.entry(k.clone()).or_default();
        // A Map's `set` keeps the first entry's place and the last value.
        match group.iter_mut().find(|(r, _)| *r == rounded) {
            Some(entry) => entry.1 = normal,
            None => group.push((rounded, normal)),
        }
        keys.push(k);
    }
    let mut averaged: HashMap<&Key, [f64; 3]> = HashMap::new();
    for (k, group) in &groups {
        let mut sum = [0.0f64; 3];
        for (_, normal) in group {
            for axis in 0..3 {
                sum[axis] += f64::from(normal[axis]);
            }
        }
        let length = js_hypot(&sum);
        if length > 0.000_001 {
            averaged.insert(k, sum.map(|v| v / length));
        }
    }
    let mut changed = false;
    let result: Vec<[f32; 3]> = keys
        .iter()
        .zip(normals)
        .map(|(k, &normal)| {
            let out = averaged.get(k).map_or(normal, |n| n.map(|v| v as f32));
            if (0..3).any(|axis| (out[axis] - normal[axis]).abs() > 0.0001) {
                changed = true;
            }
            out
        })
        .collect();
    changed.then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_normals_at_one_place_and_skin_are_averaged() {
        let positions = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let normals = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        let joints = [[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0]];
        let weights = [[1.0, 0.0, 0.0, 0.0]; 4];
        let out = smooth_player_outline_normals(&positions, &normals, &joints, &weights).expect("changed");
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((out[0][0] - h).abs() < 1e-6 && (out[0][1] - h).abs() < 1e-6);
        assert_eq!(out[0], out[1]);
        // Its own place, and the same place on another joint: unchanged.
        assert_eq!(out[2], [0.0, 0.0, 1.0]);
        assert_eq!(out[3], [0.0, 0.0, 1.0]);
        // Nothing split: nothing to do.
        assert!(
            smooth_player_outline_normals(&positions[2..3], &normals[2..3], &joints[2..3], &weights[2..3]).is_none()
        );
    }
}
