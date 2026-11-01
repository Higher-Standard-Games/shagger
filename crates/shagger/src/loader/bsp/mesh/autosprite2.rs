//! SHAG `src/engine/loader/bsp/mesh/autosprite2.ts`: the quads of an
//! `autosprite2` face, each with the long axis it turns about.
//!
//! Only `createAutosprite2Geometry`: the vertex shader does what
//! `updateAutosprite2Geometry` does on the CPU.

use crate::shared::js::js_hypot;

/// One quad: its centre, the long axis, the short side and its vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Autosprite2Quad {
    pub center: [f64; 3],
    pub axis: [f64; 3],
    pub right: [f64; 3],
    /// Vertex numbers in the batch, first appearance order.
    pub vertices: Vec<u32>,
}

/// `Autosprite2Geometry`, without the copy of the positions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Autosprite2Geometry {
    pub quads: Vec<Autosprite2Quad>,
}

/// An edge: from, to, the vector between and its length.
type Edge = ([f64; 3], [f64; 3], [f64; 3], f64);

/// `createAutosprite2Geometry`: every six indices are a quad.
#[must_use]
pub fn create_autosprite2_geometry(positions: &[f64], indices: &[u32]) -> Autosprite2Geometry {
    let point = |i: u32| {
        let i = i as usize * 3;
        [positions[i], positions[i + 1], positions[i + 2]]
    };
    let mut quads = Vec::new();
    let mut i = 0;
    while i + 5 < indices.len() {
        // The source triangle order, reversed at the Q3-to-Babylon boundary.
        let triangle = [indices[i], indices[i + 2], indices[i + 1]];
        let mut edges: Vec<Edge> = [(1, 0), (2, 0), (2, 1)]
            .iter()
            .map(|&(a, b)| {
                let from = point(triangle[b]);
                let to = point(triangle[a]);
                let vector = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
                (from, to, vector, js_hypot(&vector))
            })
            .collect();
        // A stable sort, longest first (Array.prototype.sort is stable).
        edges.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        let (diagonal, long, short) = (edges[0], edges[1], edges[2]);
        let elements = &indices[i..i + 6];
        i += 6;
        if !diagonal.3.is_finite() || short.3 < 0.0001 {
            continue;
        }
        let axis = long.2.map(|v| v / long.3);
        let right = short.2.map(|v| v / short.3);
        let dot = axis[0] * right[0] + axis[1] * right[1] + axis[2] * right[2];
        if dot.abs() > 0.9999 {
            continue;
        }
        let mut vertices = Vec::new();
        for &e in elements {
            if !vertices.contains(&e) {
                vertices.push(e);
            }
        }
        quads.push(Autosprite2Quad {
            center: [0, 1, 2].map(|j| (diagonal.0[j] + diagonal.1[j]) * 0.5),
            axis,
            right,
            vertices,
        });
    }
    Autosprite2Geometry { quads }
}
