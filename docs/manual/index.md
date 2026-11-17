# SHAGGER documentation

SHAGGER is the Rust counterpart of the [SHAG](https://github.com/Higher-Standard-Games/shag) engine. It is a library, not a program: [wash-fps-rust](https://github.com/Higher-Standard-Games/wash-fps-rust), the native Washoi client, is built on it, as wash-fps is built on SHAG.

SHAGGER ports SHAG's TypeScript one to one where Rust allows. It reads the same packages, speaks the same wire protocol, plays the same demos and runs the same shared simulation, bit for bit where the simulation and the wire format are concerned.

## How this manual relates to SHAG's

[SHAG's manual](https://shag.dev/) is the reference for what the engine does: the cvars, the console, the packages, the maps and shaders, the protocol and the demo format. This manual does not repeat it. It covers what is particular to the Rust port: the crates, how to build and test them, the Rust API, the native backends (wgpu, winit, cpal, egui, gilrs, rapier), and where the port differs from the TypeScript and why.

Each guide here links to the SHAG page for the behaviour it ports. When the two disagree, the TypeScript is right and the Rust has a bug, unless the guide says the difference is deliberate.

This manual is published at [shagger.shag.dev](https://shagger.shag.dev/).

## Start here

| Section | Use it for |
| --- | --- |
| [Introduction](./introduction/index.md) | What SHAGGER is, what it ports and what it leaves out |
| [Getting started](./getting-started/index.md) | Build, test, and use it from a game |
| [Project structure](./getting-started/project-structure.md) | The two crates and their modules, mapped to SHAG's folders |
| [Guides](./guides/index.md) | Each subsystem's Rust API and how it differs from SHAG |
| [Tools](./tools/index.md) | Golden vectors and the parser fuzzer |
| [Contributing](./contributing/index.md) | Porting rules, checks and licences |
| [FAQ](./faq/index.md) | Troubleshooting |

## Quick verification

```sh
cargo test --workspace
```

On Linux, install ALSA's and udev's development packages first (see [Getting started](./getting-started/index.md)).

## Other references

- The plan, its findings and the milestones are in wash-fps-rust's [`docs/research/RUST.md`](https://github.com/Higher-Standard-Games/wash-fps-rust/blob/main/docs/research/RUST.md). Section numbers in this manual and in the source refer to it.
- The rules for agents are in [`AGENTS.md`](https://github.com/Higher-Standard-Games/shagger/blob/main/AGENTS.md).

SHAGGER is released under the [MIT License](https://github.com/Higher-Standard-Games/shagger/blob/main/LICENSE), except for the Babylon.js ports in `crates/shagger/src/babylon/`, which are Apache-2.0 (see [Contributing](./contributing/index.md#licences)).
