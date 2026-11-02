//! SHAG `src/engine/loader/bsp/mesh`: turning BSP faces into vertex and index
//! arrays, and lightmap pages into texture bytes. The Babylon mesh and texture
//! objects are left out; the renderer uploads what these return.

pub mod autosprite2;
pub mod autosprite_records;
pub mod coordinates;
pub mod face_builders;
pub mod lightmaps;
pub mod pvs;
pub mod tangents;
pub mod types;
pub mod vertices;
