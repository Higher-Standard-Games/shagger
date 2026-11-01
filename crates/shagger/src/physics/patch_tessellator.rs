//! Bezier patch tessellation for collision (SHAG
//! `src/engine/physics/PatchTessellator.ts`). Output is in Babylon axes
//! (y up), as the TypeScript builds it.

use crate::babylon::maths::math_vector::Vector3;
use crate::loader::bsp::bsp_loader::BspFace;
use shagger_shared::js::{js_max, js_min};

#[derive(Debug, Clone, PartialEq)]
pub struct PatchCollisionMesh {
    pub vertices: Vec<Vector3>,
    pub indices: Vec<usize>,
    pub mins: Vector3,
    pub maxs: Vector3,
}

fn quadratic(a: f64, b: f64, c: f64, u: f64) -> f64 {
    if u == 0.0 {
        return a;
    }
    if u == 1.0 {
        return c;
    }
    let ab = a + (b - a) * u;
    let bc = b + (c - b) * u;
    ab + (bc - ab) * u
}

fn evaluate_bezier(ctrl: &[[[f64; 3]; 3]; 3], s: f64, t: f64) -> [f64; 3] {
    std::array::from_fn(|axis| {
        quadratic(
            quadratic(ctrl[0][0][axis], ctrl[0][1][axis], ctrl[0][2][axis], s),
            quadratic(ctrl[1][0][axis], ctrl[1][1][axis], ctrl[1][2][axis], s),
            quadratic(ctrl[2][0][axis], ctrl[2][1][axis], ctrl[2][2][axis], s),
            t,
        )
    })
}

/// `PatchTessellator.buildPatch` over the collision vertex slots (patch
/// control points at their file index). `None` for a patch under 3×3 or one
/// missing a control point.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn build_patch(vertices: &[Option<[f32; 3]>], face: &BspFace, level: usize) -> Option<PatchCollisionMesh> {
    let (width, height) = (face.patch_size[0], face.patch_size[1]);
    if width < 3 || height < 3 {
        return None;
    }
    let mut control: Vec<Vec<[f64; 3]>> = Vec::new();
    for row in 0..height {
        let mut points = Vec::new();
        for col in 0..width {
            let idx = i64::from(face.first_vertex) + i64::from(row) * i64::from(width) + i64::from(col);
            let v = usize::try_from(idx)
                .ok()
                .and_then(|i| vertices.get(i).copied().flatten())?;
            // Q3 (x, y, z) to Babylon (x, z, y).
            points.push([f64::from(v[0]), f64::from(v[2]), f64::from(v[1])]);
        }
        control.push(points);
    }

    let mut out = PatchCollisionMesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        mins: Vector3::new(f64::MAX, f64::MAX, f64::MAX),
        maxs: Vector3::new(-f64::MAX, -f64::MAX, -f64::MAX),
    };
    // Both are at least 3 here.
    #[allow(clippy::cast_sign_loss)]
    let (patches_x, patches_y) = (((width - 1) / 2) as usize, ((height - 1) / 2) as usize);
    let mut vertex_index = 0;
    for py in 0..patches_y {
        for px in 0..patches_x {
            let ctrl: [[[f64; 3]; 3]; 3] =
                std::array::from_fn(|j| std::array::from_fn(|i| control[py * 2 + j][px * 2 + i]));
            let start = vertex_index;
            for v in 0..=level {
                for u in 0..=level {
                    let pos = evaluate_bezier(&ctrl, u as f64 / level as f64, v as f64 / level as f64);
                    out.vertices.push(Vector3::new(pos[0], pos[1], pos[2]));
                    out.mins = Vector3::new(
                        js_min(out.mins.x, pos[0]),
                        js_min(out.mins.y, pos[1]),
                        js_min(out.mins.z, pos[2]),
                    );
                    out.maxs = Vector3::new(
                        js_max(out.maxs.x, pos[0]),
                        js_max(out.maxs.y, pos[1]),
                        js_max(out.maxs.z, pos[2]),
                    );
                    vertex_index += 1;
                }
            }
            for v in 0..level {
                for u in 0..level {
                    let i00 = start + v * (level + 1) + u;
                    let (i10, i01) = (i00 + 1, i00 + level + 1);
                    let i11 = i01 + 1;
                    out.indices.extend_from_slice(&[i00, i10, i01, i10, i11, i01]);
                }
            }
        }
    }
    Some(out)
}
