//! SHAG `src/engine/input/BindingValidation.ts`: saved bindings, checked.

use serde_json::Value;

use super::input_types::{InputBinding, Modifier, MouseButton, ScrollDirection};

fn binding(value: &Value) -> Option<InputBinding> {
    let object = value.as_object()?;
    let mut result = InputBinding::default();
    let mut any = false;
    if let Some(key) = object.get("key") {
        let key = key.as_str().filter(|k| !k.trim().is_empty())?;
        result.key = Some(key.to_owned());
        any = true;
    }
    if let Some(button) = object.get("mouseButton") {
        result.mouse_button = Some(MouseButton::parse(button.as_str()?)?);
        any = true;
    }
    if let Some(direction) = object.get("scrollDirection") {
        result.scroll_direction = Some(ScrollDirection::parse(direction.as_str()?)?);
        any = true;
    }
    if let Some(gesture) = object.get("gesture") {
        let gesture = gesture.as_str().filter(|g| !g.trim().is_empty())?;
        result.gesture = Some(gesture.to_owned());
        any = true;
    }
    if let Some(button) = object.get("gamepadButton") {
        result.gamepad_button = Some(button.as_u64()?);
        any = true;
    }
    if !any {
        return None;
    }
    if let Some(modifiers) = object.get("modifiers") {
        let mut list = Vec::new();
        for (key, required) in modifiers.as_object()? {
            let modifier = match key.as_str() {
                "ctrl" => Modifier::Ctrl,
                "shift" => Modifier::Shift,
                "alt" => Modifier::Alt,
                _ => return None,
            };
            list.retain(|(m, _)| *m != modifier);
            list.push((modifier, required.as_bool()?));
        }
        result.modifiers = Some(list);
    }
    Some(result)
}

/// `validatedBindingOverrides`: invalid overrides fall back to defaults;
/// explicit empty arrays remain unbound. In the saved object's order.
#[must_use]
pub fn validated_binding_overrides(value: &Value) -> Vec<(String, Vec<InputBinding>)> {
    let mut overrides: Vec<(String, Vec<InputBinding>)> = Vec::new();
    let Some(object) = value.as_object() else {
        return overrides;
    };
    for (id, bindings) in object {
        let Some(bindings) = bindings.as_array().filter(|_| !id.is_empty()) else {
            continue;
        };
        let valid: Vec<InputBinding> = bindings.iter().filter_map(binding).collect();
        if bindings.is_empty() || !valid.is_empty() {
            overrides.retain(|(other, _)| other != id);
            overrides.push((id.clone(), valid));
        }
    }
    overrides
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invalid_entries_fall_back_and_empty_arrays_stay_unbound() {
        let overrides = validated_binding_overrides(&json!({
            "jump": [{ "key": "KeyX" }, { "key": "  " }, { "mouseButton": "back" }],
            "fire": [],
            "aim": [{ "nope": 1 }],
            "use": "KeyE",
            "crouch": [{ "key": "KeyC", "modifiers": { "shift": true } }],
        }));
        let ids: Vec<&str> = overrides.iter().map(|(id, _)| id.as_str()).collect();
        // serde_json orders object keys by name.
        assert_eq!(ids, ["crouch", "fire", "jump"]);
        assert_eq!(overrides[2].1, vec![InputBinding::key("KeyX")]);
        assert!(overrides[1].1.is_empty());
        assert_eq!(
            serde_json::to_string(&overrides[0].1).unwrap(),
            r#"[{"key":"KeyC","modifiers":{"shift":true}}]"#
        );
        assert!(validated_binding_overrides(&json!([1])).is_empty());
    }

    #[test]
    fn a_bad_modifier_drops_the_binding() {
        let overrides = validated_binding_overrides(&json!({
            "a": [{ "key": "KeyA", "modifiers": { "meta": true } }],
            "b": [{ "key": "KeyB", "modifiers": { "ctrl": 1 } }],
        }));
        assert!(overrides.is_empty());
    }
}
