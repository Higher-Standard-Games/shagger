//! SHAG `src/engine/audio/AudioV2Sfx.ts`: short decoded sound effects on
//! the sfx bus and its four channels, 2D or placed in the world, and loops.
//!
//! The browser decodes on first use; here sounds are decoded by
//! [`AudioV2Sfx::preload`] from whatever the caller fetches them with (the
//! packages), and `play_*` plays only what is loaded, as SHAG does for the
//! preloaded combat bank. Output goes to the sound card, or to an offline
//! mixer that [`AudioV2Sfx::render`] pulls from (tests, the parity harness,
//! `--audio-out`). There is no unlock, epoch or async loading to track.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DVec3;

use crate::audio::decode::{SoundBuffer, decode_sound};
use crate::audio::device::DeviceOutput;
use crate::audio::loop_overlap::{LoopShape, shape_loop};
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

/// How often a missing or lost sound card is looked for again.
const REOPEN_EVERY: Duration = Duration::from_secs(2);
/// Looking for a device blocks the game thread for a moment (the host lists
/// its devices): each failed look waits twice as long, up to this.
const REOPEN_MAX: Duration = Duration::from_secs(30);
/// Commands held for a device that cannot take them; past it, the oldest
/// volume changes go first.
const PENDING_MAX: usize = 4096;
/// The rate sounds are decoded at when there is no device to ask.
const FALLBACK_RATE: u32 = 48_000;

/// The sound card, or the wait for one.
struct Device {
    output: Option<DeviceOutput>,
    /// The rate the sounds were decoded at; a new device is opened at it.
    rate: u32,
    reopen_at: Instant,
    /// The wait after the next failed look.
    reopen_wait: Duration,
    /// A look for a device under way, off the frame's thread: a failed open
    /// costs milliseconds (the sound system reads its configuration each
    /// time), a hitch every retry on a machine with no sound card.
    probe: Option<std::thread::JoinHandle<bool>>,
    /// What must still reach the mixer (streams, stops, volumes) when the
    /// queue was full or there was no device. A one-shot that cannot play
    /// now is dropped instead: late, it would be wrong.
    pending: VecDeque<Command>,
}

/// Commands that change what plays from now on, which a full queue must
/// not lose.
fn must_arrive(command: &Command) -> bool {
    !matches!(
        command,
        Command::Play(Play { looped: false, .. }) | Command::SetListener(_)
    )
}

impl Device {
    fn hold(&mut self, command: Command) {
        let voice = |c: &Command| match c {
            Command::Play(p) => Some(p.id),
            Command::PlayStream { id, .. }
            | Command::Stop(id)
            | Command::SetVoiceVolume(id, _)
            | Command::SetVoicePosition(id, _) => Some(*id),
            _ => None,
        };
        match &command {
            // A newer value replaces the one waiting.
            Command::SetVoiceVolume(id, _) => {
                if let Some(at) = self
                    .pending
                    .iter()
                    .position(|c| matches!(c, Command::SetVoiceVolume(other, _) if other == id))
                {
                    self.pending[at] = command;
                    return;
                }
            }
            Command::SetVoicePosition(id, _) => {
                if let Some(at) = self
                    .pending
                    .iter()
                    .position(|c| matches!(c, Command::SetVoicePosition(other, _) if other == id))
                {
                    self.pending[at] = command;
                    return;
                }
            }
            Command::SetSfxGain(_) | Command::SetMusicGain(_) | Command::SetChannelGain(..) => {
                let same = |c: &Command| match (c, &command) {
                    (Command::SetChannelGain(a, _), Command::SetChannelGain(b, _)) => a == b,
                    _ => std::mem::discriminant(c) == std::mem::discriminant(&command),
                };
                if let Some(at) = self.pending.iter().position(same) {
                    self.pending[at] = command;
                    return;
                }
            }
            // A voice stopped before it reached the mixer never plays: both go.
            Command::Stop(id) => {
                let id = *id;
                let started = self
                    .pending
                    .iter()
                    .any(|c| matches!(c, Command::Play(_) | Command::PlayStream { .. }) && voice(c) == Some(id));
                if started {
                    self.pending.retain(|c| voice(c) != Some(id));
                    return;
                }
            }
            Command::StopAll => {
                self.pending
                    .retain(|c| voice(c).is_none() && !matches!(c, Command::StopAll));
            }
            _ => {}
        }
        // Past the cap only volume changes give way; a stream or a stop is
        // never lost (a lost stream ends at once, and the playlist would
        // start the next, every frame). Stops cancel their plays above, so
        // what waits stays bounded by the voices alive.
        if self.pending.len() >= PENDING_MAX
            && let Some(at) = self
                .pending
                .iter()
                .position(|c| matches!(c, Command::SetVoiceVolume(..)))
        {
            self.pending.remove(at);
        }
        if self.pending.len() < PENDING_MAX || !matches!(command, Command::SetVoiceVolume(..)) {
            self.pending.push_back(command);
        }
    }

