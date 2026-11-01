//! The renderer (RUST.md 8.2 and 21): wgpu in place of Babylon's scene.
//!
//! This is the first slice of milestone 6: the map's world surfaces with
//! their base textures and baked lighting, drawn from a free camera, to a
//! window or to an offscreen image. Q3 shader stages, PVS, models
//! and post-processing follow (RUST.md 21.10).

pub use glam;
pub use wgpu;

pub mod camera;
pub mod gpu;
pub mod offscreen;
pub mod sky_renderer;
pub mod world_geometry;
pub mod world_materials;
pub mod world_renderer;
