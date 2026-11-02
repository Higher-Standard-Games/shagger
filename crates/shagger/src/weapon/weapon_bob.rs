//! SHAG `src/engine/weapon/WeaponBob.ts`: walk bob, sprint tilt and the
//! jump and land bob of a first-person weapon. The cvars it reads
//! (`cl_gunbob*`) come in as [`GunBobVars`], at their defaults unless set.

/// Smooth bob transitions.
const BOB_LERP_SPEED: f64 = 8.0;
/// How fast the jump/land bob reaches its target.
const JUMP_LAND_LERP: f64 = 12.0;
/// How fast it recovers to 0.
const JUMP_LAND_RECOVER: f64 = 4.0;
/// Minimum seconds between triggers.
const JUMP_LAND_COOLDOWN: f64 = 0.15;

// Sprint speed thresholds (Quake units/s).
const BASE_SPEED: f64 = 320.0;
/// 100% sprint tilt point.
const SPRINT_SPEED: f64 = 420.0;
/// 120% cap.
const MAX_EXTRA_SPEED: f64 = 800.0;

/// The `cl_gunbob*` cvars `WeaponBob` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GunBobVars {
    pub enabled: bool,
    pub amplitude: f64,
    pub speed: f64,
    pub sprint_tilt: f64,
    pub sprint_inward: f64,
    pub jump: f64,
    pub land: f64,
}

impl Default for GunBobVars {
    /// `MovementVars`' defaults.
    fn default() -> Self {
        Self {
            enabled: true,
            amplitude: 1.5,
            speed: 1.0,
            sprint_tilt: 10.0,
            sprint_inward: 30.0,
            jump: 2.0,
            land: 3.0,
        }
    }
}

/// `BobState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BobState {
    /// Horizontal walk bob (left/right sway).
    pub bob_x: f64,
    /// Vertical walk bob (up/down sway).
    pub bob_y: f64,
    /// Sprint pitch tilt (degrees, negative = tilted up).
    pub sprint_tilt: f64,
    /// Sprint roll (degrees, positive = rolled right).
    pub sprint_roll: f64,
    /// Sprint yaw, barrel toward chest (degrees).
    pub sprint_yaw: f64,
    /// Jump/land vertical bob offset.
    pub jump_land_bob: f64,
}

/// What `update` takes about the player this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BobInput {
    pub delta_time: f64,
    pub grounded: bool,
    /// Horizontal speed (Quake units/s).
    pub horizontal_speed: f64,
    /// Vertical velocity (positive = upward).
    pub velocity_y: f64,
    /// Vertical speed at the last landing.
    pub landing_velocity: f64,
    /// The shared view bob cycle (radians).
    pub bob_cycle: f64,
    pub reloading: bool,
    /// 0 = hip, 1 = fully aimed.
    pub ads_progress: f64,
    /// The sign-carrying base X offset (positive = right hand).
    pub hand_offset_x: f64,
}

/// `WeaponBob`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponBob {
    current_bob_x: f64,
    current_bob_y: f64,
    current_sprint_tilt: f64,
    current_sprint_roll: f64,
    current_sprint_yaw: f64,
    jump_land_target: f64,
    jump_land_current: f64,
    jump_land_cooldown: f64,
    was_grounded: bool,
}

impl Default for WeaponBob {
    fn default() -> Self {
        Self {
            current_bob_x: 0.0,
            current_bob_y: 0.0,
            current_sprint_tilt: 0.0,
            current_sprint_roll: 0.0,
            current_sprint_yaw: 0.0,
            jump_land_target: 0.0,
            jump_land_current: 0.0,
            jump_land_cooldown: 0.0,
            was_grounded: true,
        }
    }
}

