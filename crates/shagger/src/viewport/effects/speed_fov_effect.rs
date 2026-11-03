//! SHAG `src/engine/viewport/effects/SpeedFOVEffect.ts`: the field of view
//! widens with speed, squeezes while a wall jump charges and bursts as it
//! goes.

use crate::viewport::ViewCvars;
use crate::viewport::player_state::{PlayerState, WallJumpState};
use crate::viewport::viewport_delta::ViewportDelta;

/// `SpeedFOVEffect`.
#[derive(Debug, Clone)]
pub struct SpeedFovEffect {
    current_fov: f64,
    was_wall_jumping: bool,
    wall_jump_burst_time: f64,
}

impl Default for SpeedFovEffect {
    fn default() -> Self {
        Self {
            current_fov: 1.0,
            was_wall_jumping: false,
            wall_jump_burst_time: 0.0,
        }
    }
}

impl SpeedFovEffect {
    pub fn update(&mut self, cvars: &ViewCvars, delta_time: f64, state: &PlayerState) -> ViewportDelta {
        let mut delta = ViewportDelta::default();
        let max_fov_increase = cvars.speed_fov;
        if max_fov_increase == 0.0 {
            self.current_fov = 1.0;
            return delta;
        }
        if state.wall_jump_state == WallJumpState::Charging {
            let target = 0.9;
            self.current_fov += (target - self.current_fov) * (delta_time * 8.0).min(1.0);
            self.was_wall_jumping = true;
            delta.fov_multiplier = self.current_fov;
            return delta;
        } else if self.was_wall_jumping && state.wall_jump_state == WallJumpState::None {
            self.wall_jump_burst_time = 0.2;
            self.was_wall_jumping = false;
        }
        if self.wall_jump_burst_time > 0.0 {
            self.wall_jump_burst_time -= delta_time;
            let burst_ratio = (self.wall_jump_burst_time / 0.2).max(0.0);
            let burst_fov = 1.0 + 0.2 * burst_ratio;
            self.current_fov += (burst_fov - self.current_fov) * (delta_time * 12.0).min(1.0);
            delta.fov_multiplier = self.current_fov;
            return delta;
        }
        const BASE: f64 = 280.0;
        const MAX: f64 = 800.0;
        let speed = state.horizontal_speed;
        let speed_ratio = if speed > BASE {
            ((speed - BASE) / (MAX - BASE)).min(1.0)
        } else {
            0.0
        };
        let target = 1.0 + speed_ratio * max_fov_increase;
        self.current_fov += (target - self.current_fov) * (delta_time * cvars.speed_fov_speed).min(1.0);
        delta.fov_multiplier = self.current_fov;
        delta
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
