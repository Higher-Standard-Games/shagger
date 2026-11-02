//! SHAG `src/engine/audio/AudioV2Sfx.ts`: short decoded sound effects on
//! the sfx bus and its four channels, 2D or placed in the world, and loops.
//!
//! The browser decodes on first use; here sounds are decoded by
//! [`AudioV2Sfx::preload`] from whatever the caller fetches them with (the
//! packages), and `play_*` plays only what is loaded, as SHAG does for the
//! preloaded combat bank. Output goes to the sound card, or to an offline
//! mixer that [`AudioV2Sfx::render`] pulls from (tests, the parity harness,
//! `--audio-out`). There is no unlock, epoch or async loading to track.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::DVec3;

use crate::audio::decode::{SoundBuffer, decode_sound};
use crate::audio::device::DeviceOutput;
use crate::audio::mixer::{Command, Mixer, Play, Route, SfxChannel, Spatial, VoiceId};
use crate::audio::spatial::Listener;

/// `Attenuation`: Quake's attenuation classes, as the inverse model's
/// rolloff.
pub mod attenuation {
    pub const NONE: f64 = 0.0;
    pub const DISTANT: f64 = 0.5;
    pub const NORMAL: f64 = 1.0;
    pub const IDLE: f64 = 2.5;
    pub const STATIC: f64 = 5.0;
    pub const WHISPER: f64 = 10.0;
}

const SOUND_EXTENSIONS: [&str; 3] = [".ogg", ".wav", ".mp3"];

/// `SfxPlayOptions` and `SpatialSfxPlayOptions`.
#[derive(Debug, Clone, Default)]
pub struct SfxPlayOptions {
    /// 1 when absent.
    pub volume: Option<f32>,
    /// A 2D sound's instances at once (8 when absent).
    pub max_instances: Option<usize>,
    pub channel: SfxChannel,
    /// Normal when absent.
    pub attenuation: Option<f64>,
    pub slot: Option<String>,
}

/// A looping sound until [`AudioV2Sfx::stop_loop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SfxLoop(VoiceId);

enum Output {
    Device(DeviceOutput),
    Offline(Box<Mixer>),
}

pub struct AudioV2Sfx {
    output: Output,
    buffers: HashMap<String, Arc<SoundBuffer>>,
    channel_volumes: [f32; 4],
    volume: f32,
    muted: bool,
    /// Loops with the volume each was started at.
    loops: HashMap<VoiceId, f32>,
    loops_muted: bool,
    next_id: VoiceId,
}

/// `normalizePath`.
#[must_use]
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_lowercase()
}