    /// Sends what waits, in order, until the queue is full again.
    fn flush(&mut self) {
        let Some(output) = &mut self.output else { return };
        while let Some(command) = self.pending.pop_front() {
            if let Err(back) = output.send(command) {
                self.pending.push_front(back);
                return;
            }
        }
    }

    fn send(&mut self, command: Command) {
        self.flush();
        let back = match &mut self.output {
            Some(output) if self.pending.is_empty() => match output.send(command) {
                Ok(()) => return,
                Err(back) => back,
            },
            _ => command,
        };
        if must_arrive(&back) {
            self.hold(back);
        }
    }
}

enum Output {
    Device(Box<Device>),
    Offline(Box<Mixer>),
}

pub struct AudioV2Sfx {
    output: Output,
    buffers: HashMap<String, Arc<SoundBuffer>>,
    /// Loops built from `buffers` (`getBuffer` with a loop's shape), and
    /// the frame each repeats from.
    shaped: HashMap<String, Option<(Arc<SoundBuffer>, usize)>>,
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
    ///
    /// # Errors
    /// No device would open.
    pub fn open_device() -> Result<Self, String> {
        let output = DeviceOutput::open(None)?;
        Ok(Self::with_output(Output::Device(Box::new(Device {
            rate: output.sample_rate(),
            output: Some(output),
            reopen_at: Instant::now(),
            reopen_wait: REOPEN_EVERY,
            probe: None,
            pending: VecDeque::new(),
        }))))
    }

    /// The sound card's default output, or silence until one appears (the
    /// game looks for it every two seconds, from [`Self::maintain`]).
    #[must_use]
    pub fn open_device_or_wait() -> Self {
        Self::open_device().unwrap_or_else(|why| {
            println!("audio: {why}; looking again from time to time");
            Self::with_output(Output::Device(Box::new(Device {
                output: None,
                rate: FALLBACK_RATE,
                reopen_at: Instant::now() + REOPEN_EVERY,
                reopen_wait: REOPEN_EVERY * 2,
                probe: None,
                pending: VecDeque::new(),
            })))
        })
    }

    /// Whether a sound card is playing now.
    #[must_use]
    pub fn has_device(&self) -> bool {
        match &self.output {
            Output::Device(d) => d.output.is_some(),
            Output::Offline(_) => false,
        }
    }

