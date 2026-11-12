//! The pads, read through gilrs: the native stand-in for the browser's
//! `navigator.getGamepads()` (`GamepadInput.tick`).
//!
//! A pad gilrs knows the layout of (an SDL mapping, or a driver with the
//! unified layout) reads as the W3C standard layout: buttons 0-16 in its
//! order, axes 0-1 the left stick and 2-3 the right, down positive. Any
//! other pad reads with an empty mapping, which `GamepadInput` ignores.

use gilrs::{Axis, Button, Gilrs, MappingSource};

use crate::input::gamepad_input::{PadButton, PadState};

/// The standard layout's buttons, in order.
const STANDARD_BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Mode,
];

/// The pads on this machine; `None` inside when gilrs could not start
/// (no pad support here), and the pads read as none.
pub struct GamepadBackend {
    gilrs: Option<Gilrs>,
}

impl Default for GamepadBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl GamepadBackend {
    #[must_use]
    pub fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(g) => Some(g),
            Err(gilrs::Error::NotImplemented(g)) => {
                eprintln!("gamepad: not supported on this platform");
                Some(g)
            }
            Err(e) => {
                eprintln!("gamepad: {e}");
                None
            }
        };
        Self { gilrs }
    }

    /// Whether pads can be read at all (`GamepadInput.available`).
    #[must_use]
    pub fn available(&self) -> bool {
        self.gilrs.is_some()
    }

    /// This frame's pads, after taking the events waiting.
    pub fn poll(&mut self) -> Vec<PadState> {
        let Some(gilrs) = &mut self.gilrs else {
            return Vec::new();
        };
        while gilrs.next_event().is_some() {}
        gilrs
            .gamepads()
            .filter(|(_, pad)| pad.is_connected())
            .map(|(id, pad)| {
                let standard = pad.mapping_source() != MappingSource::None;
                let button = |b: Button| {
                    pad.button_data(b).map_or(PadButton::default(), |d| PadButton {
                        value: f64::from(d.value()),
                        pressed: d.is_pressed(),
                    })
                };
                let axis = |a: Axis, sign: f64| pad.axis_data(a).map_or(0.0, |d| f64::from(d.value()) * sign);
                PadState {
                    index: usize::from(id),
                    id: pad.name().to_owned(),
                    mapping: if standard { "standard".to_owned() } else { String::new() },
                    // gilrs reads up as positive; the standard layout, down.
                    axes: vec![
                        axis(Axis::LeftStickX, 1.0),
                        axis(Axis::LeftStickY, -1.0),
                        axis(Axis::RightStickX, 1.0),
                        axis(Axis::RightStickY, -1.0),
                    ],
                    buttons: STANDARD_BUTTONS.iter().map(|b| button(*b)).collect(),
                }
            })
            .collect()
    }
}
