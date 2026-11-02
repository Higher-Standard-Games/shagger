//! Decoding sound files to PCM at the mixer's rate, as
//! `AudioEngineV2.createSoundBufferAsync` (Web Audio's `decodeAudioData`)
//! does for `AudioV2Sfx.loadBuffer`: Ogg Vorbis through `lewton`, WAV
//! (PCM and float) read here.
//!
//! RUST.md 22.7 named symphonia, which is MPL-2.0; lewton is MIT or
//! Apache-2.0 and the package holds only Vorbis and WAV. Resampling is
//! linear for now, where browsers use a windowed sinc.

use std::io::Cursor;

/// Decoded PCM at the mixer's rate, interleaved, one or two channels.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundBuffer {
    pub channels: usize,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl SoundBuffer {
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    /// Length in seconds (`buffer.duration`).
    #[must_use]
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate.max(1))
    }
}

/// Decodes an Ogg Vorbis or WAV file (told apart by its magic) and
/// resamples it to `rate`. Files with more than two channels keep the
/// first two.
pub fn decode_sound(bytes: &[u8], rate: u32) -> Result<SoundBuffer, String> {
    let (channels, source_rate, samples) = if bytes.starts_with(b"OggS") {
        decode_vorbis(bytes)?
    } else if bytes.starts_with(b"RIFF") {
        decode_wav(bytes)?
    } else {
        return Err("neither Ogg nor WAV".into());
    };
    let samples = keep_two_channels(channels, samples);
    let channels = channels.min(2);
    Ok(SoundBuffer {
        channels,
        sample_rate: rate,
        samples: resample(channels, source_rate, rate, &samples),
    })
}

fn keep_two_channels(channels: usize, samples: Vec<f32>) -> Vec<f32> {
    if channels <= 2 {
        return samples;
    }
    samples
        .chunks_exact(channels)
        .flat_map(|frame| [frame[0], frame[1]])
        .collect()
}

fn decode_vorbis(bytes: &[u8]) -> Result<(usize, u32, Vec<f32>), String> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let channels = usize::from(reader.ident_hdr.audio_channels);
    let rate = reader.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
        samples.extend(packet.into_iter().map(|s| f32::from(s) / 32768.0));
    }
    Ok((channels, rate, samples))
}

fn decode_wav(bytes: &[u8]) -> Result<(usize, u32, Vec<f32>), String> {
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return Err("not a WAVE file".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let mut format = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(at + 4) as usize;
        let body = at + 8;
        let end = (body + size).min(bytes.len());
        if id == b"fmt " && size >= 16 {
            // tag, channels, rate, bits per sample
            format = Some((
                u16_at(body),
                usize::from(u16_at(body + 2)),
                u32_at(body + 4),
                u16_at(body + 14),
            ));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format.ok_or("data before fmt")?;
            let data = &bytes[body..end];
            // WAVE_FORMAT_EXTENSIBLE names its real format in the subformat.
            let float = tag == 3 || (tag == 0xFFFE && bits == 32 && is_float_extensible(bytes, at));
            let samples: Vec<f32> = match (float, bits) {
                (false, 8) => data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect(),
                (false, 16) => data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0)
                    .collect(),
                (false, 24) => data
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
                    .collect(),
                (false, 32) => data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0)
                    .collect(),
                (true, 32) => data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
                _ => return Err(format!("unsupported WAV format {tag} at {bits} bits")),
            };
            if channels == 0 {
                return Err("no channels".into());
            }
            return Ok((channels, rate, samples));
        }
        at = body + size + (size & 1);
    }
    Err("no data chunk".into())
}

/// Whether the `fmt ` chunk before `data_at` is an extensible float format.
fn is_float_extensible(bytes: &[u8], data_at: usize) -> bool {
    // The subformat GUID starts 24 bytes into a 40-byte fmt body; its first
    // two bytes are the format tag.
    let Some(fmt) = bytes[..data_at].windows(4).position(|w| w == b"fmt ") else {
        return false;
    };
    let tag = fmt + 8 + 24;
    tag + 2 <= bytes.len() && u16::from_le_bytes([bytes[tag], bytes[tag + 1]]) == 3
}

/// Linear interpolation from `from` to `to` Hz.
#[must_use]
pub fn resample(channels: usize, from: u32, to: u32, samples: &[f32]) -> Vec<f32> {
    if from == to || from == 0 || channels == 0 {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    let out_frames = (frames as u64 * u64::from(to)).div_ceil(u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    let mut out = Vec::with_capacity(out_frames * channels);
    for i in 0..out_frames {
        let at = i as f64 * step;
        let i0 = (at as usize).min(frames - 1);
        let i1 = (i0 + 1).min(frames - 1);
        let t = (at - i0 as f64) as f32;
        for c in 0..channels {
            let a = samples[i0 * channels + c];
            let b = samples[i1 * channels + c];
            out.push(a + (b - a) * t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_pcm_wav() {
        let sound = decode_sound(&wav(2, 48000, &[16384, -16384, 0, 32767]), 48000).unwrap();
        assert_eq!(sound.channels, 2);
        assert_eq!(sound.frames(), 2);
        assert_eq!(sound.samples[..2], [0.5, -0.5]);
    }

    #[test]
    fn resamples_to_the_mixer_rate() {
        let sound = decode_sound(&wav(1, 24000, &[0, 16384, 0, -16384]), 48000).unwrap();
        assert_eq!(sound.frames(), 8);
        assert_eq!(sound.samples[1], 0.25);
        assert!((sound.duration() - 4.0 / 24000.0).abs() < 1e-12);
    }
}
