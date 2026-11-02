//! Web Audio's `PannerNode` as SHAG's spatial sounds use it (RUST.md 22.2):
//! the inverse distance model and equal-power panning, written from the W3C
//! Web Audio specification (sections "Spatialization/Panning" and
//! "Distance Effects"). No Babylon code: Babylon only passes the settings on.

use glam::DVec3;

/// `REFERENCE_DISTANCE` (`refDistance`): full volume inside it.
pub const REFERENCE_DISTANCE: f64 = 125.0;

/// The `inverse` distance model:
/// `ref / (ref + rolloff × (max(d, ref) − ref))`.
#[must_use]
pub fn inverse_distance_gain(distance: f64, reference: f64, rolloff: f64) -> f64 {
    let d = distance.max(reference);
    reference / (reference + rolloff * (d - reference))
}

/// Where the listener stands and looks, as Web Audio's `AudioListener`
/// holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listener {
    pub position: DVec3,
    /// Web Audio's forward. Babylon hands its listener the camera's
    /// right-handed forward (`RightHandedForwardReadOnly` turned by the
    /// camera), the opposite of where a left-handed camera looks; see
    /// [`Listener::from_camera`].
    pub forward: DVec3,
    pub up: DVec3,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            position: DVec3::ZERO,
            forward: DVec3::NEG_Z,
            up: DVec3::Y,
        }
    }
}

impl Listener {
    /// The listener Babylon attaches to a camera at `position` looking
    /// along `look` (game space, left-handed): Web Audio's forward is
    /// `-look`, so left and right come out right and front and back are
    /// swapped, which equal-power panning cannot hear (RUST.md 22.2).
    #[must_use]
    pub fn from_camera(position: DVec3, look: DVec3, up: DVec3) -> Self {
        Self {
            position,
            forward: -look,
            up,
        }
    }

    /// The spec's azimuth of `source` in degrees, -180 to 180 (0 in front,
    /// 90 to the right).
    #[must_use]
    pub fn azimuth(&self, source: DVec3) -> f64 {
        let to_source = source - self.position;
        if to_source.length_squared() == 0.0 {
            return 0.0;
        }
        let to_source = to_source.normalize();
        let forward = self.forward.normalize_or_zero();
        let right = forward.cross(self.up).normalize_or_zero();
        let up = right.cross(forward);
        let projected = (to_source - up * to_source.dot(up)).normalize_or_zero();
        let mut azimuth = projected.dot(right).clamp(-1.0, 1.0).acos().to_degrees();
        if projected.dot(forward) < 0.0 {
            azimuth = 360.0 - azimuth;
        }
        if (0.0..=270.0).contains(&azimuth) {
            90.0 - azimuth
        } else {
            450.0 - azimuth
        }
    }
}

/// The equal-power gains for an azimuth, folded to the front: for a mono
/// source `(left, right)` scale its one channel; for a stereo source see
/// [`pan_stereo`].
#[must_use]
pub fn equal_power_gains(azimuth: f64, stereo: bool) -> (f64, f64) {
    let mut a = azimuth.clamp(-180.0, 180.0);
    if a < -90.0 {
        a = -180.0 - a;
    } else if a > 90.0 {
        a = 180.0 - a;
    }
    let x = if !stereo {
        (a + 90.0) / 180.0
    } else if a <= 0.0 {
        (a + 90.0) / 90.0
    } else {
        a / 90.0
    };
    let angle = x * std::f64::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// The spec's stereo panning: the side the sound is on keeps its channel
/// and takes a share of the other.
#[must_use]
pub fn pan_stereo(azimuth: f64, left: f32, right: f32) -> (f32, f32) {
    let (gl, gr) = equal_power_gains(azimuth, true);
    let (gl, gr) = (gl as f32, gr as f32);
    let folded = {
        let a = azimuth.clamp(-180.0, 180.0);
        if a < -90.0 {
            -180.0 - a
        } else if a > 90.0 {
            180.0 - a
        } else {
            a
        }
    };
    if folded <= 0.0 {
        (left + right * gl, right * gr)
    } else {
        (left * gl, right + left * gr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_volume_where_the_attenuation_classes_say() {
        // RUST.md 22.3: d = ref + ref / rolloff
        for (rolloff, half) in [(0.5, 375.0), (1.0, 250.0), (2.5, 175.0), (5.0, 150.0), (10.0, 137.5)] {
            assert!((inverse_distance_gain(half, REFERENCE_DISTANCE, rolloff) - 0.5).abs() < 1e-12);
        }
        assert_eq!(inverse_distance_gain(10.0, REFERENCE_DISTANCE, 1.0), 1.0);
        assert_eq!(inverse_distance_gain(5000.0, REFERENCE_DISTANCE, 0.0), 1.0);
    }

    #[test]
    fn right_is_right_for_a_left_handed_camera() {
        // A camera at the origin looking +z: game-space right is +x.
        let listener = Listener::from_camera(DVec3::ZERO, DVec3::Z, DVec3::Y);
        let right = listener.azimuth(DVec3::new(100.0, 0.0, 0.0));
        let (l, r) = equal_power_gains(right, false);
        assert!(r > 0.99 && l < 0.01, "{right} {l} {r}");
        let left = listener.azimuth(DVec3::new(-100.0, 0.0, 0.0));
        let (l, r) = equal_power_gains(left, false);
        assert!(l > 0.99 && r < 0.01);
        // In front and behind both land in the middle.
        for z in [100.0, -100.0] {
            let (l, r) = equal_power_gains(listener.azimuth(DVec3::new(0.0, 0.0, z)), false);
            assert!((l - r).abs() < 1e-9);
        }
    }

    #[test]
    fn stereo_keeps_the_near_channel() {
        assert_eq!(pan_stereo(0.0, 0.5, 0.25), (0.5, 0.25));
        let (l, r) = pan_stereo(90.0, 1.0, 0.0);
        assert!(l.abs() < 1e-6 && (r - 1.0).abs() < 1e-6);
    }
}
