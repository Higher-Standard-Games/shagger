# Project structure

```text
shagger/
├── Cargo.toml              workspace: members, edition 2024, rust-version, lints
├── deny.toml               cargo deny: licences and advisories
├── crates/
│   ├── shagger_shared/     SHAG src/shared: no GPU, no window, no async
│   │   ├── src/
│   │   └── tests/          golden tests, tests/golden/ holds the vectors
│   └── shagger/            SHAG src/engine (minus editor and server), src/lobby client
│       ├── src/
│       └── tests/          golden, offscreen and fuzz tests
├── tools/golden/           Node scripts that write the golden vectors from SHAG
└── docs/manual/            this manual
```

## The two crates

| Crate | SHAG counterpart | Rule |
| --- | --- | --- |
| `shagger_shared` | `src/shared` | Depends on nothing that touches a GPU, a window or an async runtime. Its only dependency is `flate2` (gzip demos). It can serve a Rust game server or a WebAssembly build without changes. |
| `shagger` | `src/engine` (minus the editor and the server), the client side of `src/lobby` | Depends on `shagger_shared` and the native backends. |

`unsafe_code` is denied in both.

## Paths follow SHAG's

Module folders follow SHAG's, and file names are SHAG's in snake case, so a SHAG path points at its SHAGGER file:

| SHAG | SHAGGER |
| --- | --- |
| `src/engine/loader/pk3/PK3PublishedIndex.ts` | `crates/shagger/src/loader/pk3/published_index.rs` |
| `src/engine/core/Cvar.ts` | `crates/shagger/src/core/cvar.rs` |
| `src/shared/demo/DemoReader.ts` | `crates/shagger_shared/src/demo/demo_reader.rs` |
| `src/shared/net/BinaryProtocol.ts` | `crates/shagger_shared/src/net/binary_protocol.rs` |

Every ported file names its TypeScript source in its first doc comment, and says where and why the Rust differs. To find the port of a SHAG file, search for its name:

```sh
grep -rl "SequenceBuilder.ts" crates
```

## `shagger_shared` modules

| Module | SHAG | Contents |
| --- | --- | --- |
| `net` | `src/shared/net`, `src/engine/network/protocol` | `BinaryEncoder`/`BinaryDecoder`, commands, message envelopes, snapshot decoding, the decoded game state |
| `collision` | `src/shared/collision` | the simulation's `CollisionWorld` and ground detection over a sweep backend |
| `bsp` | `src/shared/bsp` | map entities, spawn points, triggers, lifts, brush model state |
| `server::simulation` | `src/shared/server/simulation` | the input sanitizer and the simulation's value types |
| `demo` | `src/shared/demo` | the `.shagdemo` reader, recorded inputs, the highlight analyzer, the reel's sequence builder and camera paths |
| `entities` | `src/shared/entities` | projectile definitions |
| `settings` | `src/shared/settings` | which settings sync with the account, and their blob |
| `spread_utils` | `src/shared/SpreadUtils.ts` | `mulberry32` and the pellet cone |
| `js`, `fdlibm`, `json`, `math_random` | none | JavaScript number semantics, V8's `sin`/`cos`/`atan2`, a `JSON.parse` reader, a cosmetic random source ([JavaScript parity](../guides/js-parity.md)) |

## `shagger` modules

| Module | SHAG | Contents | Guide |
| --- | --- | --- | --- |
| `core` | `src/engine/core` | cvars, crash reports, the console log file, whole-file writes, bounded thread joins | [Cvars and console](../guides/cvars-and-console.md), [Native runtime](../guides/native-runtime.md) |
| `console` | `src/engine/console` | the command trait, the registry, the engine's commands | [Cvars and console](../guides/cvars-and-console.md) |
| `debug` | `src/engine/debug` | the drop-down console | [Cvars and console](../guides/cvars-and-console.md) |
| `input` | `src/engine/input` | actions and bindings, key codes, gamepads, the device in use | [Input](../guides/input.md) |
| `loader` | `src/engine/loader` | `pk3`, `bsp`, `shader`, `texture`, `model` | [Packages and maps](../guides/packages-and-maps.md), [Models and animation](../guides/models-and-animation.md) |
| `animation` | `src/engine/animation` | the player model, its fit, clips and aim pose | [Models and animation](../guides/models-and-animation.md) |
| `rendering` | Babylon's scene, `src/engine/rendering` | the wgpu device, world and model renderers, sky, water, fog, dynamic lights, offscreen frames | [Rendering](../guides/rendering.md) |
| `lighting` | `src/engine/lighting` | sky lights, emissive surfaces | [Rendering](../guides/rendering.md) |
| `effects` | `src/engine/effects` | colour correction | [Rendering](../guides/rendering.md) |
| `babylon` | Babylon.js | the Babylon code the port needs, Apache-2.0 | [Rendering](../guides/rendering.md), [Contributing](../contributing/index.md#licences) |
| `audio` | `src/engine/AudioManager.ts`, `src/engine/audio` | decoding, the mixer, the sound card, spatial sound, effects and music | [Audio](../guides/audio.md) |
| `network` | `src/engine/network` | the game server connection, snapshots, clock sync, the movement contracts | [Networking and lobby](../guides/networking-and-lobby.md) |
| `lobby` | `src/lobby` (client) | the lobby connection, player keys, settings sync | [Networking and lobby](../guides/networking-and-lobby.md) |
| `physics` | `src/engine/physics` | the Q3 brush and patch sweep, the rapier world for ragdolls | [Collision and physics](../guides/collision-and-physics.md) |
| `movement` | `src/engine/movement` | movement variables | [Collision and physics](../guides/collision-and-physics.md) |
| `camera`, `viewport` | `src/engine/camera`, `src/engine/viewport` | shake, kick, field of view, first-person view effects | [Viewport and weapons](../guides/viewport-and-weapons.md) |
| `weapon` | `src/engine/weapon` | the view model, its motion, and weapon effects | [Viewport and weapons](../guides/viewport-and-weapons.md) |
| `world` | `src/engine/world` | pickup model placement | [Models and animation](../guides/models-and-animation.md) |
| `ui` | `src/engine/ui` | the egui layer, the 2D HUD batch, fonts, shared components | [UI](../guides/ui.md) |
