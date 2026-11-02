//! The mixer behind `AudioV2Sfx` (RUST.md 22.7): what Babylon's AudioV2
//! graph and Web Audio do for SHAG's sounds, as one render function that
//! runs on the device's audio thread or offline.
//!
//! ```text
//! sfx bus ─┬─ effects ─┐
//!          ├─ players  ├─ voices: one-shot or loop, play volume,
//!          ├─ hits     │  optional fixed position (inverse distance,
//!          └─ announcer┘  equal-power pan), slot, instance group
//! music bus ──────────── voices
//! ```
//!
//! Every gain change ramps linearly over 10 ms (Babylon's
//! `parameterRampDuration`), and the spatial gains are worked out once per
//! 128-frame render quantum, as Web Audio does. After construction nothing
//! here allocates: voices live in a fixed-capacity list and sound data is
//! shared through `Arc`s that the caller's cache keeps alive.
//!
//! The voice rules of RUST.md 22.5: at most 48 spatial one-shots, cutting
//! the one closest to its end; a new sound in a slot cuts the last; a 2D
//! sound group keeps at most `max_instances`, stopping the oldest.

use std::sync::Arc;

use glam::DVec3;

use crate::audio::decode::SoundBuffer;
use crate::audio::spatial::{Listener, REFERENCE_DISTANCE, equal_power_gains, inverse_distance_gain, pan_stereo};

/// Web Audio's render quantum.
pub const RENDER_QUANTUM: usize = 128;
/// Babylon's `parameterRampDuration`.
pub const PARAMETER_RAMP_SECONDS: f64 = 0.01;
/// `MAX_ACTIVE_SPATIAL_SOUNDS`.
pub const MAX_ACTIVE_SPATIAL_SOUNDS: usize = 48;
/// Voices at once, of every kind; past it the oldest is stolen.
pub const MAX_VOICES: usize = 256;

/// `SfxChannel`: the mix a sound belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SfxChannel {
    #[default]
    Effects,
    Players,
    Hits,
    Announcer,
}

impl SfxChannel {
    /// `SFX_CHANNELS`.
    pub const ALL: [Self; 4] = [Self::Effects, Self::Players, Self::Hits, Self::Announcer];

    fn index(self) -> usize {
        self as usize
    }
}

/// Where a voice goes: an sfx channel or the music bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Sfx(SfxChannel),
    Music,
}

const MUSIC_BUS: usize = 4;

/// A voice's handle.
pub type VoiceId = u64;

/// A sound placed in the world, fixed where it started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spatial {
    pub position: DVec3,
    /// The attenuation class (`rolloffFactor`).
    pub rolloff: f64,
}

/// One sound to start.
#[derive(Debug, Clone)]
pub struct Play {
    pub id: VoiceId,
    pub buffer: Arc<SoundBuffer>,
    pub route: Route,
    pub volume: f32,
    pub looped: bool,
    pub spatial: Option<Spatial>,
    /// One sound at a time per slot.
    pub slot: Option<u64>,
    /// A 2D sound's group and its `maxInstances`.
    pub instances: Option<(u64, usize)>,
}

/// What the game thread asks of the mixer.
#[derive(Debug)]
pub enum Command {
    Play(Play),
    /// Music: interleaved stereo at the mixer's rate, pushed by a decoder
    /// thread, on the music bus until stopped. An empty queue plays
    /// silence.
    PlayStream {
        id: VoiceId,
        samples: rtrb::Consumer<f32>,
        volume: f32,
    },
    Stop(VoiceId),
    SetVoiceVolume(VoiceId, f32),
    SetListener(Listener),
    /// The sfx bus (`s_sfxvolume`, 0 when muted).
    SetSfxGain(f32),
    SetChannelGain(SfxChannel, f32),
    SetMusicGain(f32),
    StopAll,
}

/// A gain that ramps linearly to its target.
#[derive(Debug, Clone, Copy)]
struct Ramp {
    value: f32,
    target: f32,
    step: f32,
}

impl Ramp {
    fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
        }
    }

    fn set(&mut self, target: f32, ramp_frames: f32) {
        self.target = target;
        self.step = (target - self.value) / ramp_frames.max(1.0);
    }

    fn next(&mut self) -> f32 {
        if self.value != self.target {
            self.value += self.step;
            if (self.step > 0.0 && self.value > self.target) || (self.step < 0.0 && self.value < self.target) {
                self.value = self.target;
            }
        }
        self.value
    }
}

