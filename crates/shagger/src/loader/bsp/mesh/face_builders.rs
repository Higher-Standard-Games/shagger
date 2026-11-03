//! SHAG `src/engine/loader/bsp/mesh/faceBuilders.ts`: polygon, mesh and patch
//! faces as [`FaceGeometry`].
//!
//! Where the TypeScript reads past the vertex or mesh-vertex lump (a corrupt
//! file) it fills the arrays with `undefined`; these return `None` instead.

use super::types::FaceGeometry;
use super::vertices::{
    BspVertex, get_vertex_color, get_vertex_lightmap_coord, get_vertex_normal, get_vertex_position,
    get_vertex_tex_coord,
};
use crate::loader::bsp::bsp_loader::{BspFace, BspFile};

/// Patch tessellation: 8 steps per sub-patch (`tessLevel`).
pub const PATCH_TESS_LEVEL: usize = 8;

fn vertex(bsp: &BspFile, index: i64) -> Option<BspVertex<'_>> {
    BspVertex::at(&bsp.vertices, usize::try_from(index).ok()?)
}

/// `buildPolygonFace` (type 1): the face's vertices, and its mesh vertices as
/// triangles with the winding reversed.
#[must_use]
pub fn build_polygon_face(bsp: &BspFile, face: &BspFace) -> Option<FaceGeometry> {
    let mut g = FaceGeometry::default();
    for i in 0..i64::from(face.num_vertices.max(0)) {
        let v = vertex(bsp, i64::from(face.first_vertex) + i)?;
        g.positions.extend(get_vertex_position(v));
        g.normals.extend(get_vertex_normal(v));
        g.uvs.extend(get_vertex_tex_coord(v));
        g.uv2s.extend(get_vertex_lightmap_coord(v));
        g.colors.extend(get_vertex_color(v));
    }
    let mut raw = Vec::with_capacity(usize::try_from(face.num_mesh_verts.max(0)).unwrap_or(0));
    for i in 0..i64::from(face.num_mesh_verts.max(0)) {
        let at = usize::try_from(i64::from(face.first_mesh_vert) + i).ok()?;
        raw.push(u32::try_from(*bsp.mesh_verts.get(at)?).ok()?);
    }
    // `for (i = 0; i < raw.length; i += 3)` reads past a partial triangle.
    // An index past the face's own vertices would draw another face's (or
    // none, and the tangents would index out of range): the face is dropped.
    let count = u32::try_from(face.num_vertices.max(0)).unwrap_or(0);
    if raw.len() % 3 != 0 || raw.iter().any(|&i| i >= count) {
        return None;
    }
    for t in raw.as_chunks::<3>().0.iter() {
        g.indices.extend([t[0], t[2], t[1]]);
    }
    Some(g)
}

/// `buildMeshFace` (type 3): the same as a polygon.
#[must_use]
pub fn build_mesh_face(bsp: &BspFile, face: &BspFace) -> Option<FaceGeometry> {
    build_polygon_face(bsp, face)
}

/// One tessellated patch vertex (`TessVert`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TessVert {
    pub pos: [f64; 3],
    pub normal: [f64; 3],
    pub uv: [f64; 2],
    pub uv2: [f64; 2],
    pub color: [f64; 4],
}

/// `buildPatchFace` (type 2): a biquadratic Bezier surface, tessellated on one
/// vertex grid shared by all its 3×3 sub-patches so neighbours meet without
/// cracks. A patch smaller than 3×3 is empty, as in the TypeScript.
#[must_use]
pub fn build_patch_face(bsp: &BspFile, face: &BspFace) -> Option<FaceGeometry> {
    let mut g = FaceGeometry::default();
    let (width, height) = (face.patch_size[0], face.patch_size[1]);
    if width < 3 || height < 3 {
        return Some(g);
    }
    let (width, height) = (width as usize, height as usize);
    let mut control = Vec::with_capacity(width * height);
    for row in 0..height {
        for col in 0..width {
            control.push(vertex(bsp, i64::from(face.first_vertex) + (row * width + col) as i64)?);
        }
    }
    let tess = PATCH_TESS_LEVEL;
    let (patches_x, patches_y) = ((width - 1) / 2, (height - 1) / 2);
    let grid_w = patches_x * tess + 1;
    let grid_h = patches_y * tess + 1;
    let mut grid = vec![TessVert::default(); grid_w * grid_h];

    for py in 0..patches_y {
        for px in 0..patches_x {
            let ctrl: [[BspVertex<'_>; 3]; 3] =
                std::array::from_fn(|j| std::array::from_fn(|i| control[(py * 2 + j) * width + px * 2 + i]));
            let u_start = usize::from(px != 0);
            let v_start = usize::from(py != 0);
            for v in v_start..=tess {
                for u in u_start..=tess {
                    let s = u as f64 / tess as f64;
                    let t = v as f64 / tess as f64;
                    grid[(py * tess + v) * grid_w + px * tess + u] = evaluate_bezier_patch(&ctrl, s, t);
                }
            }
        }
    }

    for vert in &grid {
        g.positions.extend(vert.pos);
        g.normals.extend(vert.normal);
        g.uvs.extend(vert.uv);
        g.uv2s.extend(vert.uv2);
        g.colors.extend(vert.color);
    }
    for gy in 0..grid_h - 1 {
        for gx in 0..grid_w - 1 {
            let i00 = (gy * grid_w + gx) as u32;
            let i10 = i00 + 1;
            let i01 = i00 + grid_w as u32;
            let i11 = i01 + 1;
            g.indices.extend([i00, i10, i01, i10, i11, i01]);
        }
    }
    Some(g)
}

/// `evaluateBezierPatch`: every attribute weighted by the Bernstein basis at
/// `(s, t)`; the normal is then normalized, or straight up when it vanishes.
#[must_use]
pub fn evaluate_bezier_patch(ctrl: &[[BspVertex<'_>; 3]; 3], s: f64, t: f64) -> TessVert {
    let bs = [(1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s), s * s];
    let bt = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let mut o = TessVert::default();
    for (j, row) in ctrl.iter().enumerate() {
        for (i, &cp) in row.iter().enumerate() {
            let w = bs[i] * bt[j];
            let pos = get_vertex_position(cp);
            let norm = get_vertex_normal(cp);
            let tex = get_vertex_tex_coord(cp);
            let lm = get_vertex_lightmap_coord(cp);
            let col = get_vertex_color(cp);
            fn add<const N: usize>(acc: &mut [f64; N], x: [f64; N], w: f64) {
                for (a, x) in acc.iter_mut().zip(x) {
                    *a += x * w;
                }
            }
            add(&mut o.pos, pos, w);
            add(&mut o.normal, norm, w);
            add(&mut o.uv, tex, w);
            add(&mut o.uv2, lm, w);
            add(&mut o.color, col, w);
        }
    }
    let [nx, ny, nz] = o.normal;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    o.normal = if len > 0.0001 {
        [nx / len, ny / len, nz / len]
    } else {
        [0.0, 1.0, 0.0]
    };
    o
}
