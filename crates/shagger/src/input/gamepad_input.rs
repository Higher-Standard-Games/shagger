//! Ported from SHAG `engine/input/GamepadInput.ts`.
//!
//! Pad sampling. Connection is availability; callers decide activation.
//! The browser reads `navigator.getGamepads()` each animation frame and
//! calls its subscribers; here the host polls (`gamepad_backend`) and
//! hands the pads to [`GamepadInput::sample`] each frame, then reads the
//! edges and axes itself.

use std::collections::BTreeSet;

use shagger_shared::js::js_hypot;

/// One button of a pad: how far it is pushed and whether it is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PadButton {
    pub value: f64,
    pub pressed: bool,
}

/// `PadState`: one pad, in the W3C standard layout when `mapping` is
/// `"standard"` (axes 0-1 the left stick, 2-3 the right, down positive).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PadState {
    pub index: usize,
    pub id: String,
    pub mapping: String,
    pub axes: Vec<f64>,
    pub buttons: Vec<PadButton>,
}

/// `PAD_LABELS`: the standard layout's buttons, as Xbox names them.
pub const PAD_LABELS: [&str; 16] = [
    "A",
    "B",
    "X",
    "Y",
    "LB",
    "RB",
    "LT",
    "RT",
    "View",
    "Menu",
    "LS",
    "RS",
    "D-pad Up",
    "D-pad Down",
    "D-pad Left",
    "D-pad Right",
];

/// `padLabel`.
#[must_use]
pub fn pad_label(button: u64) -> String {
    usize::try_from(button)
        .ok()
        .and_then(|b| PAD_LABELS.get(b))
        .map_or_else(|| format!("Button {button}"), |l| (*l).to_owned())
}

/// `padStick`: radial filtering preserves direction and smoothly remaps the
/// remaining travel.
#[must_use]
pub fn pad_stick(x: f64, y: f64, dead_zone: f64, exponent: f64) -> (f64, f64) {
    if !x.is_finite() || !y.is_finite() {
        return (0.0, 0.0);
    }
    let length = js_hypot(&[x, y]);
    let dead = dead_zone.clamp(0.0, 0.9);
    if length <= dead {
        return (0.0, 0.0);
    }
    let scaled = ((length.min(1.0) - dead) / (1.0 - dead)).powf(exponent.max(0.1));
    (x / length * scaled, y / length * scaled)
}

/// `GamepadInput`.
#[derive(Debug, Clone)]
pub struct GamepadInput {
    pub pads: Vec<PadState>,
    /// The pad in use.
    pub pad: Option<PadState>,
    pub down: BTreeSet<usize>,
    pub pressed: BTreeSet<usize>,
    pub released: BTreeSet<usize>,
    /// The host can read pads at all.
    pub available: bool,
    preferred: Option<usize>,
    blocked: bool,
    stick_dead_zones: [f64; 2],
}

impl Default for GamepadInput {
    fn default() -> Self {
        Self {
            pads: Vec::new(),
            pad: None,
            down: BTreeSet::new(),
            pressed: BTreeSet::new(),
            released: BTreeSet::new(),
            available: true,
            preferred: None,
            blocked: false,
            stick_dead_zones: [0.12, 0.12],
        }
    }
}

impl GamepadInput {
    /// `setStickDeadZones`: the same radial dead zones for handoff safety
    /// as for stick input.
    pub fn set_stick_dead_zones(&mut self, left: f64, right: f64) {
        self.stick_dead_zones = [left, right];
    }

    /// `select`.
    pub fn select(&mut self, index: usize) {
        self.preferred = Some(index);
        self.suppress_until_neutral();
    }

    /// `suppressUntilNeutral`.
    pub fn suppress_until_neutral(&mut self) {
        self.blocked = self.pad.is_none() || !self.neutral();
        self.down.clear();
        self.pressed.clear();
    }

    /// `neutral`: no button and neither stick out of its dead zone.
    #[must_use]
    pub fn neutral(&self) -> bool {
        let Some(pad) = &self.pad else { return true };
        let axis = |i: usize| pad.axes.get(i).copied().unwrap_or(0.0);
        !pad.buttons.iter().any(|b| b.pressed || b.value >= 0.35)
            && self
                .stick_dead_zones
                .iter()
                .enumerate()
                .all(|(stick, dead)| pad_stick(axis(stick * 2), axis(stick * 2 + 1), *dead, 1.0) == (0.0, 0.0))
    }

    /// `sample`: this frame's pads.
    pub fn sample(&mut self, pads: Vec<PadState>) {
        let before = std::mem::take(&mut self.down);
        self.pads = pads;
        let previous = self.pad.as_ref().map(|p| p.index);
        let find = |f: &dyn Fn(&PadState) -> bool| self.pads.iter().find(|p| f(p)).cloned();
        self.pad = find(&|p| Some(p.index) == self.preferred)
            .or_else(|| find(&|p| Some(p.index) == previous))
            .or_else(|| find(&|p| p.mapping == "standard"))
            .or_else(|| self.pads.first().cloned());
        self.down = before.clone();
        if previous.is_some() && self.pad.as_ref().map(|p| p.index) != previous {
            self.suppress_until_neutral();
        }
        let mut next = BTreeSet::new();
        if self.blocked && self.neutral() {
            self.blocked = false;
        }
        if !self.blocked
            && let Some(pad) = self.pad.as_ref().filter(|p| p.mapping == "standard")
        {
            for (index, button) in pad.buttons.iter().enumerate() {
                let threshold = if self.down.contains(&index) { 0.35 } else { 0.55 };
                if button.pressed || button.value >= threshold {
                    next.insert(index);
                }
            }
        }
        self.pressed = next.difference(&before).copied().collect();
        self.released = before.difference(&next).copied().collect();
        self.down = next;
    }

