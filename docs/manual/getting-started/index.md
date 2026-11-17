# Getting started

## Requirements

- **Rust 1.95 or later.** `rust-toolchain.toml` selects the stable channel with rustfmt and clippy, and the workspace's `rust-version` is 1.95, which CI checks.
- **On Linux:** ALSA's and udev's development packages, for cpal (sound) and gilrs (gamepads). On Debian and Ubuntu:

  ```sh
  sudo apt-get install -y libasound2-dev libudev-dev
  ```

- **A GPU, for some tests.** wgpu finds a hardware or a software adapter (llvmpipe, WARP). The tests that draw report a skip and pass when there is none.
- **Node 22.18 or later and a SHAG checkout**, only to regenerate golden vectors (see [Golden vectors](../tools/golden-vectors.md)).

## Build and test

```sh
git clone https://github.com/Higher-Standard-Games/shagger
cd shagger
cargo build --workspace
cargo test --workspace
```

The workspace has two crates, `shagger_shared` and `shagger` ([Project structure](./project-structure.md)). Neither has a binary: the programs are the games built on them.

Some tests read data that is not in the repository. They pass without it and say so:

| Variable | What the test reads |
| --- | --- |
| `SHAGGER_PLAYER_DATA` | a folder holding `washoi_player_model.pk3`, for `tests/player_model_golden.rs` |
| `SHAGGER_TEST_FONT` | a TrueType or OpenType file, for the font and `Hud2d` tests |
| `SHAGGER_TEST_MP3` | an MP3 file, for the music decoder test |

The checks CI runs, and the ones to run before pushing, are listed in [Contributing](../contributing/index.md#checks).

## Use it from a game

A game depends on both crates by git revision. wash-fps-rust pins one in its `Cargo.toml`:

```toml
[dependencies]
shagger = { git = "https://github.com/Higher-Standard-Games/shagger", rev = "<commit>" }
```

`shagger` re-exports `shagger_shared` as `shagger::shared`, and the renderer re-exports `wgpu` and `glam` as `shagger::rendering::wgpu` and `shagger::rendering::glam`, so a game builds against the same versions the engine does.

To move the pin, change `rev` and run `cargo update -p shagger`.

### Working on the engine and a game together

Clone shagger next to the game and point Cargo at the local copy in the game's `.cargo/config.toml` (wash-fps-rust keeps that file out of git):

```toml
[patch."https://github.com/Higher-Standard-Games/shagger"]
shagger = { path = "../shagger/crates/shagger" }
shagger_shared = { path = "../shagger/crates/shagger_shared" }
```

The patch also rewrites the game's `Cargo.lock`. Do not commit that change: land the engine change in shagger first, then move the game's pin to the merged commit with the patch set aside, so the lock file names the git revision.

## Next

- [Project structure](./project-structure.md): where each SHAG folder landed.
- [Guides](../guides/index.md): each subsystem's API.
