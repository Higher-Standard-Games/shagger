//! SHAG `src/engine/weapon/WeaponReloadAnim.ts`: the weapon tilts, pumps
//! and settles over the reload's time. The caller passes its clock in
//! seconds.

/// `ReloadAnimOffsets`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReloadAnimOffsets {
    pub roll: f64,
    pub pitch: f64,
    pub x_offset: f64,
    pub y_offset: f64,
    pub z_offset: f64,
}

/// `WeaponReloadAnim`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeaponReloadAnim {
    active: bool,
    start: f64,
    duration: f64,
    /// The slam's jitter (`Math.random`).
    seed: u32,
}

impl WeaponReloadAnim {
    /// `start`: a reload of `duration` seconds from `now`.
    pub fn start(&mut self, duration: f64, now: f64) {
        self.active = true;
        self.start = now;
        self.duration = duration;
    }

    pub fn cancel(&mut self) {
        self.active = false;
    }

    #[must_use]
    pub fn is_reloading(&self) -> bool {
        self.active
    }

    /// 0 to 1.
    #[must_use]
    pub fn progress(&self, now: f64) -> f64 {
        if !self.active {
            return 0.0;
        }
        ((now - self.start) / self.duration).min(1.0)
    }

    fn random(&mut self) -> f64 {
        // xorshift32: the jitter only has to look random.
        let mut x = self.seed.wrapping_add(0x9e37_79b9) | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        f64::from(x) / f64::from(u32::MAX)
    }

    /// `getOffsets` at `now`; the reload ends once its time is up.
    pub fn offsets(&mut self, now: f64) -> ReloadAnimOffsets {
        if !self.active {
            return ReloadAnimOffsets::default();
        }
        let p = self.progress(now);
        if p >= 1.0 {
            self.active = false;
            return ReloadAnimOffsets::default();
        }
        const MAX_ROLL: f64 = std::f64::consts::PI / 4.0;
        const MAX_PITCH: f64 = std::f64::consts::PI / 8.0;
        const DIP_Y: f64 = -4.0;
        const SLIDE_DIST: f64 = -9.0;
        // A snappy tilt in, held, then settled out.
        let tilt = if p < 0.25 {
            1.0 - (1.0 - p / 0.25).powi(3)
        } else if p > 0.75 {
            let t = (1.0 - p) / 0.25;
            t * t * (3.0 - 2.0 * t)
        } else {
            1.0
        };
        let mut o = ReloadAnimOffsets {
            roll: tilt * MAX_ROLL,
            pitch: tilt * MAX_PITCH,
            y_offset: tilt * DIP_Y,
            ..ReloadAnimOffsets::default()
        };
        // The pump: back, hold, slammed home.
        let mut pump = 0.0;
        if p > 0.3 && p < 0.7 {
            if p < 0.45 {
                pump = ((p - 0.3) / 0.15 * std::f64::consts::PI / 2.0).sin() * SLIDE_DIST;
            } else if p < 0.55 {
                pump = SLIDE_DIST;
            } else {
                let t = (p - 0.55) / 0.15;
                pump = (1.0 - t) * SLIDE_DIST;
                if t < 0.3 {
                    let shake = (1.0 - t / 0.3) * 0.5;
                    o.x_offset += (self.random() - 0.5) * shake;
                    o.y_offset += (self.random() - 0.5) * shake;
                }
            }
        }
        o.y_offset += pump;
        o.x_offset += pump * -0.5;
        o.z_offset += pump * 0.2;
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilts_pumps_and_ends() {
        let mut r = WeaponReloadAnim::default();
        r.start(1.0, 10.0);
        let o = r.offsets(10.5);
        assert!((o.roll - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((o.y_offset - (-4.0 - 9.0)).abs() < 1e-12);
        assert!((o.x_offset - 4.5).abs() < 1e-12);
        assert!(r.is_reloading());
        assert_eq!(r.offsets(11.0), ReloadAnimOffsets::default());
        assert!(!r.is_reloading());
    }
}
