//! SHAG `src/engine/viewport/PlayerState.ts`.

use glam::DVec3;

/// `wallJumpState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WallJumpState {
    #[default]
    None,
    Available,
    Charging,
    Pushing,
}

/// What the effects read about the player this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerState {
    pub position: DVec3,
    pub velocity: DVec3,
    pub horizontal_speed: f64,
    pub is_grounded: bool,
    pub was_grounded_last_frame: bool,
    pub is_jumping: bool,
    /// -1 left, 1 right.
    pub strafe_input: f64,
    /// -1 back, 1 forward.
    pub forward_input: f64,
    /// The fall speed caught on landing.
    pub landing_velocity: Option<f64>,
    pub wall_jump_state: WallJumpState,
    pub is_sprinting: bool,
    pub is_aiming: bool,
    pub is_firing: bool,
    pub is_sliding: bool,
}

impl Default for PlayerState {
    /// `createNeutralPlayerState`.
    fn default() -> Self {
        Self {
            position: DVec3::ZERO,
            velocity: DVec3::ZERO,
            horizontal_speed: 0.0,
            is_grounded: true,
            was_grounded_last_frame: true,
            is_jumping: false,
            strafe_input: 0.0,
            forward_input: 0.0,
            landing_velocity: Some(0.0),
            wall_jump_state: WallJumpState::None,
            is_sprinting: false,
            is_aiming: false,
            is_firing: false,
            is_sliding: false,
        }
    }
}
