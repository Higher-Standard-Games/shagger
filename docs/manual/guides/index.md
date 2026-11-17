# Guides

Each guide covers one subsystem's Rust API and where it differs from SHAG. The behaviour itself, such as which cvars exist, what a shader keyword does or what a snapshot holds, is in SHAG's manual, linked at the top of each guide.

| Guide | Covers | SHAG's page |
| --- | --- | --- |
| [Cvars and console](./cvars-and-console.md) | `core::cvar`, `console`, `debug::debug_console` | [Cvar system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/cvar-system.md), [Console system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/console-system.md) |
| [Input](./input.md) | `input` | none yet |
| [Packages and maps](./packages-and-maps.md) | `loader::{pk3, bsp, shader, texture}` | [PK3 filesystem](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/pk3-filesystem.md), [BSP system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/bsp-system.md), [Shader system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/shader-system.md), [Skybox system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/skybox-system.md) |
| [Models and animation](./models-and-animation.md) | `loader::model`, `animation`, `world` | [Model loading](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/model-loading.md), [Pickup system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/pickup-system.md) |
| [Rendering](./rendering.md) | `rendering`, `lighting`, `effects`, `babylon` | [Render variables](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/render-variables.md), [Lighting system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/lighting-system.md) |
| [Audio](./audio.md) | `audio` | none yet |
| [Networking and lobby](./networking-and-lobby.md) | `shagger_shared::net`, `network`, `lobby` | [Networking](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/networking.md), [Lobby server](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/lobby-server.md) |
| [Collision and physics](./collision-and-physics.md) | `shagger_shared::collision`, `physics`, `movement` | [Physics system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/physics-system.md), [Movement system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/movement-system.md) |
| [Viewport and weapons](./viewport-and-weapons.md) | `camera`, `viewport`, `weapon` | [Camera system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/camera-system.md), [Viewport effects](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/viewport-effects.md), [Weapon system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/weapon-system.md) |
| [UI](./ui.md) | `ui` | none yet |
| [Demos and highlight reels](./demos-and-reels.md) | `shagger_shared::demo` | none yet |
| [JavaScript parity](./js-parity.md) | `shagger_shared::{js, fdlibm, json, math_random}` | none: this is the port's own |
| [Native runtime](./native-runtime.md) | `core::{crash_report, console_log, atomic_file, join}`, TLS | none: this is the port's own |

## Conventions across the API

- **Names follow the TypeScript.** A Rust item keeps its TypeScript name in snake case, and its doc comment quotes the TypeScript name in backticks (`` `Cvar.init()` ``), so a search for either finds it.
- **The game owns I/O and the clock.** Where SHAG relies on the browser's event loop, the Rust port is a state machine the game drives: it passes in the time (`wall` milliseconds for `Date.now()`, `mono` milliseconds for `performance.now()`) and polls for what happened. Threads that block (sockets, downloads, sound) report through channels, and nothing a frame calls blocks.
- **The game's parts come in through traits and closures.** The engine does not know Washoi's events, files or settings. It asks for them: an `EventCodec` for snapshot events, a `fetch` closure for files out of the packages, a context type `C` for console commands.
- **Axes are Babylon's in public, Q3's inside.** As in SHAG, collision and map code takes and returns y-up vectors and works in Quake's z-up space internally.
- **`f64` where JavaScript computes.** JavaScript numbers are doubles. Simulation and protocol code keeps `f64` wherever the TypeScript does arithmetic, and narrows to the wire type only where the TypeScript writes bytes. File data stays in the type it is stored as (`f32` in a BSP) and widens where the TypeScript computes with it.
