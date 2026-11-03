//! SHAG `src/engine/viewport/FirstPersonEffects.ts`: the first-person
//! effects in their order, and the aim-down-sights zoom.

use super::ViewCvars;
use super::effects::idle_breathing_effect::IdleBreathingEffect;
use super::effects::landing_impact_effect::LandingImpactEffect;
use super::effects::speed_fov_effect::SpeedFovEffect;
use super::effects::strafe_lean_effect::StrafeLeanEffect;
use super::effects::walk_bob_effect::WalkBobEffect;
use super::player_state::PlayerState;
use super::viewport_delta::ViewportDelta;

/// `AdsZoom`: the field of view narrows while aiming.
#[derive(Debug, Clone, Default)]
pub struct AdsZoom {
    pub progress: f64,
}

impl AdsZoom {
    /// The field-of-view multiplier this frame.
    pub fn step(&mut self, cvars: &ViewCvars, delta_time: f64, aiming: bool) -> f64 {
        let speed = 1.0 / cvars.ads_time.max(0.05);
        self.progress += (f64::from(u8::from(aiming)) - self.progress) * (delta_time * speed).min(1.0);
        if self.progress <= 0.001 {
            return 1.0;
        }
        (1.0 - (cvars.ads_fov_reduction / 100.0) * self.progress).max(0.4)
    }

    pub fn reset(&mut self) {
        self.progress = 0.0;
    }
}

/// `FirstPersonEffectStack`: `createFirstPersonEffects`' five effects and
/// the zoom.
#[derive(Debug, Clone, Default)]
pub struct FirstPersonEffectStack {
    pub walk_bob: WalkBobEffect,
    pub landing: LandingImpactEffect,
    pub strafe_lean: StrafeLeanEffect,
    pub speed_fov: SpeedFovEffect,
    pub idle: IdleBreathingEffect,
    ads: AdsZoom,
    last: ViewportDelta,
}

impl FirstPersonEffectStack {
    /// The effects' sum this frame; a frame of no time repeats the last.
    pub fn update(&mut self, cvars: &ViewCvars, delta_time: f64, state: &PlayerState) -> ViewportDelta {
        if delta_time <= 0.0 {
            return self.last;
        }
        let mut combined = ViewportDelta::default();
        if cvars.view_effects {
            combined.combine(&self.walk_bob.update(cvars, delta_time, state));
            combined.combine(&self.landing.update(cvars, delta_time, state));
            combined.combine(&self.strafe_lean.update(cvars, delta_time, state));
            combined.combine(&self.speed_fov.update(cvars, delta_time, state));
            combined.combine(&self.idle.update(cvars, delta_time, state));
        }
        combined.fov_multiplier *= self.ads.step(cvars, delta_time, state.is_aiming);
        self.last = combined;
        combined
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(speed: f64) -> PlayerState {
        PlayerState {
            horizontal_speed: speed,
            ..PlayerState::default()
        }
    }

    #[test]
    fn walking_bobs_and_standing_breathes() {
        let cvars = ViewCvars::default();
        let mut stack = FirstPersonEffectStack::default();
        let mut lowest: f64 = 0.0;
        let mut highest: f64 = 0.0;
        for _ in 0..120 {
            let d = stack.update(&cvars, 1.0 / 60.0, &running(320.0));
            lowest = lowest.min(d.position_offset.y);
            highest = highest.max(d.position_offset.y);
        }
        // About the amplitude times the speed factor (320 / 280).
        assert!(highest > 1.0 && lowest < -1.0, "{lowest} {highest}");
        let mut still = FirstPersonEffectStack::default();
        let d = still.update(&cvars, 1.0, &running(0.0));
        assert!(d.position_offset.y.abs() <= 0.02);
    }

    #[test]
    fn a_hard_landing_dips_then_recovers() {
        let cvars = ViewCvars::default();
        let mut stack = FirstPersonEffectStack::default();
        let landed = PlayerState {
            was_grounded_last_frame: false,
            landing_velocity: Some(-800.0),
            ..PlayerState::default()
        };
        let d = stack.update(&cvars, 0.05, &landed);
        assert!(d.position_offset.y < -4.0, "{}", d.position_offset.y);
        let mut last = d;
        for _ in 0..30 {
            last = stack.update(&cvars, 0.05, &PlayerState::default());
        }
        assert!(last.position_offset.y.abs() < 0.03, "{}", last.position_offset.y);
    }

    #[test]
    fn strafing_rolls_against_the_strafe_and_speed_widens_the_view() {
        let cvars = ViewCvars::default();
        let mut stack = FirstPersonEffectStack::default();
        let state = PlayerState {
            strafe_input: 1.0,
            is_grounded: false,
            horizontal_speed: 800.0,
            ..PlayerState::default()
        };
        let mut d = ViewportDelta::default();
        for _ in 0..60 {
            d = stack.update(&cvars, 1.0 / 30.0, &state);
        }
        assert!((d.rotation_offset.z + 1.5).abs() < 0.01, "{}", d.rotation_offset.z);
        assert!((d.fov_multiplier - 1.38).abs() < 0.01, "{}", d.fov_multiplier);
    }

    #[test]
    fn aiming_narrows_the_view() {
        let cvars = ViewCvars::default();
        let mut zoom = AdsZoom::default();
        let mut m = 1.0;
        for _ in 0..30 {
            m = zoom.step(&cvars, 1.0 / 60.0, true);
        }
        assert!((m - 0.85).abs() < 0.001, "{m}");
        let off = ViewCvars {
            view_effects: false,
            ..cvars
        };
        let mut stack = FirstPersonEffectStack::default();
        assert_eq!(stack.update(&off, 0.1, &running(400.0)).position_offset.y, 0.0);
    }
}
