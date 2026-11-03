//! SHAG `src/engine/viewport/effects/StrafeLeanEffect.ts`: the view rolls
//! into a strafe.

use glam::DVec3;

use crate::viewport::ViewCvars;
use crate::viewport::player_state::PlayerState;
use crate::viewport::viewport_delta::ViewportDelta;

/// `StrafeLeanEffect`.
#[derive(Debug, Clone, Default)]
pub struct StrafeLeanEffect {
    current_lean: f64,
}

impl StrafeLeanEffect {
    pub fn update(&mut self, cvars: &ViewCvars, delta_time: f64, state: &PlayerState) -> ViewportDelta {
        let mut delta = ViewportDelta::default();
        let max_tilt = cvars.strafe_tilt;
        if max_tilt == 0.0 {
            self.current_lean = 0.0;
            return delta;
        }
        let target_lean = -state.strafe_input * max_tilt;
        let lerp_factor = (delta_time * cvars.strafe_tilt_speed).min(1.0);
        self.current_lean += (target_lean - self.current_lean) * lerp_factor;
        delta.rotation_offset = DVec3::new(0.0, 0.0, self.current_lean);
        delta
    }

    pub fn reset(&mut self) {
        self.current_lean = 0.0;
    }
}
