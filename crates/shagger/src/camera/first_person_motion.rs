//! SHAG `src/engine/camera/FirstPersonMotion.ts`: how a first-person eye
//! moves with its body beyond the view effects: the dip into a slide, the
//! lift through a ledge grab, and the ease up a step.

/// The view's dip while sliding, in radians, from the slide's phase (0 to 1).
#[must_use]
pub fn slide_pitch(phase: f64) -> f64 {
    if phase > 0.01 { phase * -0.22 } else { 0.0 }
}

/// Eases the ledge grab lift toward its phase's target.
#[must_use]
pub fn ease_ledge_pitch(current: f64, phase: f64, dt: f64) -> f64 {
    let target = if phase > 0.01 { phase * 0.15 } else { 0.0 };
    current + (target - current) * (dt * 8.0).min(1.0)
}

/// The eye's height as it climbs: small steps up ease in by `factor` per
/// frame; a fall, a big jump in height, a teleport or `snap` is followed
/// exactly.
#[must_use]
pub fn smooth_step_up(smoothed_y: f64, y: f64, step_height: f64, factor: f64, snap: bool) -> f64 {
    let rise = y - smoothed_y;
    if snap || rise.abs() > step_height * 2.0 || rise < -0.5 {
        return y;
    }
    smoothed_y + rise * factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_matches_the_typescript() {
        assert_eq!(slide_pitch(0.0), 0.0);
        assert!((slide_pitch(1.0) + 0.22).abs() < 1e-12);
        assert!((ease_ledge_pitch(0.0, 1.0, 0.0625) - 0.075).abs() < 1e-12);
        assert_eq!(smooth_step_up(0.0, 10.0, 18.0, 0.3, false), 3.0);
        assert_eq!(smooth_step_up(0.0, 40.0, 18.0, 0.3, false), 40.0);
        assert_eq!(smooth_step_up(10.0, 0.0, 18.0, 0.3, false), 0.0);
    }
}
