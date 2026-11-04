//! SHAG `src/engine/weapon/WeaponCharge.ts`: the blade's charge-up on the
//! view model. Progress runs 0 to 1 over the charge time; the weapon
//! shakes harder and faster as it fills, and wears a pulsing turquoise
//! outline that thickens with it.
//!
//! The browser reads `performance.now()`; here the caller passes its clock
//! in milliseconds. The browser saves the outline it overwrites and puts it
//! back on release; here the outline is chosen every frame, so the charge
//! hands back the outline to wear (`None` once released) and nothing needs
//! restoring.

use glam::Vec3;

use crate::rendering::model_renderer::Outline;

/// `ChargeShake`: the offset the shake adds to the weapon's position.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ChargeShake {
    pub x: f64,
    pub y: f64,
}

/// What a frame of charging shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ChargeFrame {
    pub shake: ChargeShake,
    /// The turquoise outline, its width before the root's scale; `None`
    /// when not charging.
    pub outline: Option<Outline>,
}

/// `WeaponCharge`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeaponCharge {
    active: bool,
    start_time: f64,
    /// 0 to 1.
    pub progress: f64,
}

impl WeaponCharge {
    /// `update` at `now` ms, `charge_time` seconds (the weapon's
    /// `chargeTime`).
    pub fn update(&mut self, is_charging: bool, charge_time: f64, now: f64) -> ChargeFrame {
        if !is_charging {
            self.active = false;
            self.progress = 0.0;
            return ChargeFrame::default();
        }
        if !self.active {
            self.active = true;
            self.start_time = now;
        }
        let charge_ms = charge_time * 1000.0;
        let elapsed = now - self.start_time;
        self.progress = (elapsed / charge_ms).min(1.0);

        // Shake: sinusoidal, its intensity and speed growing with the charge.
        let shake_intensity = self.progress * 2.5;
        let shake_freq = 20.0 + self.progress * 30.0;
        let shake = ChargeShake {
            x: (now * 0.001 * shake_freq).sin() * shake_intensity,
            y: (now * 0.001 * shake_freq * 1.3).cos() * shake_intensity * 0.7,
        };

        // Turquoise pulsing outline.
        let (outline_min, outline_max) = (0.6, 4.8);
        let width = outline_min + self.progress * (outline_max - outline_min);
        let pulse_rate = 4.0 + self.progress * 8.0;
        let pulse = 0.6 + 0.4 * (now * 0.001 * pulse_rate * std::f64::consts::PI * 2.0).sin();
        let color = Vec3::new(0.05 * pulse as f32, pulse as f32, 0.9 * pulse as f32);
        ChargeFrame {
            shake,
            outline: Some(Outline {
                color,
                width: width as f32,
            }),
        }
    }

    /// `reset` (a weapon switch).
    pub fn reset(&mut self) {
        self.active = false;
        self.progress = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charge_fills_over_its_time_and_stops_on_release() {
        let mut charge = WeaponCharge::default();
        let start = charge.update(true, 1.0, 1000.0);
        assert_eq!(charge.progress, 0.0);
        // No shake at the start, and the thinnest outline.
        assert_eq!(start.shake, ChargeShake::default());
        assert!((start.outline.unwrap().width - 0.6).abs() < 1e-6);
        charge.update(true, 1.0, 1500.0);
        assert!((charge.progress - 0.5).abs() < 1e-12);
        let full = charge.update(true, 1.0, 3000.0);
        assert_eq!(charge.progress, 1.0);
        assert!((full.outline.unwrap().width - 4.8).abs() < 1e-6);
        let released = charge.update(false, 1.0, 3100.0);
        assert_eq!(released, ChargeFrame::default());
        assert_eq!(charge.progress, 0.0);
        // A new hold starts from empty.
        charge.update(true, 1.0, 5000.0);
        assert_eq!(charge.progress, 0.0);
    }

    #[test]
    fn the_shake_grows_with_the_charge() {
        let mut charge = WeaponCharge::default();
        charge.update(true, 1.0, 0.0);
        let full = charge.update(true, 1.0, 1013.0);
        let expected = (1013.0f64 * 0.001 * 50.0).sin() * 2.5;
        assert!((full.shake.x - expected).abs() < 1e-12);
        assert!(full.shake.x.abs() <= 2.5 && full.shake.y.abs() <= 1.75);
    }
}
