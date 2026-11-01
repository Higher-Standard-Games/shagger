//! The SHAGGER engine, the Rust counterpart of SHAG's `src/engine`.
//!
//! Module folders follow SHAG's, so a SHAG path points at its SHAGGER file
//! (`src/engine/loader/pk3/PK3PublishedIndex.ts` → `loader/pk3/published_index.rs`).
//! The plan is `docs/research/RUST.md` in wash-fps-rust.
//!
//! Licensing: [`babylon`] holds code ported from Babylon.js and is
//! Apache-2.0; everything else in this crate is MIT.

pub mod babylon;
pub mod loader;
pub mod network;
pub mod physics;

pub use shagger_shared as shared;
