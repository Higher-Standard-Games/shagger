//! SHAGGER shared code, the Rust counterpart of SHAG's `src/shared`.
//!
//! Nothing here touches the GPU, a window or an async runtime, so the
//! simulation, the protocol codec and their parity tools build anywhere.
//! See `docs/research/RUST.md` in wash-fps-rust, section 14.

pub mod js;
