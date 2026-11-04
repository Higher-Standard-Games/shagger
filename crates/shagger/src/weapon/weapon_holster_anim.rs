//! SHAG `src/engine/weapon/WeaponHolsterAnim.ts`: the view model slides out
//! of view and back for movement states that take both hands (climbing a
//! ladder).
//!
//! It looks like [`super::weapon_switch_anim`] (drop and roll, eased) but is
//! one progress value driven toward a target rather than a timeline: the
//! weapon stays down for as long as the climb lasts, and letting go halfway
//! down raises it from where it is.

/// `HolsterAnimOffsets`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HolsterAnimOffsets {
    pub y_offset: f64,
    pub roll: f64,
    pub x_offset: f64,
    pub z_offset: f64,
}

const DOWN_DURATION_S: f64 = 0.18;
const UP_DURATION_S: f64 = 0.2;
/// Deeper than a weapon switch: it has to clear the frame.
const Y_DROP: f64 = -90.0;
const ROLL: f64 = std::f64::consts::PI / 5.0;

/// `WeaponHolsterAnim`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeaponHolsterAnim {
    /// 0 fully raised, 1 fully stowed.
    progress: f64,
    target: f64,
}

impl WeaponHolsterAnim {
    /// `setHolstered`.
    pub fn set_holstered(&mut self, holstered: bool) {
        self.target = if holstered { 1.0 } else { 0.0 };
    }

    /// `update`: toward the target, `delta_time` in seconds.
    pub fn update(&mut self, delta_time: f64) {
        if self.progress == self.target {
            return;
        }
        let duration = if self.target > self.progress {
            DOWN_DURATION_S
        } else {
            UP_DURATION_S
        };
        let step = if duration > 0.0 { delta_time / duration } else { 1.0 };
        self.progress = if self.target > self.progress {
            self.target.min(self.progress + step)
        } else {
            self.target.max(self.progress - step)
        };
    }

    /// `getOffsets`: ease-out cubic, as the switch uses.
    #[must_use]
    pub fn offsets(&self) -> HolsterAnimOffsets {
        if self.progress == 0.0 {
            return HolsterAnimOffsets::default();
        }
        let eased = 1.0 - (1.0 - self.progress).powi(3);
        HolsterAnimOffsets {
            y_offset: eased * Y_DROP,
            roll: eased * ROLL,
            ..HolsterAnimOffsets::default()
        }
    }

    /// `isFullyHolstered`: stowed and settled, so the mesh can stop drawing.
    #[must_use]
    pub fn is_fully_holstered(&self) -> bool {
        self.progress >= 1.0
    }

    /// `isActive`: not fully raised, stowed or moving.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.progress > 0.0 || self.target > 0.0
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stows_stays_and_comes_back_from_where_it_is() {
        let mut h = WeaponHolsterAnim::default();
        assert_eq!(h.offsets(), HolsterAnimOffsets::default());
        h.set_holstered(true);
        assert!(h.is_active());
        h.update(0.09);
        assert!(!h.is_fully_holstered());
        let half = h.offsets();
        assert!((half.y_offset - Y_DROP * 0.875).abs() < 1e-9, "{half:?}");
        h.update(1.0);
        assert!(h.is_fully_holstered());
        assert_eq!(h.offsets().y_offset, Y_DROP);
        // Let go: up over 0.2 s, from where it is.
        h.set_holstered(false);
        h.update(0.1);
        assert!(!h.is_fully_holstered());
        assert!((h.offsets().y_offset - Y_DROP * 0.875).abs() < 1e-9);
        h.update(0.1);
        assert_eq!(h.offsets(), HolsterAnimOffsets::default());
        assert!(!h.is_active());
    }
}
