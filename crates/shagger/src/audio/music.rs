//! Music playlists (SHAG `src/engine/AudioManager.ts`: `Playlist`,
//! `startPlaylist`, `stopPlaylist`, `playCurrentTrack`, `nextTrack`,
//! `fadeInMusic`, `fadeOutMusic`, `setMusicVolume`, `setMusicEnabled`,
//! `setPlaylistMuted`).
//!
//! The browser plays each track in an `<audio>` element; here an MP3 is
//! decoded with `rmp3` (CC0, around the CC0 C library minimp3; the
//! `minimp3` crate pulls in `slice-ring-buffer`, RUSTSEC-2025-0044) and streamed
//! into the mixer's music bus through a ring buffer. On a sound card a
//! thread decodes ahead; offline (`AudioV2Sfx::offline`) `update` decodes
//! on the caller's thread, so a render to a file is the same every run.
//!
//! Fades step the element volume by 0.02 every 50 ms as the TypeScript's
//! `setInterval`s do, counted from `update`'s `dt`. A track that cannot be
//! found or decoded is skipped (the playlists list one that the package
//! does not hold).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::audio::audio_v2_sfx::AudioV2Sfx;
use crate::audio::mixer::VoiceId;
use crate::shared::json::{self, Value};

/// `PlaylistTrack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistTrack {
    pub file: String,
    pub title: Option<String>,
}

/// `Playlist`: `loop` is true unless the file says false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub name: Option<String>,
    pub looped: bool,
    pub shuffle: bool,
    pub tracks: Vec<PlaylistTrack>,
}

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

impl Playlist {
    /// Reads a `*.playlist.json`.
    pub fn parse(source: &str) -> Result<Self, String> {
        let root = json::parse(source).map_err(|e| format!("playlist: {e:?}"))?;
        let Some(Value::Array(items)) = field(&root, "tracks") else {
            return Err("playlist: no tracks".into());
        };
        let tracks = items
            .iter()
            .filter_map(|t| {
                Some(PlaylistTrack {
                    file: text(field(t, "file"))?,
                    title: text(field(t, "title")),
                })
            })
            .collect();
        Ok(Self {
            name: text(field(&root, "name")),
            looped: !matches!(field(&root, "loop"), Some(Value::Bool(false))),
            shuffle: matches!(field(&root, "shuffle"), Some(Value::Bool(true))),
            tracks,
        })
    }
}

/// An MP3 decoded and resampled to stereo at the mixer's rate.
struct TrackDecoder {
    decoder: rmp3::DecoderOwned<Vec<u8>>,
    rate: u32,
    /// Source frames not yet passed, and where the next output frame
    /// falls among them.
    source: Vec<[f32; 2]>,
    source_rate: u32,
    position: f64,
    pending: Vec<f32>,
    done: bool,
}

impl TrackDecoder {
    fn new(bytes: Vec<u8>, rate: u32) -> Result<Self, String> {
        let mut decoder = Self {
            decoder: rmp3::DecoderOwned::new(bytes),
            rate,
            source: Vec::new(),
            source_rate: 0,
            position: 0.0,
            pending: Vec::new(),
            done: false,
        };
        // A file that yields no audio is not a track.
        decoder.decode_more();
        if decoder.done && decoder.pending.is_empty() {
            return Err("not an MP3".into());
        }
        Ok(decoder)
    }

    /// Decodes a frame into `pending`; sets `done` at the end.
    fn decode_more(&mut self) {
        loop {
            match self.decoder.next() {
                Some(rmp3::Frame::Audio(audio)) => {
                    let channels = usize::from(audio.channels());
                    if channels == 0 || audio.sample_rate() == 0 {
                        continue;
                    }
                    self.source_rate = audio.sample_rate();
                    for chunk in audio.samples().chunks_exact(channels) {
                        let l = f32::from(chunk[0]) / 32768.0;
                        let r = if channels > 1 { f32::from(chunk[1]) / 32768.0 } else { l };
                        self.source.push([l, r]);
                    }
                    self.resample(false);
                    if !self.pending.is_empty() {
                        return;
                    }
                }
                Some(rmp3::Frame::Other(_)) => {}
                None => {
                    self.resample(true);
                    self.done = true;
                    return;
                }
            }
        }
    }

