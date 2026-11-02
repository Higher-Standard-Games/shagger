//! SHAG `src/engine/weapon/WeaponRecoil.ts`: the view model's kick and
//! return. The kick follows the weapon's fire rate: it snaps in within a
//! few frames and is back at rest before the next shot.

/// Max backward distance per 1.0 recoil scale.
const RECOIL_KICK: f64 = 1.5;
/// Max upward pitch (radians, about 7 degrees).
const RECOIL_PITCH_KICK: f64 = 0.12;
/// Seconds to reach the kick (shorter for fast-firing weapons).
const RECOIL_ATTACK: f64 = 0.03;
/// Backward recovery speed for slow-firing weapons (units/s).
const RECOIL_RECOVER: f64 = 8.0;
/// Pitch recovery speed for slow-firing weapons (radians/s).
const RECOIL_PITCH_RECOVER: f64 = 0.6;
/// Aiming reduces recoil to 20% unless the weapon says otherwise.
pub const RECOIL_ADS_MULT: f64 = 0.2;
/// Share of the refire interval the kick and return may take.
const RECOIL_REFIRE_SHARE: f64 = 0.85;

/// `WeaponRecoil`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponRecoil {
    /// Backward kick.
    pub offset: f64,
    /// Upward pitch kick.
    pub pitch_offset: f64,
    attack: f64,
    attack_elapsed: f64,
    start_offset: f64,
    start_pitch: f64,
    target: f64,
    pitch_target: f64,
    recover_speed: f64,
    pitch_recover_speed: f64,
}

impl Default for WeaponRecoil {
    fn default() -> Self {
        Self {
            offset: 0.0,
            pitch_offset: 0.0,
            attack: 0.0,
            attack_elapsed: 0.0,
            start_offset: 0.0,
            start_pitch: 0.0,
            target: 0.0,
            pitch_target: 0.0,
            recover_speed: RECOIL_RECOVER,
            pitch_recover_speed: RECOIL_PITCH_RECOVER,
        }
    }
}

impl WeaponRecoil {
    /// `fire`: aimed, the backward punch follows `ads_recoil_scale` but the
    /// barrel's rise keeps to 20%. `refire_seconds` is the time to the next
    /// shot; the kick is done and back at rest within it.
    pub fn fire(&mut self, recoil_scale: f64, ads_progress: f64, ads_recoil_scale: f64, refire_seconds: Option<f64>) {
        let back_mult = 1.0 - ads_progress * (1.0 - ads_recoil_scale);
        let pitch_mult = 1.0 - ads_progress * (1.0 - RECOIL_ADS_MULT);
        self.target = RECOIL_KICK * recoil_scale * back_mult;
        self.pitch_target = RECOIL_PITCH_KICK * recoil_scale * pitch_mult;

        let window = match refire_seconds {
            Some(s) if s > 0.0 => s * RECOIL_REFIRE_SHARE,
            _ => f64::INFINITY,
        };
        self.attack = RECOIL_ATTACK.min(window * 0.3);
        self.attack_elapsed = 0.0;
        self.start_offset = self.offset;
        self.start_pitch = self.pitch_offset;
        if self.attack <= 0.0 {
            self.offset = self.target;
            self.pitch_offset = self.pitch_target;
        }

        // Return at the slow default, or faster when the next shot comes sooner.
        let return_time = window - self.attack;
        self.recover_speed = RECOIL_RECOVER.max(self.target / return_time.max(0.01));
        self.pitch_recover_speed = RECOIL_PITCH_RECOVER.max(self.pitch_target / return_time.max(0.01));
    }

    /// `update`: the kick, then the return.
    pub fn update(&mut self, delta_time: f64) {
        if self.attack_elapsed < self.attack {
            self.attack_elapsed = self.attack.min(self.attack_elapsed + delta_time);
            let t = self.attack_elapsed / self.attack;
            self.offset = self.start_offset + (self.target - self.start_offset) * t;
            self.pitch_offset = self.start_pitch + (self.pitch_target - self.start_pitch) * t;
            return;
        }
        if self.offset > 0.0 {
            self.offset = (self.offset - self.recover_speed * delta_time).max(0.0);
        }
        if self.pitch_offset > 0.0 {
            self.pitch_offset = (self.pitch_offset - self.pitch_recover_speed * delta_time).max(0.0);
        }
    }

    /// `reset` (on weapon switch).
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.pitch_offset = 0.0;
        self.target = 0.0;
        self.pitch_target = 0.0;
        self.attack = 0.0;
        self.attack_elapsed = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kick_peaks_then_returns_before_the_next_shot() {
        let mut recoil = WeaponRecoil::default();
        recoil.fire(1.0, 0.0, RECOIL_ADS_MULT, Some(0.1));
        recoil.update(0.5);
        assert!((recoil.offset - 1.5).abs() < 1e-12);
        let mut t = 0.0;
        while recoil.offset > 0.0 {
            recoil.update(0.001);
            t += 0.001;
        }
        assert!(t < 0.1);
    }
}
