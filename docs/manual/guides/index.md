# Guides

Each guide covers one subsystem's Rust API and where it differs from SHAG. The behaviour itself, such as which cvars exist, what a shader keyword does or what a snapshot holds, is in SHAG's manual, linked at the top of each guide.

| Guide | Covers | SHAG's page |
| --- | --- | --- |
| [Cvars and console](./cvars-and-console.md) | `core::cvar`, `console`, `debug::debug_console` | [Cvar system](https://shag.dev/guides/cvar-system.html), [Console system](https://shag.dev/guides/console-system.html) |
| [Input](./input.md) | `input` | none yet |
| [Packages and maps](./packages-and-maps.md) | `loader::{pk3, bsp, shader, texture}` | [PK3 filesystem](https://shag.dev/guides/pk3-filesystem.html), [BSP system](https://shag.dev/guides/bsp-system.html), [Shader system](https://shag.dev/guides/shader-system.html), [Skybox system](https://shag.dev/guides/skybox-system.html) |
| [Models and animation](./models-and-animation.md) | `loader::model`, `animation`, `world` | [Model loading](https://shag.dev/guides/model-loading.html), [Pickup system](https://shag.dev/guides/pickup-system.html) |
| [Rendering](./rendering.md) | `rendering`, `lighting`, `effects`, `babylon` | [Render variables](https://shag.dev/guides/render-variables.html), [Lighting system](https://shag.dev/guides/lighting-system.html) |
| [Audio](./audio.md) | `audio` | none yet |
| [Networking and lobby](./networking-and-lobby.md) | `shagger_shared::net`, `network`, `lobby` | [Networking](https://shag.dev/guides/networking.html), [Lobby server](https://shag.dev/guides/lobby-server.html) |
| [Collision and physics](./collision-and-physics.md) | `shagger_shared::collision`, `physics`, `movement` | [Physics system](https://shag.dev/guides/physics-system.html), [Movement system](https://shag.dev/guides/movement-system.html) |
| [Viewport and weapons](./viewport-and-weapons.md) | `camera`, `viewport`, `weapon` | [Camera system](https://shag.dev/guides/camera-system.html), [Viewport effects](https://shag.dev/guides/viewport-effects.html), [Weapon system](https://shag.dev/guides/weapon-system.html) |
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
