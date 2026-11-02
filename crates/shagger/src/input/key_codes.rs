//! winit's keys and buttons under the names the browser gives them
//! (`KeyboardEvent.code`), which the saved bindings use. Not a SHAG port: the
//! browser names keys itself.

use winit::event::MouseButton as WinitButton;
use winit::keyboard::KeyCode;

use super::input_types::MouseButton;

/// `KeyboardEvent.code` for a winit key: winit names most keys as the
/// browser does (`KeyW`, `Digit1`, `ShiftLeft`, `Backquote`, `F4`).
#[must_use]
pub fn dom_code(key: KeyCode) -> String {
    match key {
        KeyCode::SuperLeft => "MetaLeft".to_owned(),
        KeyCode::SuperRight => "MetaRight".to_owned(),
        other => format!("{other:?}"),
    }
}

/// The browser's three mouse buttons.
#[must_use]
pub fn dom_mouse_button(button: WinitButton) -> Option<MouseButton> {
    match button {
        WinitButton::Left => Some(MouseButton::Left),
        WinitButton::Right => Some(MouseButton::Right),
        WinitButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_take_the_browser_names() {
        let cases = [
            (KeyCode::KeyW, "KeyW"),
            (KeyCode::Digit1, "Digit1"),
            (KeyCode::Space, "Space"),
            (KeyCode::ShiftLeft, "ShiftLeft"),
            (KeyCode::ControlRight, "ControlRight"),
            (KeyCode::Backquote, "Backquote"),
            (KeyCode::F4, "F4"),
            (KeyCode::ArrowUp, "ArrowUp"),
            (KeyCode::BracketLeft, "BracketLeft"),
            (KeyCode::SuperLeft, "MetaLeft"),
            (KeyCode::Enter, "Enter"),
        ];
        for (key, name) in cases {
            assert_eq!(dom_code(key), name);
        }
    }
}
