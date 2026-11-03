//! SHAG `src/engine/viewport/effects/LandingImpactEffect.ts`: the view
//! dips on landing, as hard as the fall was fast, and comes back up.

use glam::DVec3;

use crate::viewport::ViewCvars;
use crate::viewport::easing::{ease_in_out_quad, ease_out_quad};
use crate::viewport::player_state::PlayerState;
use crate::viewport::viewport_delta::ViewportDelta;

/// `LandingImpactEffect`.
#[derive(Debug, Clone)]
pub struct LandingImpactEffect {
    /// The deepest dip of this landing.
    impact_offset: f64,
    /// 0 just landed, 1 done.
    impact_phase: f64,
    pub last_landing_velocity: f64,
}

impl Default for LandingImpactEffect {
    fn default() -> Self {
        Self {
            impact_offset: 0.0,
            impact_phase: 1.0,
            last_landing_velocity: 0.0,
        }
    }
}

impl LandingImpactEffect {
    pub fn update(&mut self, cvars: &ViewCvars, delta_time: f64, state: &PlayerState) -> ViewportDelta {
        let mut delta = ViewportDelta::default();
        if !cvars.landing {
            return delta;
        }
        let just_landed = state.is_grounded && !state.was_grounded_last_frame;
        if just_landed {
            let landing_velocity = state.landing_velocity.unwrap_or(state.velocity.y);
            self.last_landing_velocity = landing_velocity;
            let impact_strength = (landing_velocity.abs() / 800.0).min(1.0);
            self.impact_offset = impact_strength * cvars.landing_amplitude;
            self.impact_phase = 0.0;
        }
        if self.impact_phase < 1.0 && self.impact_offset > 0.0 {
            self.impact_phase += delta_time * cvars.landing_speed;
            let offset_y = if self.impact_phase < 0.3 {
                // Down fast.
                -self.impact_offset * ease_out_quad(self.impact_phase / 0.3)
            } else if self.impact_phase < 1.0 {
                // Back up slower.
                -self.impact_offset * (1.0 - ease_in_out_quad((self.impact_phase - 0.3) / 0.7))
            } else {
                self.impact_phase = 1.0;
                0.0
            };
            delta.position_offset = DVec3::new(0.0, offset_y, 0.0);
        }
        delta
    }

    /// `trigger("landing", { velocity })`.
    pub fn landing(&mut self, cvars: &ViewCvars, velocity: f64) {
        let impact_strength = (velocity.abs() / 800.0).min(1.0);
        self.impact_offset = impact_strength * cvars.landing_amplitude;
        self.impact_phase = 0.0;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
