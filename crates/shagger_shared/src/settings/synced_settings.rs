//! SHAG `src/shared/settings/SyncedSettings.ts`: which settings follow the
//! player's account, and the blob the lobby keeps for them. Key and command
//! bindings, game preferences (the `cl_`, `cg_`, `s_` and `m_` cvars and
//! colour correction), the HUD layout, the theme and the remembered loadouts
//! sync; graphics and anything tied to this device stay.
//!
//! The blob is compared as text, so [`serialize_synced_settings`] writes it
//! exactly as `JSON.stringify` does with its keys sorted.

use std::collections::BTreeMap;

use crate::js::js_number_to_string;
use crate::json::{self, Value};

/// Largest blob the lobby stores.
pub const MAX_SETTINGS_BYTES: usize = 32 * 1024;
pub const SYNCED_SETTINGS_VERSION: f64 = 1.0;
/// The browser's storage entry for persisted cvars.
pub const CVAR_STORAGE_KEY: &str = "engine_cvars";
/// Storage entries copied whole.
pub const SYNCED_STORAGE_KEYS: [&str; 4] = ["shag_keybindings", "engine_binds", "hud-user-prefs", "ui_theme_mode"];
pub const LOADOUT_KEY_PREFIX: &str = "shag.loadout.";

const SYNCED_CVAR_PREFIXES: [&str; 4] = ["cl_", "cg_", "s_", "m_"];
const DEVICE_CVARS: [&str; 8] = [
    "cl_name",
    "cl_interp",
    "cl_timenudge",
    "cl_netgraph",
    "cl_player_pose_hz",
    "cl_player_outline_proxy",
    "cl_animdebug",
    "cl_remotehitbox",
];
const PLAYER_RENDER_CVARS: [&str; 2] = ["r_colorcorrection", "r_colorcorrection_strength"];

/// `isSyncedCvar`.
#[must_use]
pub fn is_synced_cvar(name: &str) -> bool {
    let lower = name.to_lowercase();
    if PLAYER_RENDER_CVARS.contains(&lower.as_str()) {
        return true;
    }
    if DEVICE_CVARS.contains(&lower.as_str()) || lower.starts_with("cl_debug") {
        return false;
    }
    SYNCED_CVAR_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// `isSyncedStorageKey`.
#[must_use]
pub fn is_synced_storage_key(key: &str) -> bool {
    SYNCED_STORAGE_KEYS.contains(&key) || key.starts_with(LOADOUT_KEY_PREFIX)
}

/// `SyncedCvarValue`.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncedCvarValue {
    String(String),
    Number(f64),
    Bool(bool),
}

/// `SyncedSettingsBlob`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncedSettingsBlob {
    pub v: f64,
    pub cvars: BTreeMap<String, SyncedCvarValue>,
    pub storage: BTreeMap<String, String>,
}

impl SyncedSettingsBlob {
    #[must_use]
    pub fn new() -> Self {
        Self {
            v: SYNCED_SETTINGS_VERSION,
            ..Self::default()
        }
    }
}

/// `parseSyncedSettings`: `None` when it is not a blob this client reads.
#[must_use]
pub fn parse_synced_settings(text: &str) -> Option<SyncedSettingsBlob> {
    let data = json::parse(text).ok()?;
    let Value::Object(_) = &data else { return None };
    if data.get("v").and_then(Value::as_f64) != Some(SYNCED_SETTINGS_VERSION) {
        return None;
    }
    let mut blob = SyncedSettingsBlob::new();
    if let Some(Value::Object(cvars)) = data.get("cvars") {
        for (name, value) in cvars {
            let value = match value {
                Value::Number(n) if n.is_finite() => SyncedCvarValue::Number(*n),
                Value::String(s) => SyncedCvarValue::String(s.clone()),
                Value::Bool(b) => SyncedCvarValue::Bool(*b),
                _ => continue,
            };
            if is_synced_cvar(name) {
                blob.cvars.insert(name.clone(), value);
            }
        }
    }
    if let Some(Value::Object(storage)) = data.get("storage") {
        for (key, value) in storage {
            if let Value::String(s) = value
                && is_synced_storage_key(key)
            {
                blob.storage.insert(key.clone(), s.clone());
            }
        }
    }
    Some(blob)
}

/// `JSON.stringify` of a string.
#[must_use]
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// JavaScript's `<` on strings: UTF-16 code units.
fn js_order<T>(record: &BTreeMap<String, T>) -> Vec<(&String, &T)> {
    let mut entries: Vec<_> = record.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    entries
}

/// `serializeSyncedSettings`.
#[must_use]
pub fn serialize_synced_settings(blob: &SyncedSettingsBlob) -> String {
    let cvars: Vec<String> = js_order(&blob.cvars)
        .into_iter()
        .map(|(k, v)| {
            let value = match v {
                SyncedCvarValue::String(s) => json_string(s),
                SyncedCvarValue::Number(n) => js_number_to_string(*n),
                SyncedCvarValue::Bool(b) => b.to_string(),
            };
            format!("{}:{value}", json_string(k))
        })
        .collect();
    let storage: Vec<String> = js_order(&blob.storage)
        .into_iter()
        .map(|(k, v)| format!("{}:{}", json_string(k), json_string(v)))
        .collect();
    format!(
        "{{\"v\":{},\"cvars\":{{{}}},\"storage\":{{{}}}}}",
        js_number_to_string(blob.v),
        cvars.join(","),
        storage.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_settings_stay() {
        assert!(is_synced_cvar("m_sensitivity"));
        assert!(is_synced_cvar("CG_crosshairSize"));
        assert!(is_synced_cvar("r_colorcorrection"));
        assert!(!is_synced_cvar("r_bloom"));
        assert!(!is_synced_cvar("cl_netgraph"));
        assert!(!is_synced_cvar("cl_debugbbox"));
        assert!(is_synced_storage_key("shag.loadout.amphi"));
    }

    #[test]
    fn parses_filters_and_serializes_sorted() {
        let blob = parse_synced_settings(
            r#"{"v":1,"cvars":{"s_volume":0.3,"r_bloom":0,"cl_fov":105,"cg_crosshaircolor":"FF\"00","x":[]},"storage":{"engine_binds":"{}","other":"x"}}"#,
        )
        .unwrap();
        assert_eq!(
            serialize_synced_settings(&blob),
            r#"{"v":1,"cvars":{"cg_crosshaircolor":"FF\"00","cl_fov":105,"s_volume":0.3},"storage":{"engine_binds":"{}"}}"#
        );
        assert!(parse_synced_settings(r#"{"v":2}"#).is_none());
    }
}
