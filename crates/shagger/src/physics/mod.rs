//! Map collision (SHAG `src/engine/physics`). The Q3 brush and patch sweep
//! the simulation runs on, its adapter, and the player hull live in
//! `shagger_shared::physics`, so a headless game simulation can use them; each
//! is re-exported here at its engine path. `physics_system` is the engine's
//! own.

pub mod collision_backend;
pub mod collision_spatial_index;
pub mod content_flags;
pub mod facet_bevels;
pub mod patch_tessellator;
pub mod physics_system;
pub mod player_physics_constants;
pub mod sweep_backend_adapter;
pub mod sweep_collision_backend;
