//! The movement contract (SHAG `src/engine/network/systems/movement/MovementProfile.ts`):
//! what a game's movement profile reads each step and returns.

use shagger_shared::collision::collision_world::CollisionWorld;
use shagger_shared::server::simulation::types::Vec3;

/// View angles in radians.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewAngles {
    pub pitch: f64,
    pub yaw: f64,
}

/// `MovementInput`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MovementInput {
    /// Desired movement direction, world-relative.
    pub direction: Vec3,
    pub view_angles: ViewAngles,
    pub buttons: u32,
    /// The command's sequence number: its identity, and the legacy clock
    /// source when `sim_step` is absent.
    pub sequence: Option<f64>,
    /// Index of the fixed-dt step this call represents; profiles derive their
    /// effect clock from it (`simTime = simStep * dt`).
    pub sim_step: Option<f64>,
}

impl MovementInput {
    /// `isPressed`.
    #[must_use]
    pub fn is_pressed(&self, button: u32) -> bool {
        self.buttons & button != 0
    }
}

/// `MovementResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementResult {
    pub position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    pub rotation: Option<Vec3>,
}

/// `getDefaults`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementDefaults {
    pub max_speed: f64,
    pub acceleration: f64,
    pub friction: f64,
    pub gravity: f64,
    pub jump_velocity: f64,
}

/// `MovementProfile`: how entities move. A game implements it with its own
/// result type.
pub trait MovementProfile {
    type Result;

    /// One step of movement.
    fn simulate(
        &mut self,
        position: Vec3,
        velocity: Vec3,
        input: Option<&MovementInput>,
        delta_time: f64,
        collision: Option<&mut dyn CollisionWorld>,
    ) -> Self::Result;

    fn defaults(&self) -> MovementDefaults;

    fn name(&self) -> &'static str;
}
