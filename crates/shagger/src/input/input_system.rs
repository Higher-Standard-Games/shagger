//! SHAG `src/engine/input/InputSystem.ts`: actions, contexts and bindings,
//! and each action's state from the raw keys and buttons a frame saw.
//!
//! The window feeds raw input under the browser's names
//! ([`super::key_codes`]); [`InputSystem::update`] runs before the frame's
//! systems read actions (`onBeforeRender`) and [`InputSystem::end_frame`]
//! after (`onAfterRender`). Saved overrides live in the synced storage entry
//! `shag_keybindings` (`synced-storage.json` in the config folder), where
//! settings sync finds them.
//!
//! Not ported: SHAG's built-in camera and menu actions (no Rust game uses
//! them), axes, touch and the fullscreen keyboard lock. Pads reach the
//! actions as virtual actions ([`InputSystem::set_virtual_action`]), as the
//! browser's `ControllerInput` sets them.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::Value;

use super::binding_validation::validated_binding_overrides;
use super::input_types::{
    ActionDefinition, ActionState, ContextDefinition, InputBinding, Modifier, MouseButton, ScrollDirection,
};
use shagger_shared::settings::synced_settings::json_string;

use crate::lobby::settings_sync::{load_synced_storage, save_synced_storage_entry};

/// The storage key the overrides are saved under.
pub const BINDINGS_KEY: &str = "shag_keybindings";

/// `RawInputState`, without pointer position and touch.
#[derive(Debug, Default)]
struct RawState {
    keys: HashSet<String>,
    keys_pressed: HashSet<String>,
    keys_released: HashSet<String>,
    buttons: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,
    /// Positive scrolls down.
    wheel: f64,
}

/// `InputSystem`.
#[derive(Debug)]
pub struct InputSystem {
    actions: Vec<ActionDefinition>,
    contexts: Vec<ContextDefinition>,
    context_stack: Vec<String>,
    states: Vec<(String, ActionState)>,
    raw: RawState,
    pending_releases: HashSet<String>,
    /// Insertion-ordered, as a `Map`.
    overrides: Vec<(String, Vec<InputBinding>)>,
    /// The folder `synced-storage.json` is in; `None` keeps nothing.
    storage: Option<PathBuf>,
    /// Actions held by something other than a binding (a pad), by source.
    virtual_held: HashSet<String>,
    virtual_sources: HashMap<String, HashSet<String>>,
    virtual_pressed: HashSet<String>,
    virtual_released: HashSet<String>,
}

impl Default for InputSystem {
    fn default() -> Self {
        Self::new(None)
    }
}

impl InputSystem {
    /// The folder `synced-storage.json` is kept in, which the settings sync
    /// must share so the bindings it brings down are the ones read.
    #[must_use]
    pub fn storage(&self) -> Option<&std::path::Path> {
        self.storage.as_deref()
    }

    /// An input system saving its bindings in `storage`, with nothing registered.
    #[must_use]
    pub fn new(storage: Option<PathBuf>) -> Self {
        Self {
            actions: Vec::new(),
            contexts: Vec::new(),
            context_stack: vec!["gameplay".to_owned()],
            states: Vec::new(),
            raw: RawState::default(),
            pending_releases: HashSet::new(),
            overrides: Vec::new(),
            storage,
            virtual_held: HashSet::new(),
            virtual_sources: HashMap::new(),
            virtual_pressed: HashSet::new(),
            virtual_released: HashSet::new(),
        }
    }

    /// `registerAction`.
    pub fn register_action(&mut self, action: ActionDefinition) {
        self.states.retain(|(id, _)| *id != action.id);
        self.states.push((action.id.clone(), ActionState::default()));
        self.actions.retain(|a| a.id != action.id);
        self.actions.push(action);
    }

    /// `registerContext`.
    pub fn register_context(&mut self, context: ContextDefinition) {
        self.contexts.retain(|c| c.id != context.id);
        self.contexts.push(context);
    }

    /// The registered action `id`.
    #[must_use]
    pub fn action(&self, id: &str) -> Option<&ActionDefinition> {
        self.actions.iter().find(|a| a.id == id)
    }

    fn context(&self, id: &str) -> Option<&ContextDefinition> {
        self.contexts.iter().find(|c| c.id == id)
    }

    /// `pushContext`.
    pub fn push_context(&mut self, id: &str) {
        if self.context(id).is_some() {
            self.context_stack.push(id.to_owned());
        }
    }

