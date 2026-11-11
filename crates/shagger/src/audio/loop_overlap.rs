//! SHAG `src/engine/audio/LoopOverlap.ts`: a loop that crossfades its own
//! tail into its head, so it repeats without a click, and an attack played
//! once in front of it. Plus the buffer `AudioV2Sfx.createBuffer` builds
//! from them for a loop's region, intro and pitch.

use crate::audio::decode::SoundBuffer;

/// `overlapLoopChannel`: the last `overlap_frames` fade (equal power) into
/// the first, and the loop is that much shorter.
#[must_use]
pub fn overlap_loop_channel(samples: &[f32], overlap_frames: f64) -> Vec<f32> {
    let overlap = if overlap_frames.is_finite() {
        (overlap_frames.floor().max(0.0) as usize).min(samples.len() / 2)
    } else {
        0
    };
    if overlap < 2 {
        return samples.to_vec();
    }
    let length = samples.len() - overlap;
    let mut result = samples[..length].to_vec();
    for i in 0..overlap {
        let angle = (i as f64 / (overlap - 1) as f64) * std::f64::consts::FRAC_PI_2;
        result[i] = (f64::from(samples[length + i]) * angle.cos() + f64::from(samples[i]) * angle.sin()) as f32;
    }
    result
}

/// `prependLoopIntro`: the attack's first `hold_frames`, then
/// `fade_frames` of it fading into the loop's tail, then the loop.
#[must_use]
pub fn prepend_loop_intro(attack: &[f32], looped: &[f32], hold_frames: f64, fade_frames: f64) -> Vec<f32> {
    let hold = (hold_frames.floor().max(0.0) as usize).min(attack.len());
    let fade = (fade_frames.floor().max(0.0) as usize)
        .min(looped.len())
        .min(attack.len() - hold);
    let mut result = Vec::with_capacity(hold + fade + looped.len());
    result.extend_from_slice(&attack[..hold]);
    for i in 0..fade {
        let t = if fade > 1 { i as f64 / (fade - 1) as f64 } else { 1.0 };
        let angle = t * std::f64::consts::FRAC_PI_2;
        result.push(
            (f64::from(attack[hold + i]) * angle.cos() + f64::from(looped[looped.len() - fade + i]) * angle.sin())
                as f32,
        );
    }
    result.extend_from_slice(looped);
    result
}

/// `LoopSfxPlayOptions.loopRegion`, seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopRegion {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// `LoopSfxPlayOptions.intro`: the attack plays this long, then fades.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopIntro {
    pub path: String,
    pub hold_seconds: f64,
    pub fade_seconds: f64,
}

/// What shapes a loop's buffer (`LoopSfxPlayOptions`' buffer half).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoopShape {
    pub loop_region: Option<LoopRegion>,
    pub overlap_seconds: f64,
    pub intro: Option<LoopIntro>,
    /// Cents; Babylon's `pitch` (a playback rate of 2^(cents/1200)).
    pub pitch: f64,
}

impl LoopShape {
    /// `getBuffer`'s cache key suffix.
    #[must_use]
    pub fn key(&self) -> String {
        let mut key = String::new();
        if self.overlap_seconds > 0.0 {
            key += &format!("#overlap={}", self.overlap_seconds);
        }
        if let Some(i) = &self.intro {
            key += &format!("#intro={}:{}:{}", i.path, i.hold_seconds, i.fade_seconds);
        }
        if let Some(r) = &self.loop_region {
            key += &format!("#region={}:{}", r.start_seconds, r.end_seconds);
        }
        if self.pitch != 0.0 {
            key += &format!("#pitch={}", self.pitch);
        }
        key
    }
}

fn channel(buffer: &SoundBuffer, c: usize) -> Vec<f32> {
    let n = buffer.channels.max(1);
    buffer.samples.iter().skip(c).step_by(n).copied().collect()
}

