# FAQ

## The build fails on Linux with `alsa` or `libudev` errors

cpal links ALSA and gilrs links udev. Install their development packages:

```sh
sudo apt-get install -y libasound2-dev libudev-dev
```

## A test says "skipped"

Some tests need what the repository does not hold: a GPU adapter (`tests/egui_offscreen.rs`), the player model package (`SHAGGER_PLAYER_DATA`), a font (`SHAGGER_TEST_FONT`) or an MP3 (`SHAGGER_TEST_MP3`). Without it they print why and pass. See [Getting started](../getting-started/index.md#build-and-test).

## A golden test fails after a SHAG change

The vectors were made from an older SHAG. Regenerate them from the SHAG commit the port follows ([Golden vectors](../tools/golden-vectors.md)), then port the TypeScript change until the test passes. If the test fails on vectors from the same SHAG commit, the Rust differs from the TypeScript: compare the two line by line, and look first for a standard library call where the TypeScript calls a `Math` function ([JavaScript parity](../guides/js-parity.md)).

## The native client's prediction drifts from the server

Prediction drifts when one step gives a different bit than the server's. The usual causes are `f64::round` or `f64::hypot` where the TypeScript calls `Math.round` or `Math.hypot`, a computation done in `f32` where JavaScript uses doubles, or operations reordered (`a + b + c` is not `a + (b + c)` in floating point). Movement parity is proven in wash-fps-rust on synthetic and shipped maps; add the failing case there.

## The picture differs slightly from the browser's

Textures are compressed to BC7 on a hardware GPU, which is lossy. `r_texture_compression 0` uploads them uncompressed, as the browser does. Beyond that, the module comments of `rendering/` say what is not yet drawn as SHAG draws it ([Rendering](../guides/rendering.md)).

## A software renderer is very slow

On llvmpipe or WARP, BC7 is decoded on every sample, so the native client keeps textures uncompressed there unless `r_texture_compression` is 2. If it is still slow, that is the software renderer; a GPU driver is needed for playable frame rates.

## Connections fail behind a company proxy

The client trusts the Mozilla roots it ships with, not the system's store. A proxy that re-signs TLS needs its certificate added: put it, as PEM, in a file and name it in `SSL_CERT_FILE` ([Native runtime](../guides/native-runtime.md#tls)).

## Two copies of the game, and one says the data is busy

The package store takes `sync.lock` in the data folder while it updates it, so two copies never write the same files. The second copy gets `SyncError::Busy`; it can play from the folder as it stands once the first has finished ([Packages and maps](../guides/packages-and-maps.md#the-published-index-and-the-package-store)).

## The game crashed

A panic writes `crash-<seconds>.txt` into the folder the game gave `core::crash_report::install`, with a backtrace. A game started without a terminal also keeps what it printed in its console log file ([Native runtime](../guides/native-runtime.md)).

## Where does the server code live?

In SHAG and wash-fps, in TypeScript. SHAGGER is the client side only ([Introduction](../introduction/index.md#what-is-not-ported)).
