//! The sound card: a cpal output stream whose callback owns the [`Mixer`]
//! and takes its [`Command`]s from a lock-free queue (RUST.md 22.7), so the
//! audio thread never locks or allocates.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::mixer::{Command, Mixer};

/// Commands waiting at most; a full queue drops the newest.
const QUEUE: usize = 1024;
/// Frames the callback mixes at a time.
const SCRATCH_FRAMES: usize = 1024;

/// An open output and the queue into its mixer.
pub struct DeviceOutput {
    commands: rtrb::Producer<Command>,
    sample_rate: u32,
    _stream: cpal::Stream,
}

impl DeviceOutput {
    /// Opens the default output device at its default rate, in f32 when it
    /// takes f32 and otherwise in its own format.
    pub fn open() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let sample_rate = config.sample_rate;
        let channels = usize::from(config.channels);
        let (commands, mut queue) = rtrb::RingBuffer::<Command>::new(QUEUE);
        let mut mixer = Mixer::new(sample_rate);
        let mut scratch = vec![0.0f32; SCRATCH_FRAMES * 2];
        let mut fill = move |out: &mut dyn FnMut(usize, f32), frames: usize| {
            while let Ok(command) = queue.pop() {
                mixer.apply(command);
            }
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(SCRATCH_FRAMES);
                let mix = &mut scratch[..n * 2];
                mixer.render(mix);
                for f in 0..n {
                    for c in 0..channels {
                        // Stereo into the first two channels; silence after.
                        let sample = if c < 2 { mix[f * 2 + c] } else { 0.0 };
                        out((done + f) * channels + c, sample);
                    }
                }
                done += n;
            }
        };
        let error = |e: cpal::Error| eprintln!("audio: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;
                    fill(&mut |i, s| data[i] = s, frames);
                },
                error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;
                    fill(&mut |i, s| data[i] = (s.clamp(-1.0, 1.0) * 32767.0) as i16, frames);
                },
                error,
                None,
            ),
            other => return Err(format!("unsupported sample format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            commands,
            sample_rate,
            _stream: stream,
        })
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Queues a command; a full queue drops it.
    pub fn send(&mut self, command: Command) {
        let _ = self.commands.push(command);
    }
}
