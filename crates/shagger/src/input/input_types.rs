//! SHAG `src/engine/input/InputTypes.ts`: the binding and action shapes.
//! Bindings serialise as the TypeScript objects do, fields in the same order.

use serde::Serialize;

/// `MouseButton`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    /// The name the TypeScript uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
        }
    }

    /// `"left" | "right" | "middle"`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "middle" => Some(Self::Middle),
            _ => None,
        }
    }
}

/// `scrollDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    /// The name the TypeScript uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    /// `"up" | "down"`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

/// `InputBinding.modifiers`: each in the order the saved object gave it.
pub type Modifiers = Vec<(Modifier, bool)>;

/// A modifier key a binding may require.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
}

/// `InputBinding`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputBinding {
    /// A keyboard `code` (`"KeyW"`, `"Space"`, `"ArrowUp"`).
    pub key: Option<String>,
    pub mouse_button: Option<MouseButton>,
    pub scroll_direction: Option<ScrollDirection>,
    /// Touch gesture (future).
    pub gesture: Option<String>,
    /// Gamepad button (future).
    pub gamepad_button: Option<u64>,
    pub modifiers: Option<Modifiers>,
}

impl InputBinding {
    /// `{ key }`.
    #[must_use]
    pub fn key(code: &str) -> Self {
        Self {
            key: Some(code.to_owned()),
            ..Self::default()
        }
    }

    /// `{ mouseButton }`.
    #[must_use]
    pub fn mouse(button: MouseButton) -> Self {
        Self {
            mouse_button: Some(button),
            ..Self::default()
        }
    }

    /// `{ scrollDirection }`.
    #[must_use]
    pub fn scroll(direction: ScrollDirection) -> Self {
        Self {
            scroll_direction: Some(direction),
            ..Self::default()
        }
    }
}

impl Serialize for InputBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        if let Some(key) = &self.key {
            map.serialize_entry("key", key)?;
        }
        if let Some(button) = self.mouse_button {
            map.serialize_entry("mouseButton", button.name())?;
        }
        if let Some(direction) = self.scroll_direction {
            map.serialize_entry("scrollDirection", direction.name())?;
        }
        if let Some(gesture) = &self.gesture {
            map.serialize_entry("gesture", gesture)?;
        }
        if let Some(button) = self.gamepad_button {
            map.serialize_entry("gamepadButton", &button)?;
        }
        if let Some(modifiers) = &self.modifiers {
            let entries: serde_json::Map<String, serde_json::Value> = modifiers
                .iter()
                .map(|(m, on)| {
                    let name = match m {
                        Modifier::Ctrl => "ctrl",
                        Modifier::Shift => "shift",
                        Modifier::Alt => "alt",
                    };
                    (name.to_owned(), serde_json::Value::Bool(*on))
                })
                .collect();
            map.serialize_entry("modifiers", &entries)?;
        }
        map.end()
    }
}

/// `ActionType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionType {
    Button,
    Axis,
}

/// `ActionDefinition`.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionDefinition {
    pub id: String,
    pub kind: ActionType,
    pub name: String,
    pub default_bindings: Vec<InputBinding>,
    /// Capture this action's keys during fullscreen mouse-controlled gameplay.
    pub lock_in_fullscreen: bool,
}

impl ActionDefinition {
    /// A button action.
    #[must_use]
    pub fn button(id: &str, name: &str, default_bindings: Vec<InputBinding>) -> Self {
        Self {
            id: id.to_owned(),
            kind: ActionType::Button,
            name: name.to_owned(),
            default_bindings,
            lock_in_fullscreen: false,
        }
    }

    /// `lockInFullscreen: true`.
    #[must_use]
    pub fn lock_in_fullscreen(mut self) -> Self {
        self.lock_in_fullscreen = true;
        self
    }
}

/// `ContextDefinition`.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextDefinition {
    pub id: String,
    /// Blocks input to contexts below in the stack.
    pub blocking: bool,
    pub actions: Vec<String>,
    /// Bindings that override the actions' defaults in this context.
    pub bindings: Vec<(String, Vec<InputBinding>)>,
}

/// `ActionState`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ActionState {
    /// Held.
    pub active: bool,
    /// Pressed this frame.
    pub pressed: bool,
    /// Released this frame.
    pub released: bool,
}
