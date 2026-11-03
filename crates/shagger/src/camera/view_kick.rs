//! SHAG `src/engine/camera/controllers/ViewKick.ts`: the view kick of our
//! own shots. The camera jumps up (and a little sideways) on each shot and
//! settles back. Visual only: never moves where the next shot goes.

/// Current offsets in radians; negative pitch is up (Babylon).
#[derive(Debug, Clone, Default)]
pub struct ViewKick {
    pub pitch: f64,
    pub yaw: f64,
    recover_rate: f64,
}

impl ViewKick {
    /// One shot: up by `pitch_degrees`, sideways by `yaw_degrees`, settling
    /// over about `recover_seconds`. Kicks stack, capped so a long burst
    /// shakes rather than climbs.
    pub fn kick(&mut self, pitch_degrees: f64, yaw_degrees: f64, recover_seconds: f64) {
        let to_radians = std::f64::consts::PI / 180.0;
        let max_pitch = (pitch_degrees * 2.5).max(0.5) * to_radians;
        self.pitch = (-max_pitch).max(self.pitch - pitch_degrees * to_radians);
        self.yaw += yaw_degrees * to_radians;
        // Exponential settle, about 98% gone after recover_seconds.
        self.recover_rate = 4.0 / recover_seconds.max(0.02);
    }

    /// Settles toward zero by `delta_time` seconds.
    pub fn settle(&mut self, delta_time: f64) {
        if self.pitch == 0.0 && self.yaw == 0.0 {
            return;
        }
        let factor = (-self.recover_rate * delta_time).exp();
        self.pitch = if (self.pitch * factor).abs() < 1e-5 {
            0.0
        } else {
            self.pitch * factor
        };
        self.yaw = if (self.yaw * factor).abs() < 1e-5 {
            0.0
        } else {
            self.yaw * factor
        };
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kicks_stack_to_a_cap_and_settle() {
        let mut k = ViewKick::default();
        for _ in 0..10 {
            k.kick(1.0, 0.2, 0.2);
        }
        assert!((k.pitch + 2.5f64.to_radians()).abs() < 1e-12);
        k.settle(0.2);
        assert!(k.pitch.abs() < 2.5f64.to_radians() * 0.02);
        k.settle(1.0);
        assert_eq!(k.pitch, 0.0);
        assert_eq!(k.yaw, 0.0);
    }
}