    /// `popContext`: never the last.
    pub fn pop_context(&mut self) -> Option<String> {
        if self.context_stack.len() <= 1 {
            return None;
        }
        self.context_stack.pop()
    }

    /// `setContext`.
    pub fn set_context(&mut self, id: &str) {
        self.context_stack = vec![id.to_owned()];
    }

    /// `getActiveContext`.
    #[must_use]
    pub fn active_context(&self) -> &str {
        self.context_stack.last().map_or("", String::as_str)
    }

    /// `isContextActive`: on the stack.
    #[must_use]
    pub fn is_context_active(&self, id: &str) -> bool {
        self.context_stack.iter().any(|c| c == id)
    }

    fn state(&self, id: &str) -> ActionState {
        self.states
            .iter()
            .find(|(other, _)| other == id)
            .map_or_else(ActionState::default, |(_, s)| *s)
    }

    /// `isActive`: held.
    #[must_use]
    pub fn is_active(&self, id: &str) -> bool {
        self.state(id).active
    }

    /// `wasPressed`: this frame.
    #[must_use]
    pub fn was_pressed(&self, id: &str) -> bool {
        self.state(id).pressed
    }

    /// `wasReleased`: this frame.
    #[must_use]
    pub fn was_released(&self, id: &str) -> bool {
        self.state(id).released
    }

    /// `getScrollDelta`: positive scrolls down.
    #[must_use]
    pub fn scroll_delta(&self) -> f64 {
        self.raw.wheel
    }

    // Raw input (`onKeyDown`, `onKeyUp`, `onMouseDown`, `onMouseUp`, `onWheel`).

    /// A key went down; repeats are ignored.
    pub fn key_down(&mut self, code: &str) {
        if self.raw.keys.insert(code.to_owned()) {
            self.raw.keys_pressed.insert(code.to_owned());
        }
    }

    /// A key came up.
    pub fn key_up(&mut self, code: &str) {
        if self.raw.keys.remove(code) {
            self.raw.keys_released.insert(code.to_owned());
        }
    }

    /// A mouse button went down.
    pub fn mouse_down(&mut self, button: MouseButton) {
        if self.raw.buttons.insert(button) {
            self.raw.buttons_pressed.insert(button);
        }
    }

    /// A mouse button came up.
    pub fn mouse_up(&mut self, button: MouseButton) {
        if self.raw.buttons.remove(&button) {
            self.raw.buttons_released.insert(button);
        }
    }

    /// The wheel turned; `delta_y` is positive scrolling down (`WheelEvent.deltaY`).
    pub fn wheel(&mut self, delta_y: f64) {
        self.raw.wheel += delta_y;
    }

    /// Whether `code` is held, whatever it is bound to.
    #[must_use]
    pub fn key_held(&self, code: &str) -> bool {
        self.raw.keys.contains(code)
    }

    /// `setVirtualAction`: `source` holds the action, or lets go of it. It
    /// reads like a held key: pressed once, active while any source holds
    /// it, released once.
    pub fn set_virtual_action(&mut self, id: &str, held: bool, source: &str) {
        let sources = self.virtual_sources.entry(id.to_owned()).or_default();
        if held {
            sources.insert(source.to_owned());
        } else {
            sources.remove(source);
        }
        let held = !sources.is_empty();
        if !held {
            self.virtual_sources.remove(id);
        }
        if held == self.virtual_held.contains(id) {
            return;
        }
        if held {
            self.virtual_held.insert(id.to_owned());
            self.virtual_pressed.insert(id.to_owned());
        } else {
            self.virtual_held.remove(id);
            self.virtual_released.insert(id.to_owned());
        }
    }

    /// `releaseHeldInput`: the window lost focus, or a text field took the keys.
    pub fn release_held_input(&mut self) {
        self.virtual_held.clear();
        self.virtual_sources.clear();
        self.virtual_pressed.clear();
        let keys: Vec<String> = self.raw.keys.drain().collect();
        self.raw.keys_released.extend(keys);
        self.raw.keys_pressed.clear();
        let buttons: Vec<MouseButton> = self.raw.buttons.drain().collect();
        self.raw.buttons_released.extend(buttons);
        self.raw.buttons_pressed.clear();
        self.raw.wheel = 0.0;
        for (id, state) in &mut self.states {
            if state.active {
                self.pending_releases.insert(id.clone());
            }
            *state = ActionState {
                active: false,
                pressed: false,
                released: self.pending_releases.contains(id),
            };
        }
    }

