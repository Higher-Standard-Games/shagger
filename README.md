# SHAGGER

The Rust counterpart of the [SHAG](https://github.com/Higher-Standard-Games/shag)
engine. [wash-fps-rust](https://github.com/Higher-Standard-Games/wash-fps-rust),
the native Washoi client, is built on it, as wash-fps is built on SHAG.

The plan, its findings and the milestones are in wash-fps-rust's
`docs/research/RUST.md`. Section numbers below refer to it.

## Crates

| Crate | SHAG counterpart | Rule |
| --- | --- | --- |
| `shagger_shared` | `src/shared` | no GPU, no window, no async |
| `shagger` | `src/engine` (minus editor and server), `src/lobby` client | depends on `shagger_shared` only |

Folder names follow SHAG's, so `src/engine/loader/pk3/PK3PublishedIndex.ts`
lives at `crates/shagger/src/loader/pk3/published_index.rs`. Port the
TypeScript as directly as Rust allows, and say in a comment where and why the
Rust differs.

## Licenses

- Everything is MIT (`LICENSE`), except:
- `crates/shagger/src/babylon/`: code ported or copied from Babylon.js, under
  the Apache License 2.0 (`LICENSE` and `NOTICE` in that folder). Babylon code
  goes in that folder and nowhere else, each file with an Apache header naming
  its Babylon source (21.1).
- `cargo deny check` enforces the dependency license allow-list in CI.

## Parity with the TypeScript

The Rust has to give the same results as SHAG, bit for bit where the
simulation and the wire format are concerned. Results computed by V8 and by
SHAG's own code are checked in as golden vectors:

```sh
node tools/golden/generate.mjs ../shag    # Node 22.18+, a SHAG checkout
cargo test
```

Each golden file names the SHAG commit it was made from. Regenerate them when
the TypeScript changes, and commit them with the Rust change they test.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```