impl WeaponBob {
    /// `update`.
    pub fn update(&mut self, input: &BobInput, vars: &GunBobVars) {
        let h_speed = input.horizontal_speed;
        let cycle = input.bob_cycle * vars.speed;
        let speed_factor = (h_speed / 320.0).min(2.0);

        // Walk bob (grounded only).
        let mut target_bob_x = 0.0;
        let mut target_bob_y = 0.0;
        let mut target_tilt = 0.0;
        let mut target_roll = 0.0;
        let mut target_inward = 0.0;
        if vars.enabled && input.grounded && h_speed > 10.0 {
            target_bob_y = (cycle * 2.0).sin() * vars.amplitude * speed_factor;
            target_bob_x = cycle.sin() * vars.amplitude * 0.5 * speed_factor;
        }

        // Sprint tilt (speed-based, not grounded): 320-420 gives 0..100%,
        // 420 and up 100..120% (capped).
        let sprint_ratio = if h_speed <= SPRINT_SPEED {
            ((h_speed - BASE_SPEED) / (SPRINT_SPEED - BASE_SPEED)).max(0.0)
        } else {
            let extra = (h_speed - SPRINT_SPEED) / (MAX_EXTRA_SPEED - SPRINT_SPEED);
            1.0 + (extra * 0.2).min(0.2)
        };
        if vars.enabled && h_speed > BASE_SPEED * 0.8 {
            target_tilt = -vars.sprint_tilt * sprint_ratio;
            target_roll = vars.sprint_tilt * 1.5 * sprint_ratio;
            let hand_sign = if input.hand_offset_x > 0.0 { -1.0 } else { 1.0 };
            target_inward = hand_sign * vars.sprint_inward * sprint_ratio;
        }

        // Dampen during reload.
        if input.reloading {
            target_bob_x = 0.0;
            target_bob_y = 0.0;
            target_tilt = 0.0;
            target_roll = 0.0;
            target_inward = 0.0;
        }

        // Dampen when aimed (stable aim).
        let ads_damp = 1.0 - input.ads_progress * 0.85;
        target_bob_x *= ads_damp;
        target_bob_y *= ads_damp;
        target_tilt *= ads_damp;
        target_roll *= ads_damp;
        target_inward *= ads_damp;

        // Smooth lerp to targets.
        let lf = (input.delta_time * BOB_LERP_SPEED).min(1.0);
        self.current_bob_x += (target_bob_x - self.current_bob_x) * lf;
        self.current_bob_y += (target_bob_y - self.current_bob_y) * lf;
        self.current_sprint_tilt += (target_tilt - self.current_sprint_tilt) * lf;
        self.current_sprint_roll += (target_roll - self.current_sprint_roll) * lf;
        self.current_sprint_yaw += (target_inward - self.current_sprint_yaw) * lf;

        // Jump / land bob.
        if self.jump_land_cooldown > 0.0 {
            self.jump_land_cooldown -= input.delta_time;
        }
        // Leaving the ground.
        if self.jump_land_cooldown <= 0.0 && self.was_grounded && !input.grounded && input.velocity_y > 50.0 {
            self.jump_land_target = -vars.jump;
            self.jump_land_cooldown = JUMP_LAND_COOLDOWN;
        }
        // Hitting the ground.
        if self.jump_land_cooldown <= 0.0 && !self.was_grounded && input.grounded {
            let impact = (input.landing_velocity.abs() / 500.0).min(1.0);
            self.jump_land_target = -vars.land * (0.5 + impact * 0.5);
            self.jump_land_cooldown = JUMP_LAND_COOLDOWN;
        }
        // Smoothly reach the target, then recover.
        let jlf = (input.delta_time * JUMP_LAND_LERP).min(1.0);
        self.jump_land_current += (self.jump_land_target - self.jump_land_current) * jlf;
        let rf = (input.delta_time * JUMP_LAND_RECOVER).min(1.0);
        self.jump_land_target *= 1.0 - rf;

        self.was_grounded = input.grounded;
    }

    /// The current smoothed bob (read after `update`).
    #[must_use]
    pub fn state(&self) -> BobState {
        BobState {
            bob_x: self.current_bob_x,
            bob_y: self.current_bob_y,
            sprint_tilt: self.current_sprint_tilt,
            sprint_roll: self.current_sprint_roll,
            sprint_yaw: self.current_sprint_yaw,
            jump_land_bob: self.jump_land_current,
        }
    }

    /// `reset` (on weapon switch).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(grounded: bool, speed: f64, velocity_y: f64) -> BobInput {
        BobInput {
            delta_time: 1.0 / 60.0,
            grounded,
            horizontal_speed: speed,
            velocity_y,
            landing_velocity: 0.0,
            bob_cycle: 1.0,
            reloading: false,
            ads_progress: 0.0,
            hand_offset_x: 12.0,
        }
    }

    #[test]
    fn walking_sways_and_sprinting_tilts() {
        let mut bob = WeaponBob::default();
        for _ in 0..600 {
            bob.update(&input(true, 420.0, 0.0), &GunBobVars::default());
        }
        let s = bob.state();
        assert!((s.bob_y - (2.0f64).sin() * 1.5 * (420.0 / 320.0)).abs() < 1e-9);
        assert!((s.sprint_tilt + 10.0).abs() < 1e-9);
        assert!((s.sprint_roll - 15.0).abs() < 1e-9);
        // Right hand: the barrel turns in toward the chest.
        assert!((s.sprint_yaw + 30.0).abs() < 1e-9);
    }

    #[test]
    fn a_jump_dips_and_recovers() {
        let mut bob = WeaponBob::default();
        bob.update(&input(false, 0.0, 200.0), &GunBobVars::default());
        assert!(bob.state().jump_land_bob < 0.0);
        for _ in 0..600 {
            bob.update(&input(false, 0.0, 0.0), &GunBobVars::default());
        }
        assert!(bob.state().jump_land_bob.abs() < 1e-6);
    }
}
