//! SHAGGER shared code, the Rust counterpart of SHAG's `src/shared`.
//!
//! Nothing here touches the GPU, a window or an async runtime, so the
//! simulation, the protocol codec and their parity tests build anywhere.
//! See `docs/research/RUST.md` in wash-fps-rust, section 14.

pub mod bsp;
pub mod collision;
pub mod demo;
pub mod entities;
pub mod fdlibm;
pub mod js;
pub mod json;
pub mod net;
pub mod server;
pub mod settings;
pub mod spread_utils;