    /// Once a frame: frees what the mixer finished with, and replaces a
    /// device that died (unplugged, the server gone) or never opened, then
    /// gives it the volumes and whatever waited for it.
    pub fn maintain(&mut self) {
        let Output::Device(device) = &mut self.output else {
            return;
        };
        if let Some(output) = &mut device.output {
            output.collect_garbage();
            if output.lost() {
                println!("audio: the output device is gone; looking for another");
                device.output = None;
                device.reopen_at = Instant::now();
                device.reopen_wait = REOPEN_EVERY;
            }
        }
        // Due for a look: a thread probes, and only a device it found is
        // opened here.
        if device.output.is_none() && device.probe.is_none() && Instant::now() >= device.reopen_at {
            let rate = device.rate;
            device.probe = std::thread::Builder::new()
                .name("audio probe".into())
                .spawn(move || DeviceOutput::probe(Some(rate)))
                .ok();
        }
        let found = match &device.probe {
            Some(probe) if probe.is_finished() => device.probe.take().and_then(|p| p.join().ok()),
            _ => None,
        };
        if found == Some(false) {
            device.reopen_at = Instant::now() + device.reopen_wait;
            device.reopen_wait = (device.reopen_wait * 2).min(REOPEN_MAX);
        }
        if device.output.is_none() && found == Some(true) {
            match DeviceOutput::open(Some(device.rate)) {
                Ok(output) => {
                    if output.sample_rate() != device.rate {
                        println!(
                            "audio: the new device plays {} Hz, not {} Hz; sounds will be off pitch",
                            output.sample_rate(),
                            device.rate
                        );
                    }
                    device.output = Some(output);
                    // The new mixer knows nothing: the volumes go first.
                    let gain = if self.muted { 0.0 } else { self.volume };
                    let mut state = vec![Command::SetSfxGain(gain)];
                    for channel in SfxChannel::ALL {
                        state.push(Command::SetChannelGain(channel, self.channel_volumes[channel as usize]));
                    }
                    for command in state.into_iter().rev() {
                        device.pending.push_front(command);
                    }
                    device.flush();
                }
                Err(_) => {
                    device.reopen_at = Instant::now() + device.reopen_wait;
                    device.reopen_wait = (device.reopen_wait * 2).min(REOPEN_MAX);
                }
            }
        } else {
            device.flush();
        }
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
            shaped: HashMap::new(),
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
            Output::Device(d) => d.rate,
            Output::Offline(m) => m.sample_rate(),
        }
    }

    fn send(&mut self, command: Command) {
        match &mut self.output {
            Output::Device(d) => d.send(command),
            Output::Offline(m) => {
                m.apply(command);
                while m.take_retired().is_some() {}
            }
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
        let listener = Listener::from_camera(position, look, up);
        match &mut self.output {
            Output::Device(d) => {
                if let Some(output) = &mut d.output {
                    output.set_listener(listener);
                }
            }
            Output::Offline(m) => m.apply(Command::SetListener(listener)),
        }
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
            loop_start: 0,
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
            loop_start: 0,
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
        Some(self.start_loop(buffer, 0, position, options))
    }

    /// `playLoop3D` with `LoopSfxPlayOptions`' region, overlap, intro and
    /// pitch: the loop's buffer is built on first use and kept.
    pub fn play_loop_shaped(
        &mut self,
        path: &str,
        position: Option<DVec3>,
        options: &SfxPlayOptions,
        shape: &LoopShape,
    ) -> Option<SfxLoop> {
        let key = format!("{}{}", normalize_path(path), shape.key());
        if !self.shaped.contains_key(&key) {
            let source = self.buffers.get(&normalize_path(path)).cloned()?;
            let attack = shape
                .intro
                .as_ref()
                .and_then(|i| self.buffers.get(&normalize_path(&i.path)).cloned());
            let built = match shape_loop(&source, attack.as_deref(), shape) {
                Ok((buffer, start)) => Some((Arc::new(buffer), start)),
                Err(e) => {
                    eprintln!("[AudioManager] Failed to build loop '{path}': {e}");
                    None
                }
            };
            self.shaped.insert(key.clone(), built);
        }
        let (buffer, start) = self.shaped.get(&key)?.clone()?;
        Some(self.start_loop(buffer, start, position, options))
    }

    /// `SfxLoop.setPosition`: a placed loop moves.
    pub fn set_loop_position(&mut self, handle: SfxLoop, position: DVec3) {
        if self.loops.contains_key(&handle.0) {
            self.send(Command::SetVoicePosition(handle.0, position));
        }
    }

    fn start_loop(
        &mut self,
        buffer: Arc<SoundBuffer>,
        loop_start: usize,
        position: Option<DVec3>,
        options: &SfxPlayOptions,
    ) -> SfxLoop {
        let volume = options.volume.unwrap_or(1.0).max(0.0);
        let id = self.next_id();
        self.loops.insert(id, volume);
        self.send(Command::Play(Play {
            id,
            buffer,
            route: Route::Sfx(options.channel),
            volume: if self.loops_muted { 0.0 } else { volume },
            looped: true,
            loop_start,
            spatial: position.map(|position| Spatial {
                position,
                rolloff: options.attenuation.unwrap_or(attenuation::NORMAL).max(0.0),
            }),
            slot: None,
            instances: None,
        }));
        SfxLoop(id)
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

    #[test]
    fn without_a_device_what_must_arrive_waits_and_one_shots_do_not() {
        let mut device = Device {
            output: None,
            rate: FALLBACK_RATE,
            reopen_at: Instant::now(),
            reopen_wait: REOPEN_EVERY,
            probe: None,
            pending: VecDeque::new(),
        };
        let buffer = Arc::new(decode_sound(&wav(), 48000).unwrap());
        device.send(Command::Play(Play {
            id: 1,
            buffer,
            route: Route::Sfx(SfxChannel::Effects),
            volume: 1.0,
            looped: false,
            loop_start: 0,
            spatial: None,
            slot: None,
            instances: None,
        }));
        assert!(device.pending.is_empty());
        let (_producer, consumer) = rtrb::RingBuffer::<f32>::new(8);
        device.send(Command::PlayStream {
            id: 2,
            samples: consumer,
            volume: 0.0,
        });
        // A fade's steps collapse into the newest.
        for step in 1..=10 {
            device.send(Command::SetVoiceVolume(2, step as f32 / 10.0));
        }
        device.send(Command::Stop(3));
        assert_eq!(device.pending.len(), 3);
        assert!(matches!(device.pending[1], Command::SetVoiceVolume(2, v) if v == 1.0));
        // A stream stopped before it reached the mixer never plays.
        device.send(Command::Stop(2));
        assert_eq!(device.pending.len(), 1);
        assert!(matches!(device.pending[0], Command::Stop(3)));
        // Bus gains keep only their newest value.
        for step in 0..5 {
            device.send(Command::SetSfxGain(step as f32));
            device.send(Command::SetChannelGain(SfxChannel::Effects, step as f32));
        }
        assert_eq!(device.pending.len(), 3);
        // Past the cap a stream still waits.
        for id in 10..10 + PENDING_MAX as u64 {
            device.send(Command::SetVoiceVolume(id, 0.5));
        }
        let (_producer, consumer) = rtrb::RingBuffer::<f32>::new(8);
        device.send(Command::PlayStream {
            id: 99_999,
            samples: consumer,
            volume: 1.0,
        });
        assert!(matches!(
            device.pending.back(),
            Some(Command::PlayStream { id: 99_999, .. })
        ));
    }
}
