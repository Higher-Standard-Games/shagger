# Introduction

SHAG is a TypeScript and Babylon.js engine that runs in the browser and in Node. SHAGGER is the same engine in Rust, for native desktop clients. The game built on it, wash-fps-rust, connects to the same lobby and game servers as the browser client and can play against it in one match.

## What is ported

| Area | SHAG | SHAGGER |
| --- | --- | --- |
| Wire protocol and game state | `src/shared/net`, `src/engine/network/protocol` | `shagger_shared::net` |
| Collision and the shared simulation's inputs | `src/shared/collision`, `src/shared/bsp`, `src/shared/server/simulation` | `shagger_shared::{collision, bsp, server}` |
| Demos and highlight reels | `src/shared/demo` | `shagger_shared::demo` |
| Game server connection | `src/engine/network/client` | `shagger::network::client` |
| Lobby client | `src/lobby` (client side) | `shagger::lobby` |
| Packages, maps, shaders, textures, models | `src/engine/loader` | `shagger::loader` |
| Renderer | Babylon's scene, `src/engine/rendering`, `lighting`, `effects` | `shagger::rendering` on wgpu |
| Audio | `src/engine/AudioManager.ts`, `src/engine/audio` | `shagger::audio` on cpal |
| Input, console, cvars | `src/engine/{input,console,core,debug}` | `shagger::{input, console, core, debug}` |
| Menus and HUD | HTML and CSS, `src/engine/ui` | `shagger::ui` with egui and a 2D batch |
| Camera, view effects, weapon view | `src/engine/{camera,viewport,weapon}` | `shagger::{camera, viewport, weapon}` |
| Ragdolls | Havok | rapier3d (`shagger::physics::physics_system`) |

## What is not ported

These stay in TypeScript, and SHAGGER has no counterpart for them:

- the game server, the lobby server and the bot server;
- SHAGGED, the map editor, and the raw `.map` path;
- the CLI and headless tools, bot training;
- the Electron and Capacitor shells, since SHAGGER builds native programs itself.

## What is SHAGGER's own

A few modules have no SHAG source, because a browser provides what they do. They are marked "not a port" or "no TypeScript counterpart" at the top of the file:

- the network threads that carry WebSockets, and the TLS setup (`network::client::{network_session, tls, http_fetch}`);
- the on-disk package store that stands in for the browser's CacheStorage (`loader::pk3::package_store`);
- the sound card and the mixer that stand in for Web Audio (`audio::{device, mixer, spatial}`);
- BC7 texture compression on upload (`rendering::texture_compression`);
- crash reports, the console log file and whole-file writes (`core::{crash_report, console_log, atomic_file}`);
- the cursor fix for macOS (`input::native_cursor`);
- the parser fuzzer (`crates/shagger/tests/fuzz_parsers.rs`).

## Where the engine ends and the game begins

SHAGGER holds what any game on the engine would use. Washoi's own code, such as its snapshot event catalogue, its simulation and its menus, lives in wash-fps-rust (RUST.md 14.1 and 14.2). The engine reaches the game's parts through traits and closures, for example [`EventCodec`](../guides/networking-and-lobby.md#snapshot-events) for the events in a snapshot.

A few modules carry Washoi weapon names (`weapon::rail`, `weapon::voltaic`, `weapon::weapon_slash`) because SHAG keeps them in `src/engine/weapon` too.
