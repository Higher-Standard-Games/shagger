//! SHAG `src/engine/weapon/WeaponLighting.ts`: the map's light grid at the
//! camera, eased, tints the first-person weapon.
//!
//! In the TypeScript the tint reaches a StandardMaterial layer's diffuse
//! colour (the guns' `_emit` layers, which draw unlit, so it shows nowhere),
//! the blade's light and the blade's reflection fill. So what changes on
//! screen is the blade, lit by its light in the map's colour.

use crate::loader::bsp::light_grid_sampling::LightSample;

/// How fast to ease toward the target.
const LIGHT_LERP_SPEED: f64 = 5.0;
/// How much the light grid moves the weapon away from white.
const LIGHT_INFLUENCE: f64 = 0.6;
/// The darkest a channel goes.
const MIN_LIGHT: f64 = 0.15;

/// `WeaponLighting`: the eased RGB, white at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponLighting {
    pub color: [f64; 3],
}

impl Default for WeaponLighting {
    fn default() -> Self {
        Self { color: [1.0; 3] }
    }
}

/// The colour a light grid sample asks for (white without one).
#[must_use]
pub fn target_color(sample: Option<&LightSample>) -> [f64; 3] {
    let Some(lighting) = sample else {
        return [1.0; 3];
    };
    let raw: [f64; 3] = std::array::from_fn(|c| (lighting.ambient[c] + lighting.directional[c]) / 255.0);
    let brightness = (raw[0] + raw[1] + raw[2]) / 3.0;
    let clamped = brightness.clamp(MIN_LIGHT, 1.0);
    let color_scale = clamped / brightness.max(0.01);
    std::array::from_fn(|c| {
        let scaled = (raw[c] * color_scale).clamp(MIN_LIGHT, 1.0);
        1.0 * (1.0 - LIGHT_INFLUENCE) + scaled * LIGHT_INFLUENCE
    })
}

impl WeaponLighting {
    /// `update`: ease toward the sample at the camera this frame.
    pub fn update(&mut self, delta_time: f64, sample: Option<&LightSample>) {
        let target = target_color(sample);
        let lf = (delta_time * LIGHT_LERP_SPEED).min(1.0);
        for (c, t) in self.color.iter_mut().zip(target) {
            *c += (t - *c) * lf;
        }
    }

    /// `reset`: back to white.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dark_red_room_tints_toward_red_and_eases() {
        let sample = LightSample {
            ambient: [60.0, 10.0, 10.0],
            directional: [40.0, 0.0, 0.0],
            direction: [0, 0],
        };
        let target = target_color(Some(&sample));
        // Brightness 40/255 holds; red keeps its share, the rest floor at 0.15.
        assert!((target[0] - (0.4 + 0.6 * (100.0 / 255.0))).abs() < 1e-12, "{target:?}");
        assert!((target[1] - (0.4 + 0.6 * MIN_LIGHT)).abs() < 1e-12, "{target:?}");
        let mut l = WeaponLighting::default();
        l.update(0.1, Some(&sample));
        assert!((l.color[1] - (1.0 + (target[1] - 1.0) * 0.5)).abs() < 1e-12);
        l.update(10.0, Some(&sample));
        assert_eq!(l.color, target);
        // Outside the grid: back to white.
        assert_eq!(target_color(None), [1.0; 3]);
    }
}
