//! SHAG `src/engine/loader/bsp/mesh/types.ts`: `FaceGeometry`. The other types
//! there describe Babylon meshes and stay with the renderer.

/// One face's vertex attributes, flat, in Babylon space. Numbers are `f64`
/// like the TypeScript arrays, so they can be compared bit for bit; the
/// renderer narrows them to `f32` on upload, as Babylon's `Float32Array` does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceGeometry {
    /// xyz per vertex.
    pub positions: Vec<f64>,
    /// xyz per vertex.
    pub normals: Vec<f64>,
    /// Texture uv per vertex.
    pub uvs: Vec<f64>,
    /// Lightmap uv per vertex.
    pub uv2s: Vec<f64>,
    /// rgba in 0..1 per vertex.
    pub colors: Vec<f64>,
    pub indices: Vec<u32>,
}

impl FaceGeometry {
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }
}