#[derive(Debug, Clone)]
struct Voice {
    id: VoiceId,
    buffer: Arc<SoundBuffer>,
    cursor: usize,
    looped: bool,
    gain: Ramp,
    bus: usize,
    spatial: Option<Spatial>,
    slot: Option<u64>,
    instances: Option<u64>,
    started: u64,
    finished: bool,
}

impl Voice {
    fn remaining(&self) -> usize {
        self.buffer.frames().saturating_sub(self.cursor)
    }

    fn is_spatial_one_shot(&self) -> bool {
        self.spatial.is_some() && !self.looped
    }
}

/// Music streams at once (a track and the one fading out).
const MAX_STREAMS: usize = 4;

#[derive(Debug)]
struct StreamVoice {
    id: VoiceId,
    samples: rtrb::Consumer<f32>,
    gain: Ramp,
}

pub struct Mixer {
    sample_rate: u32,
    ramp_frames: f32,
    voices: Vec<Voice>,
    streams: Vec<StreamVoice>,
    /// sfx, then the four channels, then music.
    sfx: Ramp,
    buses: [Ramp; 5],
    /// Each bus's gain per frame of the quantum.
    bus_gains: [[f32; RENDER_QUANTUM]; 5],
    listener: Listener,
    started: u64,
}

impl Mixer {
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            ramp_frames: (PARAMETER_RAMP_SECONDS * f64::from(sample_rate)) as f32,
            voices: Vec::with_capacity(MAX_VOICES),
            streams: Vec::with_capacity(MAX_STREAMS),
            sfx: Ramp::new(1.0),
            buses: [Ramp::new(1.0); 5],
            bus_gains: [[0.0; RENDER_QUANTUM]; 5],
            listener: Listener::default(),
            started: 0,
        }
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Voices playing, for tests and the debug overlay.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Spatial one-shots playing.
    #[must_use]
    pub fn spatial_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_spatial_one_shot()).count()
    }

    #[must_use]
    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.voices.iter().any(|v| v.id == id) || self.streams.iter().any(|v| v.id == id)
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Play(play) => self.play(play),
            Command::PlayStream { id, samples, volume } => {
                if self.streams.len() < MAX_STREAMS {
                    self.streams.push(StreamVoice {
                        id,
                        samples,
                        gain: Ramp::new(volume.max(0.0)),
                    });
                }
            }
            Command::Stop(id) => {
                self.voices.retain(|v| v.id != id);
                self.streams.retain(|v| v.id != id);
            }
            Command::SetVoiceVolume(id, volume) => {
                let frames = self.ramp_frames;
                if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
                    v.gain.set(volume, frames);
                }
                if let Some(v) = self.streams.iter_mut().find(|v| v.id == id) {
                    v.gain.set(volume, frames);
                }
            }
            Command::SetListener(listener) => self.listener = listener,
            Command::SetSfxGain(gain) => self.sfx.set(gain, self.ramp_frames),
            Command::SetChannelGain(channel, gain) => self.buses[channel.index()].set(gain, self.ramp_frames),
            Command::SetMusicGain(gain) => self.buses[MUSIC_BUS].set(gain, self.ramp_frames),
            // Music is not a sound effect: its streams keep playing.
            Command::StopAll => self.voices.clear(),
        }
    }

    fn play(&mut self, play: Play) {
        if play.buffer.frames() == 0 {
            return;
        }
        if let Some(slot) = play.slot {
            self.voices.retain(|v| v.slot != Some(slot));
        }
        if let Some((group, max)) = play.instances {
            // Babylon's maxInstances: the oldest started instance stops.
            while self.voices.iter().filter(|v| v.instances == Some(group)).count() >= max.max(1) {
                self.remove_oldest(|v| v.instances == Some(group));
            }
        }
        if self.voices.len() >= MAX_VOICES {
            self.remove_oldest(|_| true);
        }
        self.started += 1;
        self.voices.push(Voice {
            id: play.id,
            buffer: play.buffer,
            cursor: 0,
            looped: play.looped,
            gain: Ramp::new(play.volume.max(0.0)),
            bus: match play.route {
                Route::Sfx(channel) => channel.index(),
                Route::Music => MUSIC_BUS,
            },
            spatial: play.spatial,
            slot: play.slot,
            instances: play.instances.map(|(group, _)| group),
            started: self.started,
            finished: false,
        });
        // Over the cap, cut the sound nearest its end: a footstep's last
        // moment before an explosion's long tail.
        while self.spatial_count() > MAX_ACTIVE_SPATIAL_SOUNDS {
            let nearest = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_spatial_one_shot())
                .min_by_key(|(_, v)| v.remaining())
                .map(|(i, _)| i);
            match nearest {
                Some(i) => {
                    self.voices.remove(i);
                }
                None => break,
            }
        }
    }

    fn remove_oldest(&mut self, mut matches: impl FnMut(&Voice) -> bool) {
        let oldest = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| matches(v))
            .min_by_key(|(_, v)| v.started)
            .map(|(i, _)| i);
        if let Some(i) = oldest {
            self.voices.remove(i);
        }
    }

    /// Mixes the next `out.len() / 2` stereo frames into `out`
    /// (interleaved left, right), replacing what was there.
    pub fn render(&mut self, out: &mut [f32]) {
        for chunk in out.chunks_mut(RENDER_QUANTUM * 2) {
            self.render_quantum(chunk);
        }
        self.voices.retain(|v| !v.finished);
    }

    fn render_quantum(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        out.fill(0.0);
        for f in 0..frames {
            let sfx = self.sfx.next();
            for (bus, ramp) in self.buses.iter_mut().enumerate() {
                let gain = ramp.next();
                self.bus_gains[bus][f] = if bus == MUSIC_BUS { gain } else { gain * sfx };
            }
        }
        let listener = self.listener;
        for voice in &mut self.voices {
            if voice.finished {
                continue;
            }
            let buffer = &voice.buffer;
            let channels = buffer.channels;
            let total = buffer.frames();
            // Web Audio's k-rate panner: once per quantum.
            let place = voice.spatial.map(|s| {
                let distance = (s.position - listener.position).length();
                let gain = inverse_distance_gain(distance, REFERENCE_DISTANCE, s.rolloff) as f32;
                let azimuth = listener.azimuth(s.position);
                (gain, azimuth, equal_power_gains(azimuth, false))
            });
            let gains = &self.bus_gains[voice.bus];
            for f in 0..frames {
                if voice.cursor >= total {
                    if voice.looped {
                        voice.cursor = 0;
                    } else {
                        voice.finished = true;
                        break;
                    }
                }
                let gain = voice.gain.next() * gains[f];
                let at = voice.cursor * channels;
                let (l, r) = if channels == 1 {
                    let s = buffer.samples[at];
                    match place {
                        Some((distance, _, (gl, gr))) => (s * gl as f32 * distance, s * gr as f32 * distance),
                        None => (s, s),
                    }
                } else {
                    let (l, r) = (buffer.samples[at], buffer.samples[at + 1]);
                    match place {
                        Some((distance, azimuth, _)) => {
                            let (l, r) = pan_stereo(azimuth, l, r);
                            (l * distance, r * distance)
                        }
                        None => (l, r),
                    }
                };
                out[f * 2] += l * gain;
                out[f * 2 + 1] += r * gain;
                voice.cursor += 1;
            }
        }
        let music = &self.bus_gains[MUSIC_BUS];
        for stream in &mut self.streams {
            for f in 0..frames {
                let gain = stream.gain.next() * music[f];
                if stream.samples.slots() < 2 {
                    continue;
                }
                let l = stream.samples.pop().unwrap_or(0.0);
                let r = stream.samples.pop().unwrap_or(0.0);
                out[f * 2] += l * gain;
                out[f * 2 + 1] += r * gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: usize) -> Arc<SoundBuffer> {
        Arc::new(SoundBuffer {
            channels,
            sample_rate: 48000,
            samples: vec![0.5; frames * channels],
        })
    }

    fn play(id: VoiceId, buffer: &Arc<SoundBuffer>) -> Play {
        Play {
            id,
            buffer: buffer.clone(),
            route: Route::Sfx(SfxChannel::Effects),
            volume: 1.0,
            looped: false,
            spatial: None,
            slot: None,
            instances: None,
        }
    }

    #[test]
    fn a_2d_mono_sound_plays_in_both_ears_and_ends() {
        let mut mixer = Mixer::new(48000);
        mixer.apply(Command::Play(Play {
            volume: 0.5,
            ..play(1, &tone(200, 1))
        }));
        let mut out = vec![0.0; 2 * 256];
        mixer.render(&mut out);
        assert_eq!(out[0], 0.25);
        assert_eq!(out[1], 0.25);
        assert_eq!(out[2 * 199 + 1], 0.25);
        assert_eq!(out[2 * 200], 0.0);
        assert_eq!(mixer.voice_count(), 0);
    }

    #[test]
    fn a_spatial_sound_fades_and_pans() {
        let mut mixer = Mixer::new(48000);
        mixer.apply(Command::SetListener(Listener::from_camera(
            DVec3::ZERO,
            DVec3::Z,
            DVec3::Y,
        )));
        mixer.apply(Command::Play(Play {
            spatial: Some(Spatial {
                position: DVec3::new(250.0, 0.0, 0.0),
                rolloff: 1.0,
            }),
            ..play(1, &tone(100, 1))
        }));
        let mut out = vec![0.0; 2 * 64];
        mixer.render(&mut out);
        // Half volume at 250 for Normal, all of it on the right.
        assert!(out[0].abs() < 1e-6, "{}", out[0]);
        assert!((out[1] - 0.25).abs() < 1e-6, "{}", out[1]);
    }

    #[test]
    fn gain_changes_ramp_over_ten_ms() {
        let mut mixer = Mixer::new(48000);
        mixer.apply(Command::Play(Play {
            looped: true,
            ..play(1, &tone(10, 1))
        }));
        mixer.apply(Command::SetSfxGain(0.0));
        let mut out = vec![0.0; 2 * 960];
        mixer.render(&mut out);
        assert!(out[2 * 240] > 0.2 && out[2 * 240] < 0.3, "{}", out[2 * 240]);
        assert_eq!(out[2 * 480], 0.0);
        assert_eq!(mixer.voice_count(), 1);
    }

    #[test]
    fn a_stream_plays_what_it_is_given() {
        let mut mixer = Mixer::new(48000);
        let (mut producer, consumer) = rtrb::RingBuffer::new(16);
        for s in [0.5, -0.5, 0.25, -0.25] {
            producer.push(s).unwrap();
        }
        mixer.apply(Command::PlayStream {
            id: 9,
            samples: consumer,
            volume: 1.0,
        });
        mixer.apply(Command::SetMusicGain(0.5));
        let mut out = vec![0.0; 8];
        mixer.render(&mut out);
        // The music ramp has barely started: still close to full.
        assert!((out[0] - 0.5).abs() < 0.01 && (out[1] + 0.5).abs() < 0.01);
        assert_eq!(out[4..], [0.0; 4]);
        assert!(mixer.is_playing(9));
        mixer.apply(Command::Stop(9));
        assert!(!mixer.is_playing(9));
    }

    #[test]
    fn the_voice_rules_hold() {
        let mut mixer = Mixer::new(48000);
        let short = tone(100, 1);
        let long = tone(10_000, 1);
        let spatial = Some(Spatial {
            position: DVec3::X,
            rolloff: 1.0,
        });
        // 48 long sounds and one short: the 50th cuts the short one.
        for id in 0..48 {
            mixer.apply(Command::Play(Play {
                spatial,
                ..play(id, &long)
            }));
        }
        mixer.apply(Command::Play(Play {
            spatial,
            ..play(100, &short)
        }));
        assert!(!mixer.is_playing(100));
        assert_eq!(mixer.spatial_count(), 48);
        // A slot keeps one.
        mixer.apply(Command::StopAll);
        for id in 0..3 {
            mixer.apply(Command::Play(Play {
                slot: Some(7),
                ..play(id, &long)
            }));
        }
        assert_eq!(mixer.voice_count(), 1);
        assert!(mixer.is_playing(2));
        // Two instances at most: the oldest stops.
        mixer.apply(Command::StopAll);
        for id in 0..3 {
            mixer.apply(Command::Play(Play {
                instances: Some((9, 2)),
                ..play(id, &long)
            }));
        }
        assert!(!mixer.is_playing(0) && mixer.is_playing(1) && mixer.is_playing(2));
    }
}
