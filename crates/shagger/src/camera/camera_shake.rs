//! wash-fps `camera/FPSController.ts`, its camera shake (`shakeCamera` and
//! the decaying random offsets in `update`): a nearby explosion or a hit
//! jolts the view, and the jolt dies away over a fraction of a second.
//! Visual only: the offsets go on the rendered camera, never the aim.

/// The widest jolt at full intensity, radians (about 5.7°).
const MAX_ANGLE: f64 = 0.1;
/// How fast the intensity dies away, per second.
const DECAY_RATE: f64 = 6.0;
/// Below this the shake is over.
const CUTOFF: f64 = 0.001;

/// The shake's state. `pitch` and `yaw` are this frame's offsets.
#[derive(Debug, Clone, Default)]
pub struct CameraShake {
    pub intensity: f64,
    pub pitch: f64,
    pub yaw: f64,
}

impl CameraShake {
    /// `shakeCamera`: stacks with a shake under way, capped at 1. The caller
    /// checks `cg_screenshake` or `cg_hitshake`.
    pub fn shake(&mut self, intensity: f64) {
        self.intensity = (self.intensity + intensity).min(1.0);
    }

    /// One frame: new random offsets scaled by the intensity, then the
    /// decay. `random` gives numbers in [0, 1), as `Math.random`.
    pub fn update(&mut self, delta_time: f64, random: &mut impl FnMut() -> f64) {
        if self.intensity <= CUTOFF {
            return;
        }
        self.pitch = (random() * 2.0 - 1.0) * MAX_ANGLE * self.intensity;
        self.yaw = (random() * 2.0 - 1.0) * MAX_ANGLE * self.intensity;
        self.intensity *= (1.0 - DECAY_RATE * delta_time).max(0.0);
        if self.intensity < CUTOFF {
            *self = Self::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shakes_stack_to_one_and_die_away() {
        let mut s = CameraShake::default();
        s.shake(0.7);
        s.shake(0.7);
        assert_eq!(s.intensity, 1.0);
        let mut r = || 1.0;
        s.update(1.0 / 60.0, &mut r);
        assert!((s.pitch - 0.1).abs() < 1e-12, "{}", s.pitch);
        assert!((s.intensity - 0.9).abs() < 1e-12);
        for _ in 0..120 {
            s.update(1.0 / 60.0, &mut r);
        }
        assert_eq!((s.intensity, s.pitch, s.yaw), (0.0, 0.0, 0.0));
    }

    #[test]
    fn still_without_a_shake() {
        let mut s = CameraShake::default();
        s.update(0.016, &mut || panic!("no random numbers for a still view"));
        assert_eq!((s.pitch, s.yaw), (0.0, 0.0));
    }
}