fn hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl AudioV2Sfx {
    /// The sound card's default output.
    pub fn open_device() -> Result<Self, String> {
        Ok(Self::with_output(Output::Device(DeviceOutput::open()?)))
    }

    /// A mixer at `sample_rate` that [`Self::render`] pulls from.
    #[must_use]
    pub fn offline(sample_rate: u32) -> Self {
        Self::with_output(Output::Offline(Box::new(Mixer::new(sample_rate))))
    }

    fn with_output(output: Output) -> Self {
        Self {
            output,
            buffers: HashMap::new(),
            channel_volumes: [1.0; 4],
            volume: 1.0,
            muted: false,
            loops: HashMap::new(),
            loops_muted: false,
            next_id: 0,
        }
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        match &self.output {
            Output::Device(d) => d.sample_rate(),
            Output::Offline(m) => m.sample_rate(),
        }
    }

    fn send(&mut self, command: Command) {
        match &mut self.output {
            Output::Device(d) => d.send(command),
            Output::Offline(m) => m.apply(command),
        }
    }

    /// Mixes the next frames of an offline output into `out` (interleaved
    /// stereo); does nothing for the sound card.
    pub fn render(&mut self, out: &mut [f32]) {
        if let Output::Offline(m) = &mut self.output {
            m.render(out);
        }
    }

    /// The offline mixer, for tests.
    #[must_use]
    pub fn offline_mixer(&self) -> Option<&Mixer> {
        match &self.output {
            Output::Offline(m) => Some(m),
            Output::Device(_) => None,
        }
    }

    /// `setVolume`: the sfx bus.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let gain = if self.muted { 0.0 } else { self.volume };
        self.send(Command::SetSfxGain(gain));
    }

    /// `setMuted`: silence that keeps the mix.
    pub fn set_muted(&mut self, muted: bool) {
        if self.muted == muted {
            return;
        }
        self.muted = muted;
        let gain = if muted { 0.0 } else { self.volume };
        self.send(Command::SetSfxGain(gain));
    }

    pub fn set_channel_volume(&mut self, channel: SfxChannel, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.channel_volumes[channel as usize] = volume;
        self.send(Command::SetChannelGain(channel, volume));
    }

    /// The listener, from the eye each frame (`syncListener`): `look` is
    /// where the camera faces in game space.
    pub fn set_listener(&mut self, position: DVec3, look: DVec3, up: DVec3) {
        self.send(Command::SetListener(Listener::from_camera(position, look, up)));
    }

    /// `preload`: decodes each path not yet loaded, trying the sound
    /// extensions when it has none. `fetch` reads a normalized path's
    /// bytes. Returns how many are loaded now.
    pub fn preload<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a str>,
        mut fetch: impl FnMut(&str) -> Option<Vec<u8>>,
    ) -> usize {
        let rate = self.sample_rate();
        for path in paths {
            let key = normalize_path(path);
            if self.buffers.contains_key(&key) {
                continue;
            }
            let candidates: Vec<String> = if SOUND_EXTENSIONS.iter().any(|e| key.ends_with(e)) {
                vec![key.clone()]
            } else {
                SOUND_EXTENSIONS.iter().map(|e| format!("{key}{e}")).collect()
            };
            for candidate in candidates {
                let Some(bytes) = fetch(&candidate) else { continue };
                match decode_sound(&bytes, rate) {
                    Ok(buffer) => {
                        self.buffers.insert(key.clone(), Arc::new(buffer));
                    }
                    Err(e) => eprintln!("[AudioManager] Failed to decode SFX '{path}': {e}"),
                }
                break;
            }
        }
        self.buffers.len()
    }

    /// A loaded sound's length in seconds.
    #[must_use]
    pub fn duration(&self, path: &str) -> Option<f64> {
        self.buffers.get(&normalize_path(path)).map(|b| b.duration())
    }

    fn next_id(&mut self) -> VoiceId {
        self.next_id += 1;
        self.next_id
    }

    /// `play2D`: everywhere at once, at most `max_instances` of this sound
    /// on this channel.
    pub fn play_2d(&mut self, path: &str, options: &SfxPlayOptions) -> bool {
        if self.muted {
            return false;
        }
        let key = normalize_path(path);
        let Some(buffer) = self.buffers.get(&key).cloned() else {
            return false;
        };
        let group = hash(&format!("{:?}:{key}", options.channel));
        let id = self.next_id();
        self.send(Command::Play(Play {
            id,
            buffer,
            route: Route::Sfx(options.channel),
            volume: options.volume.unwrap_or(1.0).max(0.0),
            looped: false,
            spatial: None,
            slot: None,
            instances: Some((group, options.max_instances.unwrap_or(8))),
        }));
        true
    }

    /// `play3D`: fixed at `position`, fading by the attenuation class.
    pub fn play_3d(&mut self, path: &str, position: DVec3, options: &SfxPlayOptions) -> bool {
        if self.muted {
            return false;
        }
        let Some(buffer) = self.buffers.get(&normalize_path(path)).cloned() else {
            return false;
        };
        let id = self.next_id();
        self.send(Command::Play(Play {
            id,
            buffer,
            route: Route::Sfx(options.channel),
            volume: options.volume.unwrap_or(1.0).max(0.0),
            looped: false,
            spatial: Some(Spatial {
                position,
                rolloff: options.attenuation.unwrap_or(attenuation::NORMAL).max(0.0),
            }),
            slot: options.slot.as_deref().map(hash),
            instances: None,
        }));
        true
    }

    /// `playLoop3D`: loops at `position`, or everywhere when `None`, until
    /// stopped.
    pub fn play_loop_3d(&mut self, path: &str, position: Option<DVec3>, options: &SfxPlayOptions) -> Option<SfxLoop> {
        let buffer = self.buffers.get(&normalize_path(path)).cloned()?;
        let volume = options.volume.unwrap_or(1.0).max(0.0);
        let id = self.next_id();
        self.loops.insert(id, volume);
        self.send(Command::Play(Play {
            id,
            buffer,
            route: Route::Sfx(options.channel),
            volume: if self.loops_muted { 0.0 } else { volume },
            looped: true,
            spatial: position.map(|position| Spatial {
                position,
                rolloff: options.attenuation.unwrap_or(attenuation::NORMAL).max(0.0),
            }),
            slot: None,
            instances: None,
        }));
        Some(SfxLoop(id))
    }

    pub fn stop_loop(&mut self, handle: SfxLoop) {
        if self.loops.remove(&handle.0).is_some() {
            self.send(Command::Stop(handle.0));
        }
    }

    /// `setLoopsMuted`: silences or restores every loop without stopping it.
    pub fn set_loops_muted(&mut self, muted: bool) {
        self.loops_muted = muted;
        let loops: Vec<(VoiceId, f32)> = self.loops.iter().map(|(&id, &v)| (id, v)).collect();
        for (id, volume) in loops {
            self.send(Command::SetVoiceVolume(id, if muted { 0.0 } else { volume }));
        }
    }

    /// Starts a music stream (see `music`): interleaved stereo at
    /// `sample_rate()`, pushed into the other end of `samples`.
    pub fn play_stream(&mut self, samples: rtrb::Consumer<f32>, volume: f32) -> VoiceId {
        let id = self.next_id();
        self.send(Command::PlayStream { id, samples, volume });
        id
    }

    /// A playing voice's or stream's volume, ramped.
    pub fn set_voice_volume(&mut self, id: VoiceId, volume: f32) {
        self.send(Command::SetVoiceVolume(id, volume));
    }

    pub fn stop_voice(&mut self, id: VoiceId) {
        self.send(Command::Stop(id));
    }

    /// Whether the offline mixer still plays `id` (always true on a device,
    /// which cannot be asked).
    #[must_use]
    pub fn offline_is_playing(&self, id: VoiceId) -> bool {
        self.offline_mixer().is_none_or(|m| m.is_playing(id))
    }

    /// `dispose`'s silence: every sound stops; the loaded sounds stay.
    pub fn stop_all(&mut self) {
        self.loops.clear();
        self.send(Command::StopAll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav() -> Vec<u8> {
        let data: Vec<u8> = (0..4800i16).flat_map(|i| ((i % 100) * 100).to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&48000u32.to_le_bytes());
        out.extend_from_slice(&96000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn plays_what_it_loaded() {
        let mut sfx = AudioV2Sfx::offline(48000);
        let loaded = sfx.preload(["Sounds\\misc\\hit_0.WAV"], |p| {
            (p == "sounds/misc/hit_0.wav").then(wav)
        });
        assert_eq!(loaded, 1);
        assert!(sfx.play_2d("sounds/misc/hit_0.wav", &SfxPlayOptions::default()));
        assert!(!sfx.play_2d("sounds/misc/missing.wav", &SfxPlayOptions::default()));
        let handle = sfx.play_loop_3d("sounds/misc/hit_0.wav", Some(DVec3::X), &SfxPlayOptions::default());
        assert_eq!(sfx.offline_mixer().unwrap().voice_count(), 2);
        let mut out = vec![0.0; 2 * 5000];
        sfx.render(&mut out);
        assert!(out.iter().any(|&s| s != 0.0));
        sfx.stop_loop(handle.unwrap());
        assert_eq!(sfx.offline_mixer().unwrap().voice_count(), 0);
        sfx.set_muted(true);
        assert!(!sfx.play_2d("sounds/misc/hit_0.wav", &SfxPlayOptions::default()));
    }
}
