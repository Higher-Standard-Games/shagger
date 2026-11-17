# Golden vectors

Golden vectors are results computed by V8 and by SHAG's own TypeScript, checked into the repository, which the Rust tests must reproduce bit for bit (RUST.md 18.6). They are how a port is proven: a ported function is done when its golden test passes on vectors from the TypeScript it ports.

## Regenerating

```sh
node tools/golden/generate.mjs <path to a SHAG checkout> [<path to wash-fps-rust>]
cargo test --workspace
```

- **Node 22.18 or later**, for TypeScript type stripping. The scripts import SHAG's `.ts` files directly through `tools/golden/ts-register.mjs`.
- **The SHAG checkout needs its dependencies** (`npm ci` in it): the collision and player model scripts import Babylon from SHAG's `node_modules`.
- **wash-fps-rust** receives Washoi's own vectors (its snapshot events and movement). It defaults to `../wash-fps-rust` next to the shagger checkout. The movement vectors run wash-fps's own simulation, so wash-fps must be checked out at `<shag>/src/games/fps`, as SHAG's submodule.
- **`WASHOI_MAPS`**, when set to a folder of the shipped BSPs, also regenerates the vectors taken from the shipped maps and packages (movement on the maps, rendering, shaders, KTX, stage evaluation, autosprites, skyboxes, PVS, pickups). wash-fps-rust's `tools/fetch_maps.sh` fills such a folder from the published data. Without it, those files are left as they are.

The generator is deterministic: a rerun on the same SHAG commit writes the same files. Each file names the SHAG commit and the Node version it was made with. Commit regenerated vectors together with the Rust change they test, and say in the commit which SHAG commit they came from.

## What is checked where

Values are written as IEEE 754 bit patterns in hex, so a test compares exact bits, not decimal approximations.

| Script | Writes | Checked by |
| --- | --- | --- |
| `generate.mjs` itself | `js_math.txt`, `js_trig.txt`, `js_atan2.txt`, `js_string_number.txt` | `shagger_shared/tests/js_*_golden.rs` |
| `generate.mjs` itself | `pk3_index_hash.txt`, `water_noise.txt` | `shagger/tests/pk3_index_hash_golden.rs`, `water_noise_golden.rs` |
| `protocol.mjs` | `commands.json`, `messages.json`, `snapshots.json`, `snapshot_envelopes.json`, `strings.json`, `demos.json`, and Washoi's event vectors | `shagger_shared/tests/protocol_golden.rs`, `demo_golden.rs`; wash-fps-rust |
| `highlights.mjs` | `highlights.json` | `shagger_shared/tests/highlights_golden.rs` |
| `bsp.mjs` | `bsp_loader.json` | `shagger/tests/bsp_loader_golden.rs` |
| `collision.mjs` | `collision.json` | `shagger/tests/collision_golden.rs` |
| `movement.mjs`, `movement_maps.mjs` | Washoi's movement vectors | wash-fps-rust |
| `render_maps.mjs`, `shaders.mjs`, `shader_materials.mjs`, `ktx.mjs`, `stage_evaluation.mjs`, `autosprites.mjs`, `skybox.mjs`, `pvs.mjs`, `pickups.mjs` | vectors from the shipped maps | wash-fps-rust |
| `player_model.mts`, `player_model_layers.mts` | `player_model_retarget.json`, `player_model_retarget_skew.json`, `player_model_layers.json` | `shagger/tests/player_model_golden.rs` |

`maps.mjs` builds the synthetic IBSP maps the collision and movement vectors run on, record by record from a seeded random source. `movement_run.mjs` is the shared runner of the movement scripts.

The two player model scripts are not run by `generate.mjs`. They need the player model package unpacked:

```sh
cd tools/golden
node --experimental-transform-types --no-warnings --import ./ts-register.mjs \
    player_model.mts <shag> <washoi_player_model.pk3 unpacked> ../../crates/shagger/tests/golden
```

## Running one script

Each script can be run on its own, from `tools/golden`, with the arguments `generate.mjs` gives it: the SHAG checkout, the output folder and the SHAG commit, plus a maps folder for the ones that read the shipped maps.

```sh
cd tools/golden
node --experimental-transform-types --no-warnings --import ./ts-register.mjs \
    highlights.mjs <shag> ../../crates/shagger_shared/tests/golden "$(git -C <shag> rev-parse HEAD)"
```

## Writing a new one

1. Build the inputs in the script from a seeded generator, so a rerun gives the same file, and include the edge cases (zeros, ties, empty input, damaged input).
2. Call SHAG's own function on them. Do not reimplement it in the script.
3. Write the outputs as bit patterns, with a header naming the generator and the SHAG commit.
4. Add the script to `generate.mjs`, and a test under `crates/*/tests/` that reads the file and compares every value.
