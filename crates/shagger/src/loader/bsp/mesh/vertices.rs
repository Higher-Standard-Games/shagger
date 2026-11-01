//! SHAG `src/engine/loader/bsp/mesh/vertices.ts`: one BSP vertex's attributes
//! in Babylon space. FBSP vertices give their first lightmap slot.

use super::coordinates::q3_to_babylon;
use crate::loader::bsp::bsp_loader::{BspVertexFbsp, BspVertexIbsp, BspVertices};

/// A vertex of either layout.
#[derive(Debug, Clone, Copy)]
pub enum BspVertex<'a> {
    Ibsp(&'a BspVertexIbsp),
    Fbsp(&'a BspVertexFbsp),
}

impl<'a> BspVertex<'a> {
    /// `bsp.vertices[index]`.
    #[must_use]
    pub fn at(vertices: &'a BspVertices, index: usize) -> Option<Self> {
        match vertices {
            BspVertices::Ibsp(v) => v.get(index).map(Self::Ibsp),
            BspVertices::Fbsp(v) => v.get(index).map(Self::Fbsp),
        }
    }

    fn position_raw(self) -> [f32; 3] {
        match self {
            Self::Ibsp(v) => v.position,
            Self::Fbsp(v) => v.position,
        }
    }

    fn normal_raw(self) -> [f32; 3] {
        match self {
            Self::Ibsp(v) => v.normal,
            Self::Fbsp(v) => v.normal,
        }
    }
}

/// `getVertexPosition`.
#[must_use]
pub fn get_vertex_position(v: BspVertex<'_>) -> [f64; 3] {
    let p = v.position_raw();
    q3_to_babylon(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

/// `getVertexNormal`.
#[must_use]
pub fn get_vertex_normal(v: BspVertex<'_>) -> [f64; 3] {
    let n = v.normal_raw();
    q3_to_babylon(f64::from(n[0]), f64::from(n[1]), f64::from(n[2]))
}

/// `getVertexTexCoord`.
#[must_use]
pub fn get_vertex_tex_coord(v: BspVertex<'_>) -> [f64; 2] {
    let t = match v {
        BspVertex::Ibsp(v) => v.tex_coord,
        BspVertex::Fbsp(v) => v.tex_coord,
    };
    [f64::from(t[0]), f64::from(t[1])]
}

/// `getVertexLightmapCoord`.
#[must_use]
pub fn get_vertex_lightmap_coord(v: BspVertex<'_>) -> [f64; 2] {
    let l = match v {
        BspVertex::Ibsp(v) => v.lightmap_coord,
        BspVertex::Fbsp(v) => v.lightmap_coords[0],
    };
    [f64::from(l[0]), f64::from(l[1])]
}

/// `getVertexColor`: 0..1.
#[must_use]
pub fn get_vertex_color(v: BspVertex<'_>) -> [f64; 4] {
    let c = match v {
        BspVertex::Ibsp(v) => v.color,
        BspVertex::Fbsp(v) => v.colors[0],
    };
    c.map(|x| f64::from(x) / 255.0)
}