    fn modifiers_held(&self, binding: &InputBinding) -> bool {
        let held = |a: &str, b: &str| self.raw.keys.contains(a) || self.raw.keys.contains(b);
        binding.modifiers.iter().flatten().all(|(m, required)| {
            !*required
                || match m {
                    Modifier::Ctrl => held("ControlLeft", "ControlRight"),
                    Modifier::Shift => held("ShiftLeft", "ShiftRight"),
                    Modifier::Alt => held("AltLeft", "AltRight"),
                }
        })
    }

    /// `updateActionStates`: each action of the active context from the raw state.
    pub fn update(&mut self) {
        let Some(context) = self.context(self.active_context()) else {
            return;
        };
        let mut next = Vec::new();
        for id in &context.actions {
            if self.action(id).is_none() {
                continue;
            }
            let bindings = self.bindings_in(context, id);
            let (mut active, mut pressed, mut released) = (false, false, false);
            for binding in bindings {
                if !self.modifiers_held(binding) {
                    continue;
                }
                if let Some(key) = &binding.key {
                    active |= self.raw.keys.contains(key);
                    pressed |= self.raw.keys_pressed.contains(key);
                    released |= self.raw.keys_released.contains(key);
                }
                if let Some(button) = binding.mouse_button {
                    active |= self.raw.buttons.contains(&button);
                    pressed |= self.raw.buttons_pressed.contains(&button);
                    released |= self.raw.buttons_released.contains(&button);
                }
                if let Some(direction) = binding.scroll_direction {
                    let wheel = self.raw.wheel;
                    if (direction == ScrollDirection::Down && wheel > 0.0)
                        || (direction == ScrollDirection::Up && wheel < 0.0)
                    {
                        active = true;
                        pressed = true;
                    }
                }
            }
            active |= self.virtual_held.contains(id);
            pressed |= self.virtual_pressed.contains(id);
            released |= self.virtual_released.contains(id);
            let released = !active && (released || self.pending_releases.contains(id));
            next.push((
                id.clone(),
                ActionState {
                    active,
                    pressed,
                    released,
                },
            ));
        }
        for (id, state) in next {
            if let Some((_, s)) = self.states.iter_mut().find(|(other, _)| *other == id) {
                *s = state;
            }
        }
    }

    /// `clearPerFrameState`.
    pub fn end_frame(&mut self) {
        self.raw.keys_pressed.clear();
        self.raw.keys_released.clear();
        self.raw.buttons_pressed.clear();
        self.raw.buttons_released.clear();
        self.raw.wheel = 0.0;
        self.virtual_pressed.clear();
        self.virtual_released.clear();
        self.pending_releases.clear();
        for (_, state) in &mut self.states {
            state.pressed = false;
            state.released = false;
        }
    }

    // Binding queries and rebinding.

    fn override_for(&self, id: &str) -> Option<&Vec<InputBinding>> {
        self.overrides.iter().find(|(other, _)| other == id).map(|(_, b)| b)
    }

    fn bindings_in<'a>(&'a self, context: &'a ContextDefinition, id: &str) -> &'a [InputBinding] {
        if let Some(b) = self.override_for(id) {
            return b;
        }
        if let Some((_, b)) = context.bindings.iter().find(|(other, _)| other == id) {
            return b;
        }
        self.action(id).map_or(&[], |a| &a.default_bindings)
    }

    /// `getBindingsForAction`: overrides, else the defaults.
    #[must_use]
    pub fn bindings_for_action(&self, id: &str) -> &[InputBinding] {
        match self.override_for(id) {
            Some(b) => b,
            None => self.default_bindings(id),
        }
    }

    /// `getDefaultBindings`: the active context's, else the action's own.
    #[must_use]
    pub fn default_bindings(&self, id: &str) -> &[InputBinding] {
        if let Some(context) = self.context(self.active_context())
            && let Some((_, b)) = context.bindings.iter().find(|(other, _)| other == id)
        {
            return b;
        }
        self.action(id).map_or(&[], |a| &a.default_bindings)
    }

