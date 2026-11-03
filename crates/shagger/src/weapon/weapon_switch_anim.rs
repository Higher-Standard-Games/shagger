//! SHAG `src/engine/weapon/WeaponSwitchAnim.ts`: the weapon slides down,
//! swaps, and slides up. State machine: idle, down, load, up, idle.
//!
//! The caller passes its clock in milliseconds, and loads synchronously:
//! [`WeaponSwitchAnim::update`] returns the weapon to load when the slide
//! down completes, and the caller reports the load with
//! [`WeaponSwitchAnim::loaded`].

/// `SwitchAnimOffsets`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SwitchAnimOffsets {
    pub y_offset: f64,
    pub roll: f64,
    pub x_offset: f64,
    pub z_offset: f64,
}

const DOWN_DURATION: f64 = 120.0;
const UP_DURATION: f64 = 140.0;
const Y_DROP: f64 = -45.0;
const ROLL: f64 = std::f64::consts::PI / 6.0;
/// Longest wait for a load before sliding up anyway.
const LOAD_WAIT: f64 = 200.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Down,
    Load,
    Up,
}

/// `WeaponSwitchAnim`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeaponSwitchAnim {
    phase: Phase,
    start_ms: f64,
    pending: Option<String>,
}

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

impl WeaponSwitchAnim {
    /// `start`: false when already switching to `weapon`.
    pub fn start(&mut self, weapon: &str, now_ms: f64) -> bool {
        if self.phase != Phase::Idle && self.pending.as_deref() == Some(weapon) {
            return false;
        }
        self.phase = Phase::Down;
        self.start_ms = now_ms;
        self.pending = Some(weapon.to_owned());
        true
    }

    /// `update`: the weapon to load now, once, as the slide down ends.
    pub fn update(&mut self, now_ms: f64) -> Option<String> {
        let elapsed = now_ms - self.start_ms;
        match self.phase {
            Phase::Idle => None,
            Phase::Down => {
                if elapsed < DOWN_DURATION {
                    return None;
                }
                self.phase = Phase::Load;
                self.start_ms = now_ms;
                self.pending.clone()
            }
            Phase::Load => {
                if elapsed >= LOAD_WAIT {
                    self.phase = Phase::Up;
                    self.start_ms = now_ms;
                }
                None
            }
            Phase::Up => {
                if elapsed >= UP_DURATION {
                    self.phase = Phase::Idle;
                    self.pending = None;
                }
                None
            }
        }
    }

    /// The load of `weapon` finished: slide up, if it is still the one
    /// wanted.
    pub fn loaded(&mut self, weapon: &str, now_ms: f64) {
        if self.pending.as_deref() == Some(weapon) && self.phase == Phase::Load {
            self.phase = Phase::Up;
            self.start_ms = now_ms;
        }
    }

    /// `getOffsets` at `now_ms`.
    #[must_use]
    pub fn offsets(&self, now_ms: f64) -> SwitchAnimOffsets {
        let elapsed = now_ms - self.start_ms;
        let at = |amount: f64| SwitchAnimOffsets {
            y_offset: amount * Y_DROP,
            roll: amount * ROLL,
            ..SwitchAnimOffsets::default()
        };
        match self.phase {
            Phase::Idle => SwitchAnimOffsets::default(),
            Phase::Down => at(ease_out_cubic((elapsed / DOWN_DURATION).min(1.0))),
            Phase::Load => at(1.0),
            Phase::Up => at(1.0 - ease_out_cubic((elapsed / UP_DURATION).min(1.0))),
        }
    }

    /// `isSwitching`.
    #[must_use]
    pub fn is_switching(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// The weapon being switched to.
    #[must_use]
    pub fn pending(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_down_swaps_and_slides_up() {
        let mut a = WeaponSwitchAnim::default();
        assert!(a.start("riotgun", 0.0));
        assert!(!a.start("riotgun", 10.0));
        assert_eq!(a.update(60.0), None);
        let half = a.offsets(60.0);
        assert!((half.y_offset - Y_DROP * 0.875).abs() < 1e-9, "{half:?}");
        assert_eq!(a.update(120.0).as_deref(), Some("riotgun"));
        assert_eq!(a.offsets(150.0).y_offset, Y_DROP);
        a.loaded("riotgun", 130.0);
        assert!((a.offsets(270.0).y_offset).abs() < 1e-12);
        a.update(270.0);
        assert!(!a.is_switching());
    }

    #[test]
    fn a_slow_load_slides_up_anyway() {
        let mut a = WeaponSwitchAnim::default();
        a.start("blade", 0.0);
        a.update(120.0);
        a.update(320.0);
        a.update(460.0);
        assert!(!a.is_switching());
    }
}