    /// `axis`: 0 while blocked or on an unknown layout.
    #[must_use]
    pub fn axis(&self, index: usize) -> f64 {
        match &self.pad {
            Some(pad) if !self.blocked && pad.mapping == "standard" => pad.axes.get(index).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// `stop`'s reset: no pads, and nothing until the controls are neutral.
    pub fn reset(&mut self) {
        self.sample(Vec::new());
        self.suppress_until_neutral();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad_at(buttons: &[usize], axes: [f64; 4], index: usize) -> PadState {
        PadState {
            index,
            id: "Xbox test".into(),
            mapping: "standard".into(),
            axes: axes.to_vec(),
            buttons: (0..17)
                .map(|i| PadButton {
                    value: if buttons.contains(&i) { 1.0 } else { 0.0 },
                    pressed: buttons.contains(&i),
                })
                .collect(),
        }
    }

    fn pad(buttons: &[usize], axes: [f64; 4]) -> PadState {
        pad_at(buttons, axes, 0)
    }

    const REST: [f64; 4] = [0.0; 4];

    #[test]
    fn rearms_with_drift_inside_the_configured_radial_dead_zones() {
        let mut input = GamepadInput::default();
        input.set_stick_dead_zones(0.25, 0.15);
        input.sample(vec![pad(&[7], REST)]);
        input.suppress_until_neutral();
        input.sample(vec![pad(&[], [0.2, 0.2, 0.1, 0.0])]);
        assert!(!input.neutral());
        assert_eq!(input.axis(0), 0.0);
        input.sample(vec![pad(&[], [0.2, 0.1, 0.1, 0.0])]);
        assert!(input.neutral());
        input.sample(vec![pad(&[7], [0.2, 0.1, 0.1, 0.0])]);
        assert!(input.pressed.contains(&7));
    }

    #[test]
    fn reads_sparse_device_lists_and_edges_without_treating_a_connection_as_input() {
        let mut input = GamepadInput::default();
        input.sample(vec![pad_at(&[], REST, 1)]);
        assert_eq!(input.pad.as_ref().map(|p| p.index), Some(1));
        assert!(input.pressed.is_empty());
        input.sample(vec![pad_at(&[0], REST, 1)]);
        assert_eq!(input.pressed.iter().copied().collect::<Vec<_>>(), [0]);
        input.sample(vec![pad_at(&[0], REST, 1)]);
        assert!(input.pressed.is_empty());
        input.sample(Vec::new());
        assert_eq!(input.released.iter().copied().collect::<Vec<_>>(), [0]);
        assert!(input.pad.is_none());
    }

    #[test]
    fn requires_neutral_input_after_a_handoff_including_a_displaced_stick() {
        let mut input = GamepadInput::default();
        input.sample(vec![pad(&[7], REST)]);
        input.suppress_until_neutral();
        input.sample(vec![pad(&[7], [0.8, 0.0, 0.0, 0.0])]);
        assert!(input.down.is_empty());
        assert_eq!(input.axis(0), 0.0);
        input.sample(vec![pad(&[], [0.8, 0.0, 0.0, 0.0])]);
        assert_eq!(input.axis(0), 0.0);
        input.sample(vec![pad(&[], REST)]);
        input.sample(vec![pad(&[7], REST)]);
        assert!(input.pressed.contains(&7));
    }

    #[test]
    fn rejects_unknown_layouts_and_never_merges_two_pads() {
        let mut input = GamepadInput::default();
        input.sample(vec![PadState {
            mapping: String::new(),
            ..pad(&[0], REST)
        }]);
        assert!(input.down.is_empty());
        input.select(2);
        input.sample(vec![pad(&[0], REST), pad_at(&[], REST, 2)]);
        input.sample(vec![pad(&[0], REST), pad_at(&[1], REST, 2)]);
        assert_eq!(input.down.iter().copied().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn filters_radial_drift_smoothly_and_preserves_diagonal_direction() {
        assert_eq!(pad_stick(0.05, 0.05, 0.12, 1.0), (0.0, 0.0));
        assert!((pad_stick(0.56, 0.0, 0.12, 1.0).0 - 0.5).abs() < 1e-9);
        let (x, y) = pad_stick(1.0, 1.0, 0.12, 1.7);
        assert!((x.hypot(y) - 1.0).abs() < 1e-9);
        assert!((x - y).abs() < 1e-12);
        assert_eq!(pad_stick(f64::NAN, 1.0, 0.12, 1.0), (0.0, 0.0));
    }

    #[test]
    fn labels_name_the_standard_buttons() {
        assert_eq!(pad_label(0), "A");
        assert_eq!(pad_label(15), "D-pad Right");
        assert_eq!(pad_label(16), "Button 16");
    }
}