    fn set_override(&mut self, id: &str, bindings: Vec<InputBinding>) {
        if let Some((_, b)) = self.overrides.iter_mut().find(|(other, _)| other == id) {
            *b = bindings;
        } else {
            self.overrides.push((id.to_owned(), bindings));
        }
    }

    /// `rebind`, saved.
    pub fn rebind(&mut self, id: &str, bindings: &[InputBinding]) {
        let value = serde_json::json!({ id: bindings });
        let Some((_, valid)) = validated_binding_overrides(&value).into_iter().next() else {
            return;
        };
        self.set_override(id, valid);
        self.save_bindings_to_storage();
    }

    /// `resetBinding`: back to the defaults, saved.
    pub fn reset_binding(&mut self, id: &str) {
        self.overrides.retain(|(other, _)| other != id);
        self.save_bindings_to_storage();
    }

    /// `resetAllBindings`.
    pub fn reset_all_bindings(&mut self) {
        self.overrides.clear();
        self.save_bindings_to_storage();
    }

    /// `loadBindingsFromStorage`: corrupt data is ignored.
    pub fn load_bindings_from_storage(&mut self) {
        let Some(dir) = &self.storage else { return };
        let Some(saved) = load_synced_storage(dir).remove(BINDINGS_KEY) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(&saved) else {
            return;
        };
        for (id, bindings) in validated_binding_overrides(&value) {
            self.set_override(&id, bindings);
        }
    }

    /// `reloadBindingsFromStorage`: after a synced settings download.
    pub fn reload_bindings_from_storage(&mut self) {
        self.overrides.clear();
        self.load_bindings_from_storage();
    }

    fn save_bindings_to_storage(&self) {
        if let Some(dir) = &self.storage {
            save_synced_storage_entry(dir, BINDINGS_KEY, Some(&self.bindings_json()));
        }
    }

    /// `JSON.stringify(exportBindings())`, actions in the order they were bound.
    #[must_use]
    pub fn bindings_json(&self) -> String {
        let entries: Vec<String> = self
            .overrides
            .iter()
            .map(|(id, b)| format!("{}:{}", json_string(id), serde_json::to_string(b).unwrap_or_default()))
            .collect();
        format!("{{{}}}", entries.join(","))
    }

    /// `exportBindings`: the overrides as JSON (keys sorted; see [`Self::bindings_json`]).
    #[must_use]
    pub fn export_bindings(&self) -> Value {
        Value::Object(
            self.overrides
                .iter()
                .map(|(id, b)| (id.clone(), serde_json::to_value(b).unwrap_or_default()))
                .collect(),
        )
    }

    /// `importBindings`: replaces every override, saved.
    pub fn import_bindings(&mut self, data: &Value) {
        self.overrides = validated_binding_overrides(data);
        self.save_bindings_to_storage();
    }
}

thread_local! {
    /// `InputSystem.getInstance()`: one per thread, as the browser has one per page.
    static INPUT: RefCell<InputSystem> = RefCell::new(InputSystem::default());
}

/// `InputSystem.getInstance().init(..)`: a fresh system saving in `storage`,
/// keeping what was registered, with the saved bindings loaded.
pub fn init_input(storage: Option<PathBuf>) {
    INPUT.with(|cell| {
        let mut input = cell.borrow_mut();
        input.storage = storage;
        input.reload_bindings_from_storage();
    });
}

