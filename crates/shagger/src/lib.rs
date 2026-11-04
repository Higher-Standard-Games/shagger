//! The SHAGGER engine, the Rust counterpart of SHAG's `src/engine`.
//!
//! Module folders follow SHAG's, so a SHAG path points at its SHAGGER file
//! (`src/engine/loader/pk3/PK3PublishedIndex.ts` → `loader/pk3/published_index.rs`).
//! The plan is `docs/research/RUST.md` in wash-fps-rust.
//!
//! Licensing: [`babylon`] holds code ported from Babylon.js and is
//! Apache-2.0; everything else in this crate is MIT.

pub mod animation;
pub mod audio;
pub mod babylon;
pub mod camera;
pub mod console;
pub mod core;
pub mod debug;
pub mod effects;
pub mod input;
pub mod lighting;
pub mod loader;
pub mod lobby;
pub mod movement;
pub mod network;
pub mod physics;
pub mod rendering;
pub mod ui;
pub mod viewport;
pub mod weapon;
#[cfg(feature = "webview")]
pub mod webview;
pub mod world;

pub use shagger_shared as shared;