    fn resample(&mut self, last: bool) {
        if self.source.is_empty() {
            return;
        }
        let step = f64::from(self.source_rate) / f64::from(self.rate);
        let frames = self.source.len();
        loop {
            let i0 = self.position as usize;
            if i0 + 1 >= frames && !(last && i0 < frames) {
                break;
            }
            let i1 = (i0 + 1).min(frames - 1);
            let t = (self.position - i0 as f64) as f32;
            for c in 0..2 {
                let a = self.source[i0][c];
                let b = self.source[i1][c];
                self.pending.push(a + (b - a) * t);
            }
            self.position += step;
        }
        let used = (self.position as usize).min(frames);
        self.source.drain(..used);
        self.position -= used as f64;
    }

    /// Fills `producer`; true once everything decoded has been pushed.
    fn pump(&mut self, producer: &mut rtrb::Producer<f32>) -> bool {
        loop {
            if self.pending.is_empty() {
                if self.done {
                    return true;
                }
                self.decode_more();
                continue;
            }
            let room = producer.slots() & !1;
            if room == 0 {
                return false;
            }
            let n = room.min(self.pending.len());
            if let Ok(chunk) = producer.write_chunk_uninit(n) {
                chunk.fill_from_iter(self.pending.drain(..n));
            }
        }
    }
}

/// Feeds one stream: from a thread on a sound card, or from `update`.
enum Feeder {
    Thread {
        ended: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    },
    Inline {
        decoder: Box<TrackDecoder>,
        producer: rtrb::Producer<f32>,
        decoded: bool,
    },
}

impl Feeder {
    /// The track has played to its end (all of it reached the mixer).
    fn ended(&mut self) -> bool {
        match self {
            Feeder::Thread { ended, .. } => ended.load(Ordering::Acquire),
            Feeder::Inline {
                decoder,
                producer,
                decoded,
            } => {
                if !*decoded {
                    *decoded = decoder.pump(producer);
                }
                *decoded && producer.slots() == producer.buffer().capacity()
            }
        }
    }
}

impl Drop for Feeder {
    fn drop(&mut self) {
        if let Feeder::Thread { stop, handle, .. } = self {
            stop.store(true, Ordering::Release);
            // Woken and left to finish on its own: joining here would hold
            // the frame for up to its 20 ms nap.
            if let Some(handle) = handle.take() {
                handle.thread().unpark();
            }
        }
    }
}

struct Track {
    id: VoiceId,
    volume: f32,
    feeder: Feeder,
}

/// The fade `setInterval`s' period, and how much each tick moves.
const FADE_TICK: f64 = 0.05;
const FADE_STEP: f32 = 0.02;

/// `AudioManager`'s music half.
pub struct MusicPlayer {
    current: Option<Track>,
    fading_in: bool,
    fading_out: Option<Track>,
    fade_clock: f64,
    music_volume: f32,
    music_enabled: bool,
    playlist: Option<Playlist>,
    playlist_name: Option<String>,
    muted_playlists: Vec<String>,
    playing_playlist: bool,
    track_index: usize,
    failed_attempts: usize,
    rng: u64,
}

