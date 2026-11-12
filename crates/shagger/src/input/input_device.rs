//! Ported from SHAG `engine/input/InputDevice.ts`.
//!
//! Which input the player is using: keyboard and mouse, or a controller
//! (touch is the browser's; `in_touch` can still force either device).
//! The device follows the last deliberate input: a key, or a mouse that
//! moved.
//!
//! Native difference: the browser pins controller mode from its start
//! screen or Settings, and only Settings hands it back. The native window
//! has neither choice, so a press on a standard pad picks the controller
//! ([`InputDevice::follow_pad`]) and a key or a moving mouse picks the
//! keyboard again. Each change is announced once through
//! [`InputDevice::take_changed`] (`input:device_changed`).

/// `InputDeviceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputDeviceKind {
    Touch,
    #[default]
    KeyboardMouse,
    Gamepad,
}

/// `InputDevice`.
#[derive(Debug, Clone, Default)]
pub struct InputDevice {
    followed: InputDeviceKind,
    published: InputDeviceKind,
    changed: Option<InputDeviceKind>,
}

impl InputDevice {
    /// `current`: the device to show controls for; `in_touch` (-1 follows,
    /// 0 keyboard and mouse, 1 touch) overrides it.
    #[must_use]
    pub fn current(&self, in_touch: f64) -> InputDeviceKind {
        if in_touch == 0.0 {
            return InputDeviceKind::KeyboardMouse;
        }
        if in_touch == 1.0 {
            return InputDeviceKind::Touch;
        }
        self.followed
    }

    /// `current`, with `in_touch` read from the cvars.
    #[must_use]
    pub fn current_now(&self) -> InputDeviceKind {
        let forced = crate::core::cvar::with_cvars(|c| {
            if c.exists("in_touch") {
                c.get_number("in_touch")
            } else {
                -1.0
            }
        });
        self.current(forced)
    }

    /// Whether a controller is the device now.
    #[must_use]
    pub fn gamepad(&self) -> bool {
        self.current_now() == InputDeviceKind::Gamepad
    }

    /// `publish`: announce the device when it differs from the last one
    /// announced.
    pub fn publish(&mut self, in_touch: f64) {
        let device = self.current(in_touch);
        if device == self.published {
            return;
        }
        self.published = device;
        self.changed = Some(device);
    }

    /// `select`: an explicit choice.
    pub fn select(&mut self, device: InputDeviceKind) {
        self.followed = device;
        self.publish(-1.0);
    }

    /// A key, a click or a moving mouse (`onKeyDown`, `onPointerMove`,
    /// `onPointerDown`).
    pub fn follow_keyboard_mouse(&mut self) {
        self.follow(InputDeviceKind::KeyboardMouse);
    }

    /// A press on a standard pad: native only (see the module notes).
    pub fn follow_pad(&mut self) {
        self.follow(InputDeviceKind::Gamepad);
    }

    fn follow(&mut self, device: InputDeviceKind) {
        if device == self.followed {
            return;
        }
        self.followed = device;
        self.publish(-1.0);
    }

    /// The device announced since the last call, once.
    pub fn take_changed(&mut self) -> Option<InputDeviceKind> {
        self.changed.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_on_a_key_or_a_pad_and_announces_each_switch_once() {
        let mut device = InputDevice::default();
        assert_eq!(device.current(-1.0), InputDeviceKind::KeyboardMouse);
        device.follow_keyboard_mouse();
        assert_eq!(device.take_changed(), None);
        device.follow_pad();
        assert_eq!(device.current(-1.0), InputDeviceKind::Gamepad);
        assert_eq!(device.take_changed(), Some(InputDeviceKind::Gamepad));
        device.follow_pad();
        assert_eq!(device.take_changed(), None);
        device.follow_keyboard_mouse();
        assert_eq!(device.take_changed(), Some(InputDeviceKind::KeyboardMouse));
    }

    #[test]
    fn forces_either_device() {
        let mut device = InputDevice::default();
        device.select(InputDeviceKind::Gamepad);
        assert_eq!(device.current(0.0), InputDeviceKind::KeyboardMouse);
        assert_eq!(device.current(1.0), InputDeviceKind::Touch);
        assert_eq!(device.current(-1.0), InputDeviceKind::Gamepad);
    }
}
