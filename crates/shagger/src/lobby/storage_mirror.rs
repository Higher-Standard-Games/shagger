//! The lobby page's local storage, mirrored to the native files
//! (wash-fps-rust docs/research/WEBVIEW_LOBBY.md, sections 7 and 8).
//!
//! When a client shows the web lobby in a webview, its files stay the store
//! and the page's local storage is a mirror of the keys both sides use. This
//! is the native half: [`StorageMirror::snapshot`] gives the page the keys at
//! start (the init script's `storage`), and [`StorageMirror::apply`] takes
//! each `storage:set` the page sends, writes the file and brings the live
//! cvars and bindings up to date.
//!
//! | Local storage key | Native file |
//! |---|---|
//! | `engine_cvars` | the cvar registry's file (`cvars.json`) |
//! | `engine_binds`, `shag_keybindings`, `hud-user-prefs`, `ui_theme_mode`, `shag.loadout.*` | `synced-storage.json` |
//! | `shag_settings_sync`, `shag_settings_backup` | `settings-sync.json`, `settings-backup.json` |
//! | `shag_player_key`, `shag_player_key_unknown` | the key file and `<key file>.unknown` |
//!
//! Not a port: the browser has no second store.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use shagger_shared::settings::synced_settings::is_synced_storage_key;

use crate::core::cvar::with_cvars;
use crate::input::input_system::with_input;
use crate::lobby::settings_sync::{load_synced_storage, save_synced_storage_entry};

/// `Cvar.ts`'s `CVAR_STORAGE_KEY`.
pub const CVARS_KEY: &str = "engine_cvars";
/// `SettingsSync.ts`'s `SYNC_STATE_STORAGE_KEY`.
pub const SYNC_STATE_KEY: &str = "shag_settings_sync";
/// `SettingsSync.ts`'s `BACKUP_STORAGE_KEY`.
pub const BACKUP_KEY: &str = "shag_settings_backup";
/// `AccountSession.ts`'s `KEY_STORAGE`.
pub const PLAYER_KEY: &str = "shag_player_key";
/// `AccountSession.ts`'s `UNKNOWN_KEY_STORAGE`.
pub const UNKNOWN_PLAYER_KEY: &str = "shag_player_key_unknown";

/// The bindings entries: a change reloads the live bindings.
const BINDING_KEYS: [&str; 2] = ["engine_binds", "shag_keybindings"];

/// Whether the page's `key` is mirrored to a native file.
#[must_use]
pub fn is_mirrored(key: &str) -> bool {
    matches!(
        key,
        CVARS_KEY | SYNC_STATE_KEY | BACKUP_KEY | PLAYER_KEY | UNKNOWN_PLAYER_KEY
    ) || is_synced_storage_key(key)
}

/// The native files behind the mirrored keys.
#[derive(Debug, Clone)]
pub struct StorageMirror {
    /// The config directory: `synced-storage.json` and the sync files.
    dir: PathBuf,
    /// The account's key (`lobby-key`); `None` keeps the key in the page only.
    key_file: Option<PathBuf>,
}

impl StorageMirror {
    #[must_use]
    pub fn new(dir: PathBuf, key_file: Option<PathBuf>) -> Self {
        Self { dir, key_file }
    }

    fn file_for(&self, key: &str) -> Option<PathBuf> {
        match key {
            SYNC_STATE_KEY => Some(self.dir.join("settings-sync.json")),
            BACKUP_KEY => Some(self.dir.join("settings-backup.json")),
            PLAYER_KEY => self.key_file.clone(),
            UNKNOWN_PLAYER_KEY => self.key_file.as_deref().map(unknown_key_file),
            _ => None,
        }
    }

    /// Every mirrored key the native side has, as local storage strings.
    #[must_use]
    pub fn snapshot(&self) -> Map<String, Value> {
        let mut out = Map::new();
        let cvars = with_cvars(|c| c.persisted());
        if !cvars.is_empty() {
            out.insert(CVARS_KEY.to_owned(), Value::String(Value::Object(cvars).to_string()));
        }
        for (key, value) in load_synced_storage(&self.dir) {
            if is_synced_storage_key(&key) {
                out.insert(key, Value::String(value));
            }
        }
        for key in [SYNC_STATE_KEY, BACKUP_KEY, PLAYER_KEY, UNKNOWN_PLAYER_KEY] {
            let Some(file) = self.file_for(key) else { continue };
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let text = if key == PLAYER_KEY || key == UNKNOWN_PLAYER_KEY {
                text.trim().to_owned()
            } else {
                text
            };
            if !text.is_empty() {
                out.insert(key.to_owned(), Value::String(text));
            }
        }
        out
    }