impl Default for MusicPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicPlayer {
    #[must_use]
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            current: None,
            fading_in: false,
            fading_out: None,
            fade_clock: 0.0,
            music_volume: 0.5,
            music_enabled: true,
            playlist: None,
            playlist_name: None,
            muted_playlists: Vec::new(),
            playing_playlist: false,
            track_index: 0,
            failed_attempts: 0,
            rng: seed | 1,
        }
    }

    /// Seeds the shuffle (tests and offline renders).
    pub fn set_shuffle_seed(&mut self, seed: u64) {
        self.rng = seed | 1;
    }

    fn random(&mut self) -> f64 {
        // xorshift64*: `Math.random` stands in, any spread will do.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    #[must_use]
    pub fn music_volume(&self) -> f32 {
        self.music_volume
    }

    #[must_use]
    pub fn is_music_enabled(&self) -> bool {
        self.music_enabled
    }

    #[must_use]
    pub fn is_playlist_playing(&self) -> bool {
        self.playing_playlist
    }

    #[must_use]
    pub fn current_playlist_name(&self) -> Option<&str> {
        self.playlist_name.as_deref()
    }

    /// The track playing now (`getCurrentPlaylistInfo().currentTrack`).
    #[must_use]
    pub fn current_track(&self) -> Option<&PlaylistTrack> {
        self.playlist.as_ref()?.tracks.get(self.track_index)
    }

    #[must_use]
    pub fn is_playlist_muted(&self, name: &str) -> bool {
        self.muted_playlists.iter().any(|n| n == name)
    }

    /// Whether a track is sounding (fading or not).
    #[must_use]
    pub fn has_music(&self) -> bool {
        self.current.is_some()
    }

    /// `setMusicVolume` (`s_volume`).
    pub fn set_music_volume(&mut self, audio: &mut AudioV2Sfx, volume: f32) {
        // The same volume again changes nothing: a fade in keeps fading.
        let volume = volume.clamp(0.0, 1.0);
        if volume == self.music_volume {
            return;
        }
        self.music_volume = volume;
        if let Some(track) = &mut self.current {
            track.volume = self.music_volume;
            audio.set_voice_volume(track.id, track.volume);
        }
    }

    /// `setMusicEnabled` (`s_musicenabled`).
    pub fn set_music_enabled(
        &mut self,
        audio: &mut AudioV2Sfx,
        fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
        enabled: bool,
    ) {
        if self.music_enabled == enabled {
            return;
        }
        self.music_enabled = enabled;
        if !enabled {
            self.stop_music(audio, false);
        } else if self.playing_playlist && self.current.is_none() {
            self.play_current_track(audio, fetch, true);
        }
    }

    /// `setPlaylistMuted`.
    pub fn set_playlist_muted(
        &mut self,
        audio: &mut AudioV2Sfx,
        fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
        name: &str,
        muted: bool,
    ) {
        if muted == self.is_playlist_muted(name) {
            return;
        }
        if muted {
            self.muted_playlists.push(name.to_owned());
        } else {
            self.muted_playlists.retain(|n| n != name);
        }
        if self.playlist_name.as_deref() != Some(name) {
            return;
        }
        if muted {
            self.stop_music(audio, true);
        } else if self.playing_playlist && self.current.is_none() {
            self.play_current_track(audio, fetch, true);
        }
    }

    /// `startPlaylist`: `fetch` reads a file from the packages (it is asked
    /// for the track's path, then `music/<path>`).
    pub fn start_playlist(
        &mut self,
        audio: &mut AudioV2Sfx,
        fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
        mut playlist: Playlist,
        name: Option<&str>,
    ) {
        if name.is_some() && self.playlist_name.as_deref() == name && self.playing_playlist {
            return;
        }
        self.stop_music(audio, false);
        if playlist.shuffle {
            // Fisher-Yates, as `shufflePlaylist`.
            for i in (1..playlist.tracks.len()).rev() {
                let j = (self.random() * (i + 1) as f64) as usize;
                playlist.tracks.swap(i, j);
            }
        }
        self.playlist = Some(playlist);
        self.playlist_name = name.map(str::to_owned);
        self.track_index = 0;
        self.playing_playlist = true;
        if self.music_enabled {
            self.play_current_track(audio, fetch, true);
        }
    }

    /// `stopPlaylist`.
    pub fn stop_playlist(&mut self, audio: &mut AudioV2Sfx, fade_out: bool) {
        self.playing_playlist = false;
        self.playlist = None;
        self.playlist_name = None;
        self.stop_music(audio, fade_out);
    }

    /// `stopMusic`.
    pub fn stop_music(&mut self, audio: &mut AudioV2Sfx, fade_out: bool) {
        if let Some(old) = self.fading_out.take() {
            audio.stop_voice(old.id);
        }
        let Some(track) = self.current.take() else {
            return;
        };
        self.fading_in = false;
        if fade_out {
            self.fading_out = Some(track);
        } else {
            audio.stop_voice(track.id);
        }
    }

    /// Starts `fetch`ed track bytes on the music bus.
    fn start_track(&mut self, audio: &mut AudioV2Sfx, bytes: Vec<u8>, fade_in: bool) -> bool {
        let rate = audio.sample_rate();
        let Ok(mut decoder) = TrackDecoder::new(bytes, rate) else {
            return false;
        };
        // A second of music ahead.
        let (mut producer, consumer) = rtrb::RingBuffer::new(rate as usize * 2);
        let volume = if fade_in { 0.0 } else { self.music_volume };
        let feeder = if audio.offline_mixer().is_some() {
            let decoded = decoder.pump(&mut producer);
            Feeder::Inline {
                decoder: Box::new(decoder),
                producer,
                decoded,
            }
        } else {
            let ended = Arc::new(AtomicBool::new(false));
            let stop = Arc::new(AtomicBool::new(false));
            let (thread_ended, thread_stop) = (ended.clone(), stop.clone());
            let handle = std::thread::Builder::new()
                .name("music".into())
                .spawn(move || {
                    let mut decoded = false;
                    while !thread_stop.load(Ordering::Acquire) {
                        if !decoded {
                            decoded = decoder.pump(&mut producer);
                        }
                        // Played out, or the device that played it is gone
                        // (its mixer dropped the queue): the next track.
                        if (decoded && producer.slots() == producer.buffer().capacity()) || producer.is_abandoned() {
                            thread_ended.store(true, Ordering::Release);
                            return;
                        }
                        std::thread::park_timeout(Duration::from_millis(20));
                    }
                })
                .ok();
            Feeder::Thread { ended, stop, handle }
        };
        let id = audio.play_stream(consumer, volume);
        self.current = Some(Track { id, volume, feeder });
        if fade_in {
            self.fading_in = true;
            self.fade_clock = 0.0;
        }
        true
    }

    /// `playCurrentTrack`.
    fn play_current_track(
        &mut self,
        audio: &mut AudioV2Sfx,
        fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
        fade_in: bool,
    ) {
        // Skipping a missing track recurses through `next_track`; a loop
        // bounds it here.
        let mut fade_in = fade_in;
        loop {
            let Some(playlist) = &self.playlist else {
                return;
            };
            if !self.playing_playlist || !self.music_enabled {
                return;
            }
            if self.playlist_name.as_ref().is_some_and(|n| self.is_playlist_muted(n)) {
                return;
            }
            let Some(track) = playlist.tracks.get(self.track_index) else {
                return;
            };
            let file = track.file.clone();
            let count = playlist.tracks.len();
            if self.current.is_some() {
                self.stop_music(audio, fade_in);
            }
            let bytes = fetch(&file).or_else(|| fetch(&format!("music/{file}")));
            if bytes.is_some_and(|b| self.start_track(audio, b, fade_in)) {
                self.failed_attempts = 0;
                return;
            }
            self.failed_attempts += 1;
            if self.failed_attempts >= count {
                self.playing_playlist = false;
                self.failed_attempts = 0;
                return;
            }
            if !self.advance() {
                return;
            }
            fade_in = false;
        }
    }

    /// `nextTrack`'s index step: false when a playlist that does not loop
    /// is over.
    fn advance(&mut self) -> bool {
        let Some(playlist) = &self.playlist else {
            return false;
        };
        if !self.playing_playlist {
            return false;
        }
        self.track_index += 1;
        if self.track_index >= playlist.tracks.len() {
            if playlist.looped {
                self.track_index = 0;
            } else {
                self.playing_playlist = false;
                return false;
            }
        }
        true
    }

    /// The fades' ticks, and the next track when one ends. Call every
    /// frame.
    pub fn update(&mut self, audio: &mut AudioV2Sfx, fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>, dt: f64) {
        if self.fading_in || self.fading_out.is_some() {
            self.fade_clock += dt;
            while self.fade_clock >= FADE_TICK {
                self.fade_clock -= FADE_TICK;
                self.fade_tick(audio);
            }
        } else {
            self.fade_clock = 0.0;
        }
        if let Some(old) = &mut self.fading_out
            && old.feeder.ended()
        {
            audio.stop_voice(old.id);
            self.fading_out = None;
        }
        let ended = self.current.as_mut().is_some_and(|t| t.feeder.ended());
        if ended {
            if let Some(track) = self.current.take() {
                audio.stop_voice(track.id);
            }
            self.fading_in = false;
            if self.advance() {
                // No fade between tracks.
                self.play_current_track(audio, fetch, false);
            }
        }
    }

    fn fade_tick(&mut self, audio: &mut AudioV2Sfx) {
        if self.fading_in {
            if let Some(track) = &mut self.current {
                let volume = track.volume + FADE_STEP;
                if volume >= self.music_volume {
                    track.volume = self.music_volume;
                    self.fading_in = false;
                } else {
                    track.volume = volume;
                }
                audio.set_voice_volume(track.id, track.volume);
            } else {
                self.fading_in = false;
            }
        }
        if let Some(old) = &mut self.fading_out {
            if old.volume > FADE_STEP {
                old.volume -= FADE_STEP;
                audio.set_voice_volume(old.id, old.volume);
            } else {
                audio.stop_voice(old.id);
                self.fading_out = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST: &str = r#"{"name":"Game","loop":true,"shuffle":false,
        "tracks":[{"file":"missing.mp3","title":"Gone"},{"file":"a.mp3","title":"A"}]}"#;

    #[test]
    fn reads_a_playlist() {
        let p = Playlist::parse(PLAYLIST).unwrap();
        assert_eq!(p.name.as_deref(), Some("Game"));
        assert!(p.looped && !p.shuffle);
        assert_eq!(p.tracks.len(), 2);
        assert_eq!(p.tracks[1].title.as_deref(), Some("A"));
        let once = Playlist::parse(r#"{"loop":false,"tracks":[]}"#).unwrap();
        assert!(!once.looped);
    }

    #[test]
    fn a_playlist_with_nothing_to_play_stops() {
        let mut audio = AudioV2Sfx::offline(48000);
        let mut music = MusicPlayer::new();
        let mut fetch = |_: &str| -> Option<Vec<u8>> { None };
        music.start_playlist(&mut audio, &mut fetch, Playlist::parse(PLAYLIST).unwrap(), Some("game"));
        assert!(!music.is_playlist_playing());
        assert!(!music.has_music());
    }

    #[test]
    fn shuffles_every_track_once() {
        let mut audio = AudioV2Sfx::offline(48000);
        let mut music = MusicPlayer::new();
        music.set_shuffle_seed(7);
        music.set_music_enabled(&mut audio, &mut |_| None, false);
        let tracks = (0..20)
            .map(|i| PlaylistTrack {
                file: format!("{i}.mp3"),
                title: None,
            })
            .collect();
        let playlist = Playlist {
            name: None,
            looped: true,
            shuffle: true,
            tracks,
        };
        music.start_playlist(&mut audio, &mut |_| None, playlist, Some("x"));
        let order: Vec<_> = music
            .playlist
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.file.clone())
            .collect();
        let mut sorted = order.clone();
        sorted.sort();
        let mut expected: Vec<_> = (0..20).map(|i| format!("{i}.mp3")).collect();
        expected.sort();
        assert_eq!(sorted, expected);
        assert_ne!(order[0..5], ["0.mp3", "1.mp3", "2.mp3", "3.mp3", "4.mp3"]);
        // Music off keeps the playlist for later.
        assert!(music.is_playlist_playing() && !music.has_music());
    }
}

#[cfg(test)]
mod real_track {
    use super::*;

    #[test]
    fn plays_a_real_track_when_given_one() {
        let Ok(path) = std::env::var("SHAGGER_TEST_MP3") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let mut audio = AudioV2Sfx::offline(48000);
        let mut music = MusicPlayer::new();
        let playlist = Playlist::parse(r#"{"tracks":[{"file":"t.mp3"}]}"#).unwrap();
        let mut fetch = |p: &str| (p == "t.mp3").then(|| bytes.clone());
        let t = std::time::Instant::now();
        music.start_playlist(&mut audio, &mut fetch, playlist, Some("g"));
        assert!(music.has_music());
        let mut out = vec![0.0; 960 * 2];
        let mut peak = 0.0f32;
        for _ in 0..500 {
            music.update(&mut audio, &mut fetch, 0.02);
            out.fill(0.0);
            audio.render(&mut out);
            peak = out.iter().fold(peak, |p, s| p.max(s.abs()));
        }
        eprintln!("10 s in {:?}, peak {peak}", t.elapsed());
        assert!(peak > 0.05 && peak <= 0.6);

        // About two seconds of it, in a playlist that plays once: it ends
        // and the playlist is over.
        let short = bytes[..40_000].to_vec();
        let playlist = Playlist::parse(r#"{"loop":false,"tracks":[{"file":"s.mp3"}]}"#).unwrap();
        let mut fetch = |p: &str| (p == "s.mp3").then(|| short.clone());
        music.start_playlist(&mut audio, &mut fetch, playlist, Some("s"));
        assert!(music.has_music());
        for _ in 0..250 {
            music.update(&mut audio, &mut fetch, 0.02);
            audio.render(&mut out);
        }
        assert!(!music.has_music() && !music.is_playlist_playing());
    }
}
