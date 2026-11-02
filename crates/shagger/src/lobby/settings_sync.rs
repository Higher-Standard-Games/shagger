//! SHAG `src/lobby/SettingsSync.ts`: keeps this machine's synced settings
//! in step with the logged-in account. On login the account's copy applies
//! unless this machine changed its own since the last sync and the
//! account's is older; an account with none takes this machine's. While
//! logged in, a change uploads two seconds after it settles (checked every
//! five), and [`SettingsSync::flush`] uploads what is left on the way out.
//!
//! The browser keeps its side in local storage; here the sync state, the
//! backup and the synced storage entries (bindings, loadouts) are files in
//! the config directory. The browser's promise chain is a state machine
//! driven by the lobby's messages and [`SettingsSync::tick`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use shagger_shared::settings::synced_settings::{
    MAX_SETTINGS_BYTES, SyncedCvarValue, SyncedSettingsBlob, is_synced_cvar, is_synced_storage_key,
    parse_synced_settings, serialize_synced_settings,
};

use crate::core::cvar::{CvarValue, Cvars};
use crate::lobby::lobby_client::LobbyClient;
use crate::lobby::types::{LobbyIn, Welcome};

const CHECK_INTERVAL: Duration = Duration::from_secs(5);
const UPLOAD_DELAY: Duration = Duration::from_secs(2);

/// `SyncState`.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncState {
    pub account_id: String,
    pub synced: String,
    pub synced_at: f64,
    pub edited_at: Option<f64>,
}

/// `SyncDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDecision {
    Apply,
    Upload,
    None,
    Keep,
}

/// `decideSync`.
#[must_use]
pub fn decide_sync(
    local: &str,
    state: Option<&SyncState>,
    account_id: &str,
    remote_settings: Option<&str>,
    remote_updated_at: Option<f64>,
) -> SyncDecision {
    let Some(remote) = remote_settings else {
        return SyncDecision::Upload;
    };
    let Some(parsed) = parse_synced_settings(remote) else {
        return SyncDecision::Keep;
    };
    if serialize_synced_settings(&parsed) == local {
        return SyncDecision::None;
    }
    let Some(state) = state.filter(|s| s.account_id == account_id) else {
        return SyncDecision::Apply;
    };
    if local == state.synced {
        return SyncDecision::Apply;
    }
    if remote_updated_at == Some(state.synced_at) {
        return SyncDecision::Upload;
    }
    if state.edited_at.unwrap_or(0.0) > remote_updated_at.unwrap_or(0.0) {
        SyncDecision::Upload
    } else {
        SyncDecision::Apply
    }
}

fn to_synced(v: &CvarValue) -> SyncedCvarValue {
    match v {
        CvarValue::Bool(b) => SyncedCvarValue::Bool(*b),
        CvarValue::Number(n) => SyncedCvarValue::Number(*n),
        CvarValue::String(s) => SyncedCvarValue::String(s.clone()),
    }
}