/// The thread's input system. Do not call it from inside itself.
pub fn with_input<R>(f: impl FnOnce(&mut InputSystem) -> R) -> R {
    INPUT.with(|cell| f(&mut cell.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(dir: Option<PathBuf>) -> InputSystem {
        let mut input = InputSystem::new(dir);
        input.register_action(ActionDefinition::button(
            "jump",
            "Jump",
            vec![InputBinding::key("Space")],
        ));
        input.register_action(ActionDefinition::button(
            "fire",
            "Fire",
            vec![InputBinding::mouse(MouseButton::Left)],
        ));
        input.register_action(ActionDefinition::button(
            "next",
            "Next",
            vec![InputBinding::scroll(ScrollDirection::Down)],
        ));
        input.register_context(ContextDefinition {
            id: "play".to_owned(),
            blocking: false,
            actions: vec!["jump".to_owned(), "fire".to_owned(), "next".to_owned()],
            bindings: Vec::new(),
        });
        input.set_context("play");
        input
    }

    #[test]
    fn actions_follow_their_bindings_through_a_frame() {
        let mut input = system(None);
        input.key_down("Space");
        input.mouse_down(MouseButton::Left);
        input.wheel(120.0);
        input.update();
        assert!(input.is_active("jump") && input.was_pressed("jump"));
        assert!(input.is_active("fire"));
        assert!(input.was_pressed("next"));
        input.end_frame();
        input.update();
        assert!(input.is_active("jump") && !input.was_pressed("jump"));
        assert!(!input.is_active("next"));
        input.key_up("Space");
        input.update();
        assert!(input.was_released("jump") && !input.is_active("jump"));
        input.end_frame();
        input.release_held_input();
        assert!(input.was_released("fire"));
        input.update();
        assert!(input.was_released("fire"));
    }

    #[test]
    fn rebinding_saves_and_loads_the_overrides() {
        let dir = std::env::temp_dir().join(format!("shagger-input-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut input = system(Some(dir.clone()));
        input.rebind("jump", &[InputBinding::key("KeyX")]);
        input.rebind("fire", &[]);
        input.key_down("Space");
        input.key_down("KeyX");
        input.mouse_down(MouseButton::Left);
        input.update();
        assert!(input.is_active("jump"));
        assert!(!input.is_active("fire"));
        let saved = load_synced_storage(&dir).remove(BINDINGS_KEY).unwrap();
        assert_eq!(saved, r#"{"jump":[{"key":"KeyX"}],"fire":[]}"#);

        let mut other = system(Some(dir.clone()));
        other.load_bindings_from_storage();
        assert_eq!(other.bindings_for_action("jump"), [InputBinding::key("KeyX")]);
        other.reset_binding("jump");
        assert_eq!(other.bindings_for_action("jump"), [InputBinding::key("Space")]);
        other.reset_all_bindings();
        assert_eq!(load_synced_storage(&dir).remove(BINDINGS_KEY).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One frame: (active, pressed, released) for `jump`.
    fn frame(input: &mut InputSystem) -> (bool, bool, bool) {
        input.update();
        let state = (
            input.is_active("jump"),
            input.was_pressed("jump"),
            input.was_released("jump"),
        );
        input.end_frame();
        state
    }

    // `VirtualActions.test.ts`.
    #[test]
    fn a_virtual_release_waits_for_a_physical_binding_still_held() {
        let mut input = system(None);
        input.key_down("Space");
        input.set_virtual_action("jump", true, "gamepad");
        frame(&mut input);
        input.set_virtual_action("jump", false, "gamepad");
        assert_eq!(frame(&mut input), (true, false, false));
        input.key_up("Space");
        assert_eq!(frame(&mut input), (false, false, true));
    }

    #[test]
    fn a_physical_release_waits_for_a_virtual_source_still_holding() {
        let mut input = system(None);
        input.key_down("Space");
        input.set_virtual_action("jump", true, "gamepad");
        frame(&mut input);
        input.key_up("Space");
        assert_eq!(frame(&mut input), (true, false, false));
        input.set_virtual_action("jump", false, "gamepad");
        assert!(frame(&mut input).2);
    }

    #[test]
    fn an_action_held_by_another_source_is_not_released() {
        let mut input = system(None);
        input.set_virtual_action("jump", true, "gamepad");
        input.set_virtual_action("jump", true, "touch");
        frame(&mut input);
        input.set_virtual_action("jump", false, "gamepad");
        assert_eq!(frame(&mut input), (true, false, false));
        input.set_virtual_action("jump", false, "touch");
        assert!(frame(&mut input).2);
    }

    #[test]
    fn virtual_actions_read_like_a_held_key() {
        let mut input = system(None);
        input.set_virtual_action("jump", true, "touch");
        assert_eq!(frame(&mut input), (true, true, false));
        input.set_virtual_action("jump", true, "touch");
        assert_eq!(frame(&mut input), (true, false, false));
        input.set_virtual_action("jump", false, "touch");
        assert_eq!(frame(&mut input), (false, false, true));
        assert_eq!(frame(&mut input), (false, false, false));
        // A press and release within one frame is a tap.
        input.set_virtual_action("jump", true, "touch");
        input.set_virtual_action("jump", false, "touch");
        assert_eq!(frame(&mut input), (false, true, true));
        // Leaving play lets go.
        input.set_virtual_action("jump", true, "touch");
        frame(&mut input);
        input.release_held_input();
        assert!(!frame(&mut input).0);
    }
}
