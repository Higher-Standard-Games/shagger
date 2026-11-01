//! SHAG `src/engine/loader/bsp/mesh/autospriteRecords.ts`: an autosprite
//! face's billboard record and the vertex attributes the shader turns it
//! with (`Q3AutospritePlugin`).
//!
//! `autospriteBounds` is left out until culling needs it.

use super::autosprite2::{Autosprite2Geometry, create_autosprite2_geometry};

/// `AutospriteRecord` (without the mesh).
#[derive(Debug, Clone, PartialEq)]
pub struct AutospriteRecord {
    /// First vertex of this sprite in its batch.
    pub vertex_start: usize,
    pub center: [f64; 3],
    pub half_width: f64,
    pub half_height: f64,
    pub is_autosprite2: bool,
    pub axis_geometry: Option<Autosprite2Geometry>,
    /// Each vertex as `[right, up]` fractions of the half extents.
    pub vertex_offsets: Vec<[f64; 2]>,
}

/// `AutospriteVertexData`: per vertex, four floats each.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutospriteVertexData {
    /// Centre xyz, w: 0 static, 1 autosprite, 2 autosprite2.
    pub center: Vec<f32>,
    pub frame: Vec<f32>,
    pub right: Vec<f32>,
}

/// `autospriteRecord`: the record for one face drawn from `vertex_start`
/// of a batch. `positions` and `center` are the face's own, `batch_positions`
/// and `indices` the batch's (an autosprite2 quad keeps its batch vertex
/// numbers).
#[must_use]
pub fn autosprite_record(
    positions: &[f64],
    center: [f64; 3],
    autosprite2: bool,
    vertex_start: usize,
    batch_positions: &[f64],
    indices: &[u32],
) -> AutospriteRecord {
    let n = positions.len() / 3;
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in 0..n {
        for k in 0..3 {
            // Math.min / Math.max: no NaNs in face positions.
            min[k] = min[k].min(positions[v * 3 + k]);
            max[k] = max[k].max(positions[v * 3 + k]);
        }
    }
    let extent = [0, 1, 2].map(|k| (max[k] - min[k]) / 2.0);
    let half_width = extent[0].max(extent[2]);
    let half_height = if extent[1] > 0.01 {
        extent[1]
    } else {
        extent[0].max(extent[2])
    };
    let offsets: Vec<[f64; 3]> = (0..n)
        .map(|v| [0, 1, 2].map(|k| positions[v * 3 + k] - center[k]))
        .collect();
    let mut me = [0.0f64; 3];
    for o in &offsets {
        for k in 0..3 {
            me[k] = me[k].max(o[k].abs());
        }
    }
    let frac = |value: f64, extent: f64| if extent > 0.001 { value / extent } else { 0.0 };
    let vertex_offsets = offsets
        .iter()
        .map(|&[ox, oy, oz]| {
            if me[1] < me[0] && me[1] < me[2] {
                // Horizontal (XZ plane): Y is the normal.
                [frac(ox, me[0]), frac(oz, me[2])]
            } else if me[0] < me[1] && me[0] < me[2] {
                // YZ plane: X is the normal.
                [frac(oz, me[2]), frac(oy, me[1])]
            } else {
                // XY plane or default: Z is the normal.
                [frac(ox, me[0]), frac(oy, me[1])]
            }
        })
        .collect();
    AutospriteRecord {
        vertex_start,
        center,
        half_width,
        half_height,
        is_autosprite2: autosprite2,
        axis_geometry: autosprite2.then(|| create_autosprite2_geometry(batch_positions, indices)),
        vertex_offsets,
    }
}

/// `autospriteVertexData`: the billboard attributes for a batch's vertices.
#[must_use]
pub fn autosprite_vertex_data(records: &[AutospriteRecord], positions: &[f64]) -> AutospriteVertexData {
    let count = positions.len() / 3;
    let mut data = AutospriteVertexData {
        center: vec![0.0; count * 4],
        frame: vec![0.0; count * 4],
        right: vec![0.0; count * 4],
    };
    let set = |target: &mut Vec<f32>, values: [f64; 4], at: usize| {
        for (k, v) in values.into_iter().enumerate() {
            target[at + k] = v as f32;
        }
    };
    for record in records {
        if record.is_autosprite2 {
            for quad in record.axis_geometry.iter().flat_map(|g| &g.quads) {
                let [cx, cy, cz] = quad.center;
                for &v in &quad.vertices {
                    let v = v as usize;
                    let d = [
                        positions[v * 3] - cx,
                        positions[v * 3 + 1] - cy,
                        positions[v * 3 + 2] - cz,
                    ];
                    let a = quad.axis;
                    let r = quad.right;
                    set(&mut data.center, [cx, cy, cz, 2.0], v * 4);
                    set(
                        &mut data.frame,
                        [a[0], a[1], a[2], d[0] * a[0] + d[1] * a[1] + d[2] * a[2]],
                        v * 4,
                    );
                    set(
                        &mut data.right,
                        [r[0], r[1], r[2], d[0] * r[0] + d[1] * r[1] + d[2] * r[2]],
                        v * 4,
                    );
                }
            }
            continue;
        }
        // As the CPU billboard: a face of fewer than four vertices keeps its shape.
        if record.vertex_offsets.len() < 4 {
            continue;
        }
        let [cx, cy, cz] = record.center;
        for (i, [right_frac, up_frac]) in record.vertex_offsets.iter().enumerate() {
            let at = (record.vertex_start + i) * 4;
            set(&mut data.center, [cx, cy, cz, 1.0], at);
            set(
                &mut data.frame,
                [right_frac * record.half_width, up_frac * record.half_height, 0.0, 0.0],
                at,
            );
        }
    }
    data
}