fn to_json(v: &SyncedCvarValue) -> Value {
    match v {
        SyncedCvarValue::Bool(b) => Value::Bool(*b),
        SyncedCvarValue::Number(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
        SyncedCvarValue::String(s) => Value::String(s.clone()),
    }
}

/// `collectSettings`: the stored cvars, then the live ones, and the synced
/// storage entries.
#[must_use]
pub fn collect_settings(cvars: &Cvars, storage: &BTreeMap<String, String>) -> SyncedSettingsBlob {
    let mut blob = SyncedSettingsBlob::new();
    for (name, value) in cvars.persisted() {
        if !is_synced_cvar(&name) {
            continue;
        }
        let value = match value {
            Value::Bool(b) => SyncedCvarValue::Bool(b),
            Value::Number(n) => SyncedCvarValue::Number(n.as_f64().unwrap_or(0.0)),
            Value::String(s) => SyncedCvarValue::String(s),
            _ => continue,
        };
        blob.cvars.insert(name, value);
    }
    for cvar in cvars.list(None) {
        if !cvar.flags.persist || !is_synced_cvar(&cvar.name) {
            continue;
        }
        if cvars.is_registered(&cvar.name) && cvar.value == cvar.default_value {
            blob.cvars.remove(&cvar.name);
        } else {
            blob.cvars.insert(cvar.name.clone(), to_synced(&cvar.value));
        }
    }
    for (key, value) in storage {
        if is_synced_storage_key(key) {
            blob.storage.insert(key.clone(), value.clone());
        }
    }
    blob
}

/// `applySettings`.
pub fn apply_settings(cvars: &mut Cvars, storage: &mut BTreeMap<String, String>, blob: &SyncedSettingsBlob) {
    let values: serde_json::Map<String, Value> = blob.cvars.iter().map(|(k, v)| (k.clone(), to_json(v))).collect();
    cvars.replace_persisted_values(&values, is_synced_cvar);
    storage.retain(|k, _| !is_synced_storage_key(k));
    storage.extend(blob.storage.iter().map(|(k, v)| (k.clone(), v.clone())));
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// The files that stand in for the browser's storage entries.
#[derive(Debug, Clone)]
struct Files {
    /// `shag_settings_sync`.
    state: PathBuf,
    /// `shag_settings_backup`.
    backup: PathBuf,
    /// The synced storage entries themselves.
    storage: PathBuf,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_text(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

/// The synced storage entries kept in `dir`.
#[must_use]
pub fn load_synced_storage(dir: &Path) -> BTreeMap<String, String> {
    read_json(&dir.join("synced-storage.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// One login's sync (`startSettingsSync` to `stopSettingsSync`).
#[derive(Debug)]
pub struct SettingsSync {
    account_id: String,
    files: Files,
    state: Option<SyncState>,
    last: String,
    ready: bool,
    stopped: bool,
    get_request: Option<String>,
    save_request: Option<(String, String)>,
    upload_at: Option<(Instant, String)>,
    next_check: Instant,
    warned_too_large: bool,
    /// The synced storage entries (bindings, loadouts) on this machine.
    pub storage: BTreeMap<String, String>,
}

impl SettingsSync {
    /// `startSettingsSync`: `None` for a lobby without synced settings or a
    /// login without an account.
    pub fn start(client: &mut LobbyClient, welcome: &Welcome, cvars: &Cvars, dir: &Path) -> Option<Self> {
        let account_id = welcome.identity.as_ref()?.id.clone();
        let synced = welcome
            .features
            .as_ref()
            .and_then(|f| f.get("syncedSettings"))
            .and_then(Value::as_bool);
        if synced != Some(true) {
            return None;
        }
        let files = Files {
            state: dir.join("settings-sync.json"),
            backup: dir.join("settings-backup.json"),
            storage: dir.join("synced-storage.json"),
        };
        let state = read_json(&files.state).and_then(|v| {
            Some(SyncState {
                account_id: v.get("accountId")?.as_str()?.to_owned(),
                synced: v.get("synced")?.as_str()?.to_owned(),
                synced_at: v.get("syncedAt")?.as_f64()?,
                edited_at: v.get("editedAt").and_then(Value::as_f64),
            })
        });
        let storage = load_synced_storage(dir);
        let last = serialize_synced_settings(&collect_settings(cvars, &storage));
        let get_request = Some(client.request_settings());
        Some(Self {
            account_id,
            files,
            state,
            last,
            ready: false,
            stopped: false,
            get_request,
            save_request: None,
            upload_at: None,
            next_check: Instant::now() + CHECK_INTERVAL,
            warned_too_large: false,
            storage,
        })
    }

    fn write_state(&self) {
        let Some(s) = &self.state else { return };
        // Millisecond times are whole: write them as `JSON.stringify` does.
        let ms = |t: f64| Value::from(t as i64);
        let mut v = serde_json::json!({ "accountId": s.account_id, "synced": s.synced, "syncedAt": ms(s.synced_at) });
        if let Some(e) = s.edited_at {
            v["editedAt"] = ms(e);
        }
        write_text(&self.files.state, &v.to_string());
    }

    fn upload(&mut self, client: &mut LobbyClient, text: String) {
        if self.stopped {
            return;
        }
        if text.len() > MAX_SETTINGS_BYTES {
            self.warned_too_large = true;
            return;
        }
        let id = client.save_settings(&text);
        self.save_request = Some((id, text));
    }

    fn fail_upload(&mut self) {
        let known = self
            .state
            .clone()
            .filter(|s| s.account_id == self.account_id)
            .unwrap_or(SyncState {
                account_id: self.account_id.clone(),
                synced: String::new(),
                synced_at: 0.0,
                edited_at: None,
            });
        self.state = Some(SyncState {
            edited_at: Some(now_ms()),
            ..known
        });
        self.write_state();
    }

    /// The lobby's answers.
    pub fn on_message(&mut self, message: &LobbyIn, client: &mut LobbyClient, cvars: &mut Cvars) {
        let LobbyIn::Other { kind, message } = message else {
            return;
        };
        let id = message.get("requestId").and_then(Value::as_str);
        match kind.as_str() {
            "settings:data" if id.is_some() && id == self.get_request.as_deref() => {
                self.get_request = None;
                let remote = message.get("settings").and_then(Value::as_str).map(str::to_owned);
                let updated_at = message.get("updatedAt").and_then(Value::as_f64);
                self.login_sync(client, cvars, remote.as_deref(), updated_at);
                self.ready = true;
            }
            "settings:saved" if id.is_some() && self.save_request.as_ref().map(|r| r.0.as_str()) == id => {
                let (_, text) = self.save_request.take().expect("matched above");
                let at = message.get("updatedAt").and_then(Value::as_f64).unwrap_or(0.0);
                self.state = Some(SyncState {
                    account_id: self.account_id.clone(),
                    synced: text,
                    synced_at: at,
                    edited_at: None,
                });
                self.write_state();
            }
            "settings:error" if id.is_some() => {
                if self.get_request.as_deref() == id {
                    // Offline or refused: keep this machine's own and try next login.
                    self.get_request = None;
                    self.ready = true;
                } else if self.save_request.as_ref().map(|r| r.0.as_str()) == id {
                    self.save_request = None;
                    self.fail_upload();
                }
            }
            _ => {}
        }
    }

    fn login_sync(
        &mut self,
        client: &mut LobbyClient,
        cvars: &mut Cvars,
        remote: Option<&str>,
        updated_at: Option<f64>,
    ) {
        self.last = serialize_synced_settings(&collect_settings(cvars, &self.storage));
        match decide_sync(&self.last, self.state.as_ref(), &self.account_id, remote, updated_at) {
            SyncDecision::Keep => self.stopped = true,
            SyncDecision::Apply => {
                let blob = parse_synced_settings(remote.unwrap_or_default()).expect("decide_sync parsed it");
                let first = self.state.as_ref().is_none_or(|s| s.account_id != self.account_id);
                if first && !self.files.backup.exists() {
                    write_text(&self.files.backup, &self.last);
                }
                apply_settings(cvars, &mut self.storage, &blob);
                write_text(
                    &self.files.storage,
                    &serde_json::to_string(&self.storage).unwrap_or_default(),
                );
                self.last = serialize_synced_settings(&collect_settings(cvars, &self.storage));
                self.state = Some(SyncState {
                    account_id: self.account_id.clone(),
                    synced: self.last.clone(),
                    synced_at: updated_at.unwrap_or(0.0),
                    edited_at: None,
                });
                self.write_state();
            }
            SyncDecision::Upload => {
                let text = self.last.clone();
                self.upload(client, text);
            }
            SyncDecision::None => {
                self.state = Some(SyncState {
                    account_id: self.account_id.clone(),
                    synced: self.last.clone(),
                    synced_at: updated_at.unwrap_or(0.0),
                    edited_at: None,
                });
                self.write_state();
            }
        }
    }

    /// `check`, every five seconds, and the delayed upload.
    pub fn tick(&mut self, client: &mut LobbyClient, cvars: &Cvars, now: Instant) {
        if self.stopped || !self.ready {
            return;
        }
        if now >= self.next_check {
            self.next_check = now + CHECK_INTERVAL;
            self.check(cvars, now);
        }
        if let Some((at, _)) = &self.upload_at
            && now >= *at
        {
            let (_, text) = self.upload_at.take().expect("checked above");
            self.upload(client, text);
        }
    }

    fn check(&mut self, cvars: &Cvars, now: Instant) {
        let text = serialize_synced_settings(&collect_settings(cvars, &self.storage));
        if text != self.last {
            self.last = text.clone();
            self.upload_at = Some((now + UPLOAD_DELAY, text));
        }
    }

    /// `flush`: on the way out, upload what has not been.
    pub fn flush(&mut self, client: &mut LobbyClient, cvars: &Cvars) {
        if self.stopped || !self.ready {
            return;
        }
        self.check(cvars, Instant::now());
        let Some((_, text)) = self.upload_at.take() else { return };
        if self.state.as_ref().map(|s| s.synced.as_str()) != Some(text.as_str()) {
            self.upload(client, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cvar::CvarOptions;

    fn state(synced: &str, at: f64, edited: Option<f64>) -> SyncState {
        SyncState {
            account_id: "a".into(),
            synced: synced.into(),
            synced_at: at,
            edited_at: edited,
        }
    }

    #[test]
    fn decisions_match_the_browser() {
        let blob = r#"{"v":1,"cvars":{"cl_fov":100},"storage":{}}"#;
        let local = r#"{"v":1,"cvars":{"cl_fov":95},"storage":{}}"#;
        assert_eq!(decide_sync(local, None, "a", None, None), SyncDecision::Upload);
        assert_eq!(
            decide_sync(local, None, "a", Some("junk"), Some(1.0)),
            SyncDecision::Keep
        );
        assert_eq!(decide_sync(blob, None, "a", Some(blob), Some(1.0)), SyncDecision::None);
        assert_eq!(
            decide_sync(local, None, "a", Some(blob), Some(1.0)),
            SyncDecision::Apply
        );
        let st = state(local, 1.0, None);
        assert_eq!(
            decide_sync(local, Some(&st), "a", Some(blob), Some(2.0)),
            SyncDecision::Apply
        );
        let edited = state("older", 2.0, Some(5.0));
        assert_eq!(
            decide_sync(local, Some(&edited), "a", Some(blob), Some(2.0)),
            SyncDecision::Upload
        );
        assert_eq!(
            decide_sync(local, Some(&edited), "a", Some(blob), Some(3.0)),
            SyncDecision::Upload
        );
        assert_eq!(
            decide_sync(local, Some(&edited), "a", Some(blob), Some(9.0)),
            SyncDecision::Apply
        );
    }

    #[test]
    fn collect_and_apply_round_trip() {
        let mut cvars = Cvars::new(None, false);
        cvars.register("cl_fov", 90, CvarOptions::default());
        cvars.register("r_bloom", 1, CvarOptions::default());
        cvars.set("cl_fov", 104);
        cvars.set("r_bloom", 0);
        let mut storage = BTreeMap::new();
        storage.insert("engine_binds".to_owned(), "{}".to_owned());
        storage.insert("not_synced".to_owned(), "x".to_owned());
        let text = serialize_synced_settings(&collect_settings(&cvars, &storage));
        assert_eq!(
            text,
            r#"{"v":1,"cvars":{"cl_fov":104},"storage":{"engine_binds":"{}"}}"#
        );
        let blob = parse_synced_settings(r#"{"v":1,"cvars":{"m_sensitivity":2},"storage":{}}"#).unwrap();
        apply_settings(&mut cvars, &mut storage, &blob);
        assert_eq!(cvars.get_number("cl_fov"), 90.0, "reset: the account has none");
        assert_eq!(cvars.get_number("m_sensitivity"), 2.0, "preloaded for its owner");
        assert_eq!(cvars.get_number("r_bloom"), 0.0, "graphics stay");
        assert!(!storage.contains_key("engine_binds"));
    }
}
