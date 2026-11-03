//! SHAG `src/engine/weapon/WeaponSlash.ts`: the melee swing (windup, swing,
//! return), cycling through three presets. The animation half; the trail
//! ribbon is not ported yet.
//!
//! The browser reads `performance.now()`; here the caller passes its clock
//! in milliseconds.

/// One swing's shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlashParams {
    pub start_pitch: f64,
    pub start_roll: f64,
    pub start_yaw: f64,
    pub end_pitch: f64,
    pub end_roll: f64,
    pub end_yaw: f64,
    pub start_offset: [f64; 3],
    pub end_offset: [f64; 3],
    pub windup_duration: f64,
    pub swing_duration: f64,
    pub return_duration: f64,
}

/// `SLASH_PRESETS`, cycled on each swing.
pub const SLASH_PRESETS: [SlashParams; 3] = [
    // Big vertical down: raised high, sweeps down with a lunge.
    SlashParams {
        start_pitch: 0.0,
        start_roll: -2.0,
        start_yaw: 0.0,
        end_pitch: 0.0,
        end_roll: 1.4,
        end_yaw: 0.0,
        start_offset: [5.0, 30.0, -15.0],
        end_offset: [-5.0, -25.0, 18.0],
        windup_duration: 60.0,
        swing_duration: 160.0,
        return_duration: 230.0,
    },
    // Diagonal, upper right to lower left.
    SlashParams {
        start_pitch: 0.7,
        start_roll: -1.8,
        start_yaw: -0.4,
        end_pitch: -0.7,
        end_roll: 1.2,
        end_yaw: 0.4,
        start_offset: [-12.0, 28.0, -12.0],
        end_offset: [12.0, -22.0, 15.0],
        windup_duration: 55.0,
        swing_duration: 165.0,
        return_duration: 230.0,
    },
    // Diagonal, upper left to lower right.
    SlashParams {
        start_pitch: -0.7,
        start_roll: -1.8,
        start_yaw: 0.4,
        end_pitch: 0.7,
        end_roll: 1.2,
        end_yaw: -0.4,
        start_offset: [12.0, 28.0, -12.0],
        end_offset: [-12.0, -22.0, 15.0],
        windup_duration: 55.0,
        swing_duration: 165.0,
        return_duration: 230.0,
    },
];

/// `SlashAnimOffsets`: position and radians on the weapon root.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SlashAnimOffsets {
    pub x_offset: f64,
    pub y_offset: f64,
    pub z_offset: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
}

/// `WeaponSlash`'s animation.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponSlash {
    active: bool,
    start_ms: f64,
    params: SlashParams,
    preset_index: usize,
}

impl Default for WeaponSlash {
    fn default() -> Self {
        Self {
            active: false,
            start_ms: 0.0,
            params: SLASH_PRESETS[0],
            preset_index: 0,
        }
    }
}

impl WeaponSlash {
    /// A swing is animating.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// `trigger`: the next preset, from `now_ms`.
    pub fn trigger(&mut self, now_ms: f64) {
        self.params = SLASH_PRESETS[self.preset_index % SLASH_PRESETS.len()];
        self.preset_index += 1;
        self.active = true;
        self.start_ms = now_ms;
    }

    /// `getOffsets` at `now_ms`; the swing ends once its time is up.
    pub fn offsets(&mut self, now_ms: f64) -> SlashAnimOffsets {
        if !self.active {
            return SlashAnimOffsets::default();
        }
        let elapsed = now_ms - self.start_ms;
        let p = self.params;
        let total = p.windup_duration + p.swing_duration + p.return_duration;
        if elapsed >= total {
            self.active = false;
            return SlashAnimOffsets::default();
        }
        let zero = [0.0; 3];
        let (t, from, to, from_off, to_off) = if elapsed < p.windup_duration {
            let t = if p.windup_duration > 0.0 {
                elapsed / p.windup_duration
            } else {
                1.0
            };
            (
                t * t,
                [0.0; 3],
                [p.start_pitch, p.start_roll, p.start_yaw],
                zero,
                p.start_offset,
            )
        } else if elapsed < p.windup_duration + p.swing_duration {
            let t = (elapsed - p.windup_duration) / p.swing_duration;
            (
                1.0 - (1.0 - t) * (1.0 - t),
                [p.start_pitch, p.start_roll, p.start_yaw],
                [p.end_pitch, p.end_roll, p.end_yaw],
                p.start_offset,
                p.end_offset,
            )
        } else {
            let t = (elapsed - p.windup_duration - p.swing_duration) / p.return_duration;
            let eased = if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            };
            (
                eased,
                [p.end_pitch, p.end_roll, p.end_yaw],
                [0.0; 3],
                p.end_offset,
                zero,
            )
        };
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        SlashAnimOffsets {
            x_offset: lerp(from_off[0], to_off[0]),
            y_offset: lerp(from_off[1], to_off[1]),
            z_offset: lerp(from_off[2], to_off[2]),
            pitch: lerp(from[0], to[0]),
            roll: lerp(from[1], to[1]),
            yaw: lerp(from[2], to[2]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swing_winds_up_sweeps_and_returns() {
        let mut s = WeaponSlash::default();
        assert_eq!(s.offsets(0.0), SlashAnimOffsets::default());
        s.trigger(1000.0);
        // End of the windup: the first preset's start pose.
        let o = s.offsets(1060.0);
        assert!((o.y_offset - 30.0).abs() < 1e-9 && (o.roll + 2.0).abs() < 1e-9);
        // End of the swing.
        let o = s.offsets(1220.0);
        assert!((o.y_offset + 25.0).abs() < 1e-9 && (o.z_offset - 18.0).abs() < 1e-9);
        assert!(s.is_active());
        assert_eq!(s.offsets(1450.0), SlashAnimOffsets::default());
        assert!(!s.is_active());
        // The next swing is the second preset.
        s.trigger(2000.0);
        let o = s.offsets(2055.0);
        assert!((o.x_offset + 12.0).abs() < 1e-9 && (o.yaw + 0.4).abs() < 1e-9);
    }
}
