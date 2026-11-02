//! The renderer (RUST.md 8.2 and 21): wgpu in place of Babylon's scene.
//!
//! Milestone 6 so far: the map's world surfaces with their Q3 shader stages,
//! baked lighting, animation and autosprites, the far box sky, PVS and
//! frustum culling, and glTF models lit by the scene's hemispheric and
//! directional lights (the map's weapon pickups), drawn from a free camera
//! to a window or to an offscreen image. Skinned players, MD3, effects and
//! post-processing follow (RUST.md 21.10).

pub use glam;
pub use wgpu;

pub mod camera;
pub mod effect_quad_batch;
pub mod frustum;
pub mod gpu;
pub mod model_renderer;
pub mod offscreen;
pub mod sky_renderer;
pub mod world_culling;
pub mod world_geometry;
pub mod world_materials;
pub mod world_renderer;
