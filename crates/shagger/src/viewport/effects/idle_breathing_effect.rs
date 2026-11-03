//! SHAG `src/engine/viewport/effects/IdleBreathingEffect.ts`: standing
//! still, the view breathes.

use glam::DVec3;

use crate::viewport::ViewCvars;
use crate::viewport::player_state::PlayerState;
use crate::viewport::viewport_delta::ViewportDelta;

/// Below this speed the player counts as standing still.
const IDLE_THRESHOLD: f64 = 50.0;

/// `IdleBreathingEffect`.
#[derive(Debug, Clone, Default)]
pub struct IdleBreathingEffect {
    breath_cycle: f64,
}

impl IdleBreathingEffect {
    pub fn update(&mut self, cvars: &ViewCvars, delta_time: f64, state: &PlayerState) -> ViewportDelta {
        let mut delta = ViewportDelta::default();
        if !cvars.idle {
            return delta;
        }
        let amplitude = cvars.idle_amplitude;
        let idle_factor = 1.0 - (state.horizontal_speed / IDLE_THRESHOLD).min(1.0);
        self.breath_cycle += delta_time * cvars.idle_frequency;
        if idle_factor < 0.1 {
            return delta;
        }
        let offset_y = self.breath_cycle.sin() * amplitude * idle_factor;
        let offset_x = (self.breath_cycle * 0.7).sin() * amplitude * 0.03 * idle_factor;
        delta.position_offset = DVec3::new(offset_x, offset_y, 0.0);
        delta
    }

    pub fn reset(&mut self) {
        self.breath_cycle = 0.0;
    }
}
