# Audio

> Behaviour: SHAG's `src/engine/AudioManager.ts` (music, playlists, the `s_*` cvars) and `src/engine/audio` (`AudioV2Sfx.ts`, `LoopOverlap.ts`). Sound names, channels, attenuation presets, voice limits and fades are the same.

Source: `crates/shagger/src/audio/`.

SHAG plays effects through Babylon's AudioV2 on Web Audio, and music in an `<audio>` element. SHAGGER replaces both with its own mixer on a cpal output stream (RUST.md 9 and 22).

```text
AudioV2Sfx ── commands ──> lock-free queue ──> audio thread: Mixer ──> sound card
MusicPlayer ── decoded MP3 through a ring buffer ──┘
```

## Effects: `AudioV2Sfx`

`audio_v2_sfx::AudioV2Sfx` is the port of `AudioV2Sfx.ts`.

```rust
use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};

let mut sfx = AudioV2Sfx::open_device_or_wait();
sfx.preload(["sounds/weapons/rocket_fire"], |path| pk3.get_file(path));
sfx.play_2d("sounds/weapons/rocket_fire", &SfxPlayOptions::default());

// Once a frame:
sfx.set_listener(position, look, up);
sfx.maintain();
```

- **Loading.** The browser decodes on first use. Here `preload` decodes up front from whatever the caller fetches with, trying `.ogg`, `.wav` and `.mp3` when a path has no extension, and `play_*` plays only what is loaded, as SHAG does for its preloaded combat bank. There is no unlock, epoch or async loading to track.
- **Playing.** `play_2d`, `play_3d`, `play_loop_3d` and `play_loop_shaped` take `SfxPlayOptions`: volume, a 2D sound's `max_instances` (8 when absent), the `SfxChannel` (`Effects`, `Players`, `Hits`, `Announcer`), an attenuation preset, and a slot in which a new sound cuts the last.
- **The device.** `open_device_or_wait` plays silence until a sound card appears; `maintain` looks for one every two seconds and replaces a device that died (unplugged, or the sound server gone).
- **Offline.** `AudioV2Sfx::offline(rate)` mixes without a device, and `render(out)` pulls samples from it: for tests, the parity harness and writing a match's sound to a file.

## The mixer

`audio::mixer` does what Babylon's AudioV2 graph and Web Audio do for SHAG's sounds, in one render function that runs on the audio thread or offline. An sfx bus with four channels and a music bus feed the output. As in Web Audio:

- every gain change ramps linearly over 10 ms (Babylon's `parameterRampDuration`);
- spatial gains are worked out once per 128-frame render quantum;
- `audio::spatial` is the `PannerNode` model SHAG's spatial sounds use, written from the W3C Web Audio specification: the inverse distance model and equal-power panning.

The voice rules are RUST.md 22.5's: at most 48 spatial one-shots, cutting the one closest to its end; a new sound in a slot cuts the last; a 2D sound group keeps at most `max_instances`, stopping the oldest.

After construction the mixer does not allocate. `audio::device::DeviceOutput` gives the audio thread its commands through a lock-free queue (`rtrb`), so the thread never blocks, allocates or frees. The listener moves every frame, so it is not queued: the newest one waits in a slot the callback takes when it can.

## Decoding

`audio::decode` decodes Ogg Vorbis through `lewton` and reads WAV (PCM and float) itself. RUST.md 22.7 named symphonia, which is MPL-2.0; lewton is MIT or Apache-2.0, and the packages hold only Vorbis and WAV. Resampling to the mixer's rate is linear for now, where browsers use a windowed sinc.

`audio::loop_overlap` ports `LoopOverlap.ts`: a loop that crossfades its own tail into its head so it repeats without a click, with an attack played once in front.

## Music

`audio::music::MusicPlayer` ports `AudioManager.ts`'s playlists: `start_playlist`, `switch_playlist`, `stop_playlist`, fades, volume, enabling and per-playlist muting. Tracks are MP3s decoded with `rmp3` and streamed into the music bus through a ring buffer. On a sound card a thread decodes ahead; offline, `update` decodes on the caller's thread, so a render to a file is the same every run.

Fades step the volume by 0.02 every 50 ms, as the TypeScript's `setInterval`s do, counted from `update`'s `dt`. A track that cannot be found or decoded is skipped.

## Cvars

`audio_cvars::register_audio_cvars` registers the cvar half of `AudioManager.ts`: `s_volume` and `s_musicenabled` for the music, `s_sfxvolume` and `s_sfxenabled` for the effects, and the channel shares `cg_volume_effects`, `cg_volume_players`, `cg_volume_hitsound` and `cg_volume_announcer`. The browser applies each in its `onChange`. Here the game calls `apply_audio_cvars(cvars, sfx, music, fetch)` when the registry's `serial` changed.