/// `createBuffer`: the loop `source` shaped by `shape`, with the frame the
/// repeat starts at (after the intro). `attack` is the intro's sound.
///
/// # Errors
/// A bad region, a bad intro length, or an intro whose channels differ.
pub fn shape_loop(
    source: &SoundBuffer,
    attack: Option<&SoundBuffer>,
    shape: &LoopShape,
) -> Result<(SoundBuffer, usize), String> {
    let rate = f64::from(source.sample_rate);
    let frames = source.frames();
    let (start, end) = match &shape.loop_region {
        Some(r) => {
            let start = (r.start_seconds * rate).round().max(0.0);
            let end = (r.end_seconds * rate).round().min(frames as f64);
            if !start.is_finite() || !end.is_finite() || start >= end {
                return Err("Invalid loop region".to_owned());
            }
            (start as usize, end as usize)
        }
        None => (0, frames),
    };
    let overlap = (shape.overlap_seconds.max(0.0) * rate).round();
    let mut channels: Vec<Vec<f32>> = (0..source.channels)
        .map(|c| overlap_loop_channel(&channel(source, c)[start..end], overlap))
        .collect();
    let loop_frames = channels.first().map_or(0, Vec::len);
    if let Some(intro) = &shape.intro {
        if !intro.hold_seconds.is_finite() || !intro.fade_seconds.is_finite() {
            return Err("Invalid loop intro duration".to_owned());
        }
        let attack = attack.ok_or_else(|| format!("Missing loop intro: {}", intro.path))?;
        if attack.channels != source.channels {
            return Err("Loop intro channels do not match".to_owned());
        }
        channels = channels
            .iter()
            .enumerate()
            .map(|(c, looped)| {
                prepend_loop_intro(
                    &channel(attack, c),
                    looped,
                    intro.hold_seconds * rate,
                    intro.fade_seconds * rate,
                )
            })
            .collect();
    }
    let mut loop_start = channels.first().map_or(0, Vec::len) - loop_frames;
    // `pitch`: the whole buffer resampled, as a playback rate would play it.
    if shape.pitch != 0.0 && shape.pitch.is_finite() {
        let speed = 2f64.powf(shape.pitch / 1200.0);
        let resample = |c: &Vec<f32>| -> Vec<f32> {
            let out = (c.len() as f64 / speed).floor() as usize;
            (0..out)
                .map(|i| {
                    let at = i as f64 * speed;
                    let k = at.floor() as usize;
                    let t = (at - k as f64) as f32;
                    let a = c[k.min(c.len() - 1)];
                    let b = c[(k + 1).min(c.len() - 1)];
                    a + (b - a) * t
                })
                .collect()
        };
        channels = channels.iter().map(resample).collect();
        loop_start = (loop_start as f64 / speed).round() as usize;
    }
    let length = channels.first().map_or(0, Vec::len);
    let mut samples = Vec::with_capacity(length * channels.len());
    for i in 0..length {
        for c in &channels {
            samples.push(c[i]);
        }
    }
    Ok((
        SoundBuffer {
            channels: source.channels,
            sample_rate: source.sample_rate,
            samples,
        },
        loop_start.min(length.saturating_sub(1)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_fades_into_the_head() {
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = overlap_loop_channel(&samples, 3.0);
        assert_eq!(out.len(), 7);
        // Equal power: all tail at the start, all head at the end.
        assert!((out[0] - 7.0).abs() < 1e-6);
        assert!((out[2] - 2.0).abs() < 1e-5);
        assert_eq!(out[3..], samples[3..7]);
        assert_eq!(overlap_loop_channel(&samples, 1.0), samples);
    }

    #[test]
    fn an_attack_plays_once_in_front() {
        let attack = vec![9.0; 6];
        let looped = vec![1.0, 2.0, 3.0, 4.0];
        let out = prepend_loop_intro(&attack, &looped, 2.0, 2.0);
        assert_eq!(out.len(), 2 + 2 + 4);
        assert_eq!(out[..2], [9.0, 9.0]);
        assert!((out[2] - 9.0).abs() < 1e-6);
        assert!((out[3] - 4.0).abs() < 1e-5);
        assert_eq!(out[4..], looped[..]);
    }

    #[test]
    fn a_region_with_an_intro_repeats_after_it() {
        let source = SoundBuffer {
            channels: 1,
            sample_rate: 100,
            samples: (0..100).map(|i| i as f32).collect(),
        };
        let shape = LoopShape {
            loop_region: Some(LoopRegion {
                start_seconds: 0.1,
                end_seconds: 0.25,
            }),
            overlap_seconds: 0.03,
            intro: Some(LoopIntro {
                path: "a".into(),
                hold_seconds: 0.12,
                fade_seconds: 0.06,
            }),
            pitch: 0.0,
        };
        let (buffer, start) = shape_loop(&source, Some(&source), &shape).unwrap();
        // 15 frames of region less 3 of overlap, after 12 + 6 of intro.
        assert_eq!(buffer.frames(), 18 + 12);
        assert_eq!(start, 18);
        let pitched = LoopShape { pitch: 1200.0, ..shape };
        let (buffer, start) = shape_loop(&source, Some(&source), &pitched).unwrap();
        assert_eq!((buffer.frames(), start), (15, 9));
    }
}