    /// The page wrote `key` (`None`: removed it). Unmirrored keys and
    /// malformed cvars are refused (false); the rest land in their files and
    /// in the live registries.
    pub fn apply(&self, key: &str, value: Option<&str>) -> bool {
        if key == CVARS_KEY {
            let values = match value {
                None => Map::new(),
                Some(text) => match serde_json::from_str::<Value>(text) {
                    Ok(Value::Object(map)) => map,
                    _ => return false,
                },
            };
            // The page holds the whole persisted set, native-only names
            // included (`Cvar.loadFromStorage` keeps unknown ones).
            with_cvars(|c| c.replace_persisted_values(&values, |_| true));
            return true;
        }
        if is_synced_storage_key(key) {
            save_synced_storage_entry(&self.dir, key, value);
            if BINDING_KEYS.contains(&key) {
                with_input(|input| input.reload_bindings_from_storage());
            }
            return true;
        }
        let Some(file) = self.file_for(key) else {
            return false;
        };
        let written = match value {
            Some(text) => crate::core::atomic_file::write_atomic(&file, text.as_bytes()),
            None => match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        };
        written.is_ok()
    }

    /// The mirrored keys whose value differs from `before`, a removed one as
    /// `null`: `storage:changed`'s entries after a match.
    #[must_use]
    pub fn changed_since(&self, before: &Map<String, Value>) -> Map<String, Value> {
        let now = self.snapshot();
        let mut out = Map::new();
        for (key, value) in &now {
            if before.get(key) != Some(value) {
                out.insert(key.clone(), value.clone());
            }
        }
        for key in before.keys() {
            if !now.contains_key(key) {
                out.insert(key.clone(), Value::Null);
            }
        }
        out
    }
}

/// `<key file>.unknown`, where the native lobby sets a refused key aside.
fn unknown_key_file(key_file: &Path) -> PathBuf {
    let mut name = key_file.file_name().unwrap_or_default().to_os_string();
    name.push(".unknown");
    key_file.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cvar::{CvarOptions, init_cvars};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shagger-mirror-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_the_shared_keys_are_mirrored() {
        for key in [
            "engine_cvars",
            "engine_binds",
            "shag_keybindings",
            "hud-user-prefs",
            "ui_theme_mode",
            "shag.loadout.rocket",
            "shag_settings_sync",
            "shag_settings_backup",
            "shag_player_key",
            "shag_player_key_unknown",
        ] {
            assert!(is_mirrored(key), "{key}");
        }
        for key in [
            "washoi.welcome.hideAutomatically",
            "engine.renderVars",
            "pk3_index",
            "quality_preference",
        ] {
            assert!(!is_mirrored(key), "{key}");
        }
    }

    #[test]
    fn keys_and_sync_files_round_trip() {
        let dir = dir("files");
        let key_file = dir.join("lobby-key");
        std::fs::write(&key_file, "ABCD-EFGH\n").unwrap();
        let mirror = StorageMirror::new(dir.clone(), Some(key_file.clone()));
        let snap = mirror.snapshot();
        assert_eq!(snap["shag_player_key"], "ABCD-EFGH");

        assert!(mirror.apply("shag_player_key", Some("NEW-KEY")));
        assert_eq!(std::fs::read_to_string(&key_file).unwrap(), "NEW-KEY");
        assert!(mirror.apply("shag_player_key_unknown", Some("OLD")));
        assert_eq!(std::fs::read_to_string(dir.join("lobby-key.unknown")).unwrap(), "OLD");
        assert!(mirror.apply("shag_settings_sync", Some(r#"{"accountId":"1"}"#)));
        assert_eq!(
            std::fs::read_to_string(dir.join("settings-sync.json")).unwrap(),
            r#"{"accountId":"1"}"#
        );
        assert!(mirror.apply("shag.loadout.rocket", Some("red")));
        assert_eq!(load_synced_storage(&dir)["shag.loadout.rocket"], "red");

        let changed = mirror.changed_since(&snap);
        assert_eq!(changed["shag_player_key"], "NEW-KEY");
        assert_eq!(changed["shag.loadout.rocket"], "red");

        assert!(mirror.apply("shag_player_key", None));
        assert!(!key_file.exists());
        assert!(mirror.apply("shag_player_key", None), "removing twice is fine");
        assert_eq!(mirror.changed_since(&snap)["shag_player_key"], Value::Null);
        assert!(!mirror.apply("washoi.welcome.hideAutomatically", Some("true")));
    }

    #[test]
    fn the_page_cvars_become_the_live_ones() {
        let dir = dir("cvars");
        init_cvars(Some(dir.join("cvars.json")), false);
        with_cvars(|c| {
            c.register("cl_fov", 90, CvarOptions::default());
            c.set("cl_fov", 100);
        });
        let mirror = StorageMirror::new(dir, None);
        assert_eq!(mirror.snapshot()["engine_cvars"], r#"{"cl_fov":100}"#);
        assert!(mirror.apply("engine_cvars", Some(r#"{"cl_fov":110,"r_webonly":1}"#)));
        assert!((with_cvars(|c| c.get_number("cl_fov")) - 110.0).abs() < 1e-9);
        assert!(with_cvars(|c| c.exists("r_webonly")), "kept for the page");
        assert!(!mirror.apply("engine_cvars", Some("[1]")));
        assert!(mirror.apply("engine_cvars", None));
        assert!((with_cvars(|c| c.get_number("cl_fov")) - 90.0).abs() < 1e-9);
    }
}
