# Instructions for agents

- Read wash-fps-rust's `docs/research/RUST.md` before structural changes. It
  is the plan; update it when a decision changes.
- Port SHAG TypeScript one to one where Rust allows. Keep SHAG's folder and
  file names (snake_case), and name the TypeScript source at the top of each
  ported module.
- Simulation and protocol code must match V8 bit for bit: use
  `shagger_shared::js` (`js_round`, `js_hypot`) instead of `f64::round` or
  `f64::hypot` wherever the TypeScript calls `Math.round` or `Math.hypot`.
  Prove each port with golden vectors from `tools/golden/generate.mjs`.
- Code ported or copied from Babylon.js goes in `crates/shagger/src/babylon/`
  only, with an Apache-2.0 header and a `NOTICE` entry. Everything else is MIT.
- `shagger_shared` must not depend on GPU, windowing or async crates.
- Before pushing: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`.
