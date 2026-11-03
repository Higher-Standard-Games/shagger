//! Map collision (SHAG `src/engine/physics`): the Q3 brush and patch sweep the
//! simulation runs on, and the adapter that hands it to `shagger_shared`.

// Index loops and negated comparisons are the TypeScript's own: the indices
// cross several parallel arrays, and `!(a < b)` is true for NaN.
#![allow(clippy::needless_range_loop, clippy::neg_cmp_op_on_partial_ord)]

pub mod collision_backend;
pub mod collision_spatial_index;
pub mod content_flags;
pub mod facet_bevels;
pub mod patch_tessellator;
pub mod physics_system;
pub mod player_physics_constants;
pub mod sweep_backend_adapter;
pub mod sweep_collision_backend;
