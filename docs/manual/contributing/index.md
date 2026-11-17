# Contributing

The short form of these rules is in [`AGENTS.md`](../../../AGENTS.md). Read wash-fps-rust's [`docs/research/RUST.md`](https://github.com/Higher-Standard-Games/wash-fps-rust/blob/main/docs/research/RUST.md) before a structural change: it is the plan, and it is updated when a decision changes.

## Porting rules

1. **Port one to one where Rust allows.** Keep SHAG's folder and file names, in snake case, and the TypeScript's function names. Where the Rust has to differ (ownership, no singletons, I/O the browser did), keep the logic the same and say in a comment where and why.
2. **Name the source.** Every ported module starts with a doc comment that names its TypeScript file (`` //! SHAG `src/engine/core/Cvar.ts`: ... ``). A module with no TypeScript source says that instead.
3. **Match V8 in simulation and protocol code.** Use `shagger_shared::js` wherever the TypeScript calls `Math.round`, `Math.hypot`, `Math.sin`, `Math.cos` or `Math.atan2`, or converts a number for `DataView` ([JavaScript parity](../guides/js-parity.md)). Keep `f64` where JavaScript computes.
4. **Prove it with golden vectors.** A port of simulation, protocol, parsing or reel code comes with vectors from the TypeScript ([Golden vectors](../tools/golden-vectors.md)).
5. **Fix the TypeScript first.** When a port finds a bug in SHAG, the fix lands in SHAG, then the port follows it and the vectors are regenerated. The two stay alike.
6. **Engine code goes here, game code goes in the game.** Anything not specific to Washoi belongs in SHAGGER; Washoi's events, simulation, screens and HUD belong in wash-fps-rust (RUST.md 14.1).
7. **Keep `shagger_shared` plain.** It must not depend on GPU, windowing or async crates.
8. **Never block a frame.** Sockets, downloads, disk writes and the sound card run on threads of their own and report through channels or lock-free queues. A thread that may hang is joined with a limit ([Native runtime](../guides/native-runtime.md#bounded-joins)).
9. **Never panic on data from outside.** Packages and network bytes go through parsers that return errors. A new parser joins the fuzzer ([Tools](../tools/index.md#parser-fuzzing)).

## Checks

Before pushing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

CI runs three jobs on every pull request:

| Job | Runs |
| --- | --- |
| `check` | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` on stable, with warnings as errors |
| `msrv` | `cargo check --workspace --all-targets` on Rust 1.95 |
| `deny` | `cargo deny check` |

The workspace lints deny `unsafe_code` and warn on all of clippy's default lints, which CI turns into errors.

## Licences

- Everything is MIT ([`LICENSE`](../../../LICENSE)), except the two below.
- **Babylon.js code** goes in `crates/shagger/src/babylon/` and nowhere else, under the Apache License 2.0 (`LICENSE` and `NOTICE` in that folder). Each file starts with an Apache-2.0 header naming the Babylon file and version it came from, and the changes made, and is listed in `NOTICE`. The rest of SHAGGER may call into it but never copies from it (RUST.md 21.1).
- **`crates/shagger_shared/src/fdlibm.rs`** is V8's `Math.sin`, `Math.cos` and `Math.atan2`, ported from Sun's fdlibm, under fdlibm's permissive notice, kept at the top of the file.
- **Dependencies** must carry a licence on the allow-list in `deny.toml` (MIT, Apache-2.0, the BSD licences, ISC, Zlib, Unicode-3.0, BSL-1.0, CC0-1.0 and CDLA-Permissive-2.0 for the Mozilla roots). `cargo deny check` enforces it, along with the advisory database. When a crate's licence does not fit, look for another: audio decoding uses lewton rather than symphonia (MPL-2.0), and music uses rmp3 rather than the `minimp3` crate (RUSTSEC-2025-0044).

## Documentation

This manual covers what is particular to the Rust port. When a change alters behaviour that SHAG's manual documents, it changed in SHAG first, so SHAG's manual is where it is described. Update a page here when the Rust API, a native backend, a build step or a deliberate difference from SHAG changes, and check every command, path and API name against the code.
