//! The sound card: a cpal output stream whose callback owns the [`Mixer`]
//! and takes its [`Command`]s from a lock-free queue (RUST.md 22.7), so the
//! audio thread never blocks, allocates or frees.
//!
//! - The listener moves every frame, so it is not queued: the newest one
//!   waits in a slot the callback takes when it can, and a stalled device
//!   cannot fill the queue with stale positions.
//! - A full queue hands the command back ([`DeviceOutput::send`]); the
//!   caller decides what must still arrive (`AudioV2Sfx`).
//! - Stream queues the mixer is done with come back through a second queue,
//!   to be freed on the game thread.
//! - A stream that dies (the device unplugged, the server gone) says so
//!   ([`DeviceOutput::lost`]), for the caller to open another.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

use crate::audio::mixer::{Command, Mixer};
use crate::audio::spatial::Listener;

/// Commands waiting at most.
const QUEUE: usize = 1024;
/// Frames the callback mixes at a time.
const SCRATCH_FRAMES: usize = 1024;
/// Finished stream queues waiting to be freed.
const RETIRED: usize = 16;

/// An open output and the queue into its mixer.
pub struct DeviceOutput {
    commands: rtrb::Producer<Command>,
    listener: Arc<Mutex<Option<Listener>>>,
    retired: rtrb::Consumer<rtrb::Consumer<f32>>,
    lost: Arc<AtomicBool>,
    sample_rate: u32,
    _stream: cpal::Stream,
}

/// The default device's config: `rate` when it plays it (a reopened device
/// keeps the rate the sounds were decoded at), f32 when it takes f32, else
/// whatever it prefers.
fn choose_config(device: &cpal::Device, rate: Option<u32>) -> Result<cpal::SupportedStreamConfig, String> {
    let default = device.default_output_config().map_err(|e| e.to_string())?;
    let want = rate.unwrap_or(default.sample_rate());
    let ranges: Vec<_> = device
        .supported_output_configs()
        .map(Iterator::collect)
        .unwrap_or_default();
    let fits = |r: &cpal::SupportedStreamConfigRange| want >= r.min_sample_rate() && want <= r.max_sample_rate();
    // Same channels as the default, the rate wanted, f32 first.
    let pick = ranges
        .iter()
        .filter(|r| r.channels() == default.channels() && fits(r))
        .min_by_key(|r| r.sample_format() != cpal::SampleFormat::F32)
        .and_then(|r| r.try_with_sample_rate(want));
    Ok(pick.unwrap_or(default))
}

impl DeviceOutput {
    /// Whether a default output device is there and offers a config,
    /// without opening a stream. The slow part of a failed [`Self::open`]
    /// (the sound system's configuration, read afresh each time), safe on
    /// any thread.
    #[must_use]
    pub fn probe(rate: Option<u32>) -> bool {
        cpal::default_host()
            .default_output_device()
            .is_some_and(|device| choose_config(&device, rate).is_ok())
    }

    /// Opens the default output device, at `rate` when it can play it.
    ///
    /// # Errors
    /// No device, or none that would open.
    pub fn open(rate: Option<u32>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no output device")?;
        let supported = choose_config(&device, rate)?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let sample_rate = config.sample_rate;
        let (commands, queue) = rtrb::RingBuffer::<Command>::new(QUEUE);
        let (graveyard, retired) = rtrb::RingBuffer::<rtrb::Consumer<f32>>::new(RETIRED);
        let listener = Arc::new(Mutex::new(None));
        let lost = Arc::new(AtomicBool::new(false));
        let feed = Feed {
            mixer: Mixer::new(sample_rate),
            queue,
            graveyard,
            listener: listener.clone(),
            scratch: vec![0.0f32; SCRATCH_FRAMES * 2],
            channels: usize::from(config.channels),
        };
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, feed, &lost),
            cpal::SampleFormat::F64 => build::<f64>(&device, config, feed, &lost),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, feed, &lost),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, feed, &lost),
            cpal::SampleFormat::I8 => build::<i8>(&device, config, feed, &lost),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, feed, &lost),
            cpal::SampleFormat::U32 => build::<u32>(&device, config, feed, &lost),
            cpal::SampleFormat::U8 => build::<u8>(&device, config, feed, &lost),
            other => return Err(format!("unsupported sample format {other}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            commands,
            listener,
            retired,
            lost,
            sample_rate,
            _stream: stream,
        })
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Queues a command, or hands it back when the queue is full.
    ///
    /// # Errors
    /// The queue is full; the command comes back.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands.push(command).map_err(|rtrb::PushError::Full(c)| c)
    }

    /// The listener for the next callback, replacing one not yet taken.
    pub fn set_listener(&mut self, listener: Listener) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = Some(listener);
        }
    }

    /// Frees what the mixer finished with, here rather than on its thread.
    pub fn collect_garbage(&mut self) {
        while self.retired.pop().is_ok() {}
    }

    /// The stream died and will play nothing more.
    #[must_use]
    pub fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

/// What the callback owns.
struct Feed {
    mixer: Mixer,
    queue: rtrb::Consumer<Command>,
    graveyard: rtrb::Producer<rtrb::Consumer<f32>>,
    listener: Arc<Mutex<Option<Listener>>>,
    scratch: Vec<f32>,
    channels: usize,
}

impl Feed {
    fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T]) {
        while let Ok(command) = self.queue.pop() {
            self.mixer.apply(command);
        }
        // Never waits: a frame the game thread holds the slot keeps the
        // last listener.
        if let Ok(mut slot) = self.listener.try_lock()
            && let Some(listener) = slot.take()
        {
            self.mixer.apply(Command::SetListener(listener));
        }
        while !self.graveyard.is_full()
            && let Some(samples) = self.mixer.take_retired()
        {
            let _ = self.graveyard.push(samples);
        }
        let channels = self.channels.max(1);
        let frames = data.len() / channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(SCRATCH_FRAMES);
            let mix = &mut self.scratch[..n * 2];
            self.mixer.render(mix);
            for f in 0..n {
                let (l, r) = (mix[f * 2], mix[f * 2 + 1]);
                let frame = &mut data[(done + f) * channels..(done + f + 1) * channels];
                if channels == 1 {
                    // Mono: both sides, or a sound panned right is lost.
                    frame[0] = T::from_sample((l + r) * 0.5);
                } else {
                    // Stereo into the first two channels; silence after.
                    frame[0] = T::from_sample(l.clamp(-1.0, 1.0));
                    frame[1] = T::from_sample(r.clamp(-1.0, 1.0));
                    for c in &mut frame[2..] {
                        *c = T::from_sample(0.0f32);
                    }
                }
            }
            done += n;
        }
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut feed: Feed,
    lost: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let lost = lost.clone();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| feed.fill(data),
            move |e: cpal::Error| {
                eprintln!("audio: {e}");
                if matches!(
                    e.kind(),
                    cpal::ErrorKind::DeviceNotAvailable
                        | cpal::ErrorKind::StreamInvalidated
                        | cpal::ErrorKind::HostUnavailable
                ) {
                    lost.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}
