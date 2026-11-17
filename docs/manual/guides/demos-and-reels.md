# Demos and highlight reels

> Behaviour: SHAG's `src/shared/demo` (`DemoReader.ts`, `DemoInputs.ts`, `DemoAnalyzer.ts`, `SequenceBuilder.ts`, `CinematicCameraPaths.ts`). RUST.md 18.5 lays out the file format. The reel a native client cuts from a demo is the reel the browser cuts, shot for shot.

Source: `crates/shagger_shared/src/demo/`.

## Reading a demo

`demo_reader::DemoReader` parses `.shagdemo` files, versions 1 to 8, gzipped or not. A demo stores a full (non-delta) raw snapshot per frame, so any frame decodes on its own through the index table.

```rust
use shagger::shared::demo::demo_reader::DemoReader;

let mut reader = DemoReader::new(bytes)?;
let header = reader.read_header()?.clone();      // map, gametype, tick rate, players, ...
let duration_ms = reader.read_index()?.duration_ms;
let frames = reader.read_all_frames(&MyEvents)?;  // the game's EventCodec
let frame = reader.find_frame_by_tick(1200.0)?;   // index of the frame at a tick
```

Frames decode snapshot events through the game's [`EventCodec`](./networking-and-lobby.md#snapshot-events), as live snapshots do. Reads keep the TypeScript's tolerance exactly: a string that runs past the end is cut short rather than refused, a frame that fails to decode is skipped, and inputs that fail to decode leave the frame with none. From version 3, each frame also carries the commands the server received (`demo_inputs`).

## Highlights

`demo_analyzer::DemoAnalyzer::analyze(frames, header, duration_ms)` finds a demo's highlights (frags, multi-kills, frag sequences, airborne frags, movement runs) and scores them, as `DemoAnalyzer.ts` does.

The TypeScript reads its game's events by name (`death`, `award`, `combat_state`). Here the game's event type says which of those an event is by implementing `HighlightEvent`, whose `analyzer_event` returns an `AnalyzerEvent`. Every map the TypeScript iterates keeps its insertion order in the port, so the highlights come out in the same order.

## The reel

`sequence_builder::build(analysis, target_duration_minutes, tick_rate)` cuts the reel's shot list (`CinematicReel`) from the analysis:

- each frag is a scene of three shots: a third-person intro, the same moment from a player's eyes, and a slow orbit;
- a frag sequence is filmed as it happened: a chase between frags, the killer's eyes for each with a counter, then the orbit;
- movement clips are cut in between frag scenes;
- three frag scenes end on a freeze frame (`ReelFreeze`, held `FREEZE_MS`), and some cuts jump ahead through speed lines;
- the reel's playback speed follows `SpeedKeyframe`s, read with `CinematicReel::planned_speed_at(tick)`.

`cinematic_camera_paths::generate_camera_path` gives a freefly shot's camera keyframes, or the entity a chase shot follows, and `interpolate_path` samples them.

Drawing the reel (the overlay, the freeze frame's stamp and caption, the speed lines) is the game's, in wash-fps-rust.

## Parity

Reels are deterministic: the same demo gives the same reel in the browser and natively. `tests/highlights_golden.rs` runs synthetic matches through the analyzer, the builder and the camera paths, and compares every number with what SHAG's TypeScript produced (`tools/golden/highlights.mjs`). `tests/demo_golden.rs` checks the reader against SHAG's own `DemoReader` on synthetic demos of every version, raw and gzipped, many of them damaged (`tools/golden/protocol.mjs`). A fix to the reel lands in the TypeScript first, then in the port, with the vectors regenerated ([Golden vectors](../tools/golden-vectors.md)).
