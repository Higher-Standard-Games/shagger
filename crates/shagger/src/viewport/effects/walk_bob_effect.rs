//! SHAG `src/engine/viewport/effects/WalkBobEffect.ts`: the view sways
//! side to side and dips with each step, rolls a little, and leans
//! forward when running fast.

use glam::DVec3;

use crate::viewport::ViewCvars;
use crate::viewport::player_state::PlayerState;
use crate::viewport::speed_constants::{BASE_SPEED, get_speed_ratio};
use crate::viewport::viewport_delta::ViewportDelta;

/// One frame of the bob (`computeBob`'s result).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bob {
    pub new_bob_cycle: f64,
    pub bob_right: f64,
    pub bob_up: f64,
    pub roll_degrees: f64,
    pub target_lean: f64,
}

/// `WalkBobEffect.computeBob`.
#[must_use]
pub fn compute_bob(cvars: &ViewCvars, bob_cycle: f64, horizontal_speed: f64, delta_seconds: f64) -> Bob {
    let speed_factor = (horizontal_speed / BASE_SPEED).min(2.0);
    let speed_ratio = get_speed_ratio(horizontal_speed);
    let effective_frequency = cvars.bob_frequency * (1.0 + speed_ratio * 0.3);
    let new_bob_cycle = bob_cycle + horizontal_speed * delta_seconds * effective_frequency;
    Bob {
        new_bob_cycle,
        bob_up: (new_bob_cycle * 2.0).sin() * cvars.bob_amplitude * speed_factor,
        bob_right: new_bob_cycle.sin() * cvars.bob_amplitude * 0.4 * speed_factor * (1.0 + speed_ratio * 0.5),
        roll_degrees: new_bob_cycle.sin() * cvars.bob_roll * speed_factor,
        target_lean: speed_ratio * cvars.bob_lean,
    }
}

/// `WalkBobEffect`.
#[derive(Debug, Clone, Default)]
pub struct WalkBobEffect {
    bob_cycle: f64,
    current_lean: f64,
    /// `state.bobCycle`: the cycle the weapon bob follows, written only on
    /// frames that bob, so it holds while airborne or standing.
    pub shared_cycle: f64,
}

impl WalkBobEffect {
    pub fn update(&mut self, cvars: &ViewCvars, delta_time: f64, state: &PlayerState) -> ViewportDelta {
        let mut delta = ViewportDelta::default();
        if !cvars.bob {
            return delta;
        }
        // Sliding keeps the view still.
        if state.is_sliding {
            self.bob_cycle *= 0.85;
            self.current_lean *= 0.85;
            return delta;
        }
        if !state.is_grounded || state.horizontal_speed < 10.0 {
            self.bob_cycle *= 0.9;
            self.current_lean *= 0.9;
            return delta;
        }
        let bob = compute_bob(cvars, self.bob_cycle, state.horizontal_speed, delta_time);
        self.bob_cycle = bob.new_bob_cycle;
        let lean_speed = 5.0;
        self.current_lean += (bob.target_lean - self.current_lean) * (delta_time * lean_speed).min(1.0);
        self.shared_cycle = self.bob_cycle;
        delta.position_offset = DVec3::new(bob.bob_right, bob.bob_up, 0.0);
        delta.rotation_offset = DVec3::new(self.current_lean, 0.0, bob.roll_degrees);
        delta
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
