//! SHAG `src/engine/core/Cvar.ts`: Quake-style console variables.
//!
//! Typed get and set with bounds, persistence, the console's listing and
//! `exportConfig` / `execConfig`. The browser keeps one static registry and
//! saves it to `localStorage["engine_cvars"]`; here a [`Cvars`] value is the
//! registry and saves the same JSON object to a file, so a browser's
//! settings can be copied over as they are.
//!
//! `onChange` callbacks are not carried over: whoever cares reads the value
//! when it draws or simulates, and [`Cvars::serial`] says when anything changed.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Map, Value};
use shagger_shared::js::{js_number_to_string, js_parse_float, js_round, js_string_to_number, js_trim};

/// `CvarType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CvarType {
    Bool,
    Int,
    Float,
    String,
}

/// `CvarValue`: what JavaScript holds, a boolean, a number or a string.
#[derive(Debug, Clone, PartialEq)]
pub enum CvarValue {
    Bool(bool),
    Number(f64),
    String(String),
}

impl From<bool> for CvarValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<f64> for CvarValue {
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}
impl From<i32> for CvarValue {
    fn from(v: i32) -> Self {
        Self::Number(f64::from(v))
    }
}
impl From<&str> for CvarValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_owned())
    }
}
impl From<String> for CvarValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl std::fmt::Display for CvarValue {
    /// `String(value)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Number(n) => f.write_str(&js_number_to_string(*n)),
            Self::String(s) => f.write_str(s),
        }
    }
}

impl CvarValue {
    /// As `JSON.stringify` writes it.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(*b),
            // JSON.stringify writes whole numbers without a fraction.
            Self::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => Value::from(*n as i64),
            Self::Number(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
            Self::String(s) => Value::String(s.clone()),
        }
    }

    fn from_json(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(Self::Bool(*b)),
            Value::Number(n) => n.as_f64().map(Self::Number),
            Value::String(s) => Some(Self::String(s.clone())),
            _ => None,
        }
    }
}

/// `CvarFlags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CvarFlags {
    /// Saved (default true).
    pub persist: bool,
    /// The console cannot change it.
    pub readonly: bool,
    pub cheat: bool,
    /// Left out of `cvarlist`.
    pub hidden: bool,
}

impl Default for CvarFlags {
    fn default() -> Self {
        Self {
            persist: true,
            readonly: false,
            cheat: false,
            hidden: false,
        }
    }
}

/// `CvarOptions`.
#[derive(Debug, Clone, Default)]
pub struct CvarOptions {
    pub flags: CvarFlags,
    /// Detected from the default when `None`.
    pub kind: Option<CvarType>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub description: String,
}

impl CvarOptions {
    #[must_use]
    pub fn describe(description: &str) -> Self {
        Self {
            description: description.to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    #[must_use]
    pub fn kind(mut self, kind: CvarType) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// `CvarInstance`.
#[derive(Debug, Clone, PartialEq)]
pub struct CvarInstance {
    pub name: String,
    pub kind: CvarType,
    pub value: CvarValue,
    pub default_value: CvarValue,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub description: String,
    pub flags: CvarFlags,
    /// Declared by its owner; a value loaded before that is only preloaded.
    registered: bool,
}

/// What `execConfig` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecResult {
    pub accepted: usize,
    /// Line (from 1) and why.
    pub rejected: Vec<(usize, String)>,
}

/// `Cvar`'s registry.
#[derive(Debug, Default)]
pub struct Cvars {
    cvars: Vec<CvarInstance>,
    index: HashMap<String, usize>,
    /// Where the persisted values live; nothing is saved without one.
    storage: Option<PathBuf>,
    /// Production clients keep `developer` at 0.
    developer_locked: bool,
    serial: u64,
}

fn valid_value(value: &CvarValue, kind: Option<CvarType>) -> bool {
    match value {
        CvarValue::Number(n) => n.is_finite(),
        CvarValue::Bool(_) => true,
        CvarValue::String(s) => match kind {
            Some(CvarType::Int | CvarType::Float) => !js_trim(s).is_empty() && js_string_to_number(s).is_finite(),
            _ => true,
        },
    }
}

/// `detectType`.
fn detect_type(value: &CvarValue) -> CvarType {
    match value {
        CvarValue::Bool(_) => CvarType::Bool,
        CvarValue::Number(n) if n.fract() == 0.0 => CvarType::Int,
        CvarValue::Number(_) => CvarType::Float,
        CvarValue::String(_) => CvarType::String,
    }
}

/// `parseFloat(value) || 0`.
fn parse_or_zero(s: &str) -> f64 {
    let n = js_parse_float(s);
    if n.is_nan() || n == 0.0 { 0.0 } else { n }
}

fn bounded(mut x: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    // Math.max / Math.min with a finite bound: NaN never reaches here.
    if let Some(m) = min {
        x = x.max(m);
    }
    if let Some(m) = max {
        x = x.min(m);
    }
    x
}

/// `coerceValue`.
fn coerce_value(value: &CvarValue, kind: CvarType, min: Option<f64>, max: Option<f64>) -> CvarValue {
    match kind {
        CvarType::Bool => CvarValue::Bool(match value {
            CvarValue::Bool(b) => *b,
            CvarValue::Number(n) => *n != 0.0,
            CvarValue::String(s) => {
                let lower = s.to_lowercase();
                lower != "0" && lower != "false" && !lower.is_empty()
            }
        }),
        CvarType::Int => {
            let n = match value {
                CvarValue::Bool(b) => f64::from(u8::from(*b)),
                CvarValue::Number(n) => js_round(*n),
                CvarValue::String(s) => js_round(parse_or_zero(s)),
            };
            CvarValue::Number(bounded(n, min, max))
        }
        CvarType::Float => {
            let n = match value {
                CvarValue::Bool(b) => f64::from(u8::from(*b)),
                CvarValue::Number(n) => *n,
                CvarValue::String(s) => parse_or_zero(s),
            };
            CvarValue::Number(bounded(n, min, max))
        }
        CvarType::String => CvarValue::String(value.to_string()),
    }
}

impl Cvars {
    /// `Cvar.init()`: a registry saving to `storage` (a JSON file), with
    /// what is already there loaded.
    #[must_use]
    pub fn new(storage: Option<PathBuf>, developer_locked: bool) -> Self {
        let mut cvars = Self {
            storage,
            developer_locked,
            ..Self::default()
        };
        cvars.load_from_storage();
        cvars
    }

    /// Bumped by every change, for whoever caches what it read.
    #[must_use]
    pub fn serial(&self) -> u64 {
        self.serial
    }

    fn is_locked(&self, name: &str) -> bool {
        self.developer_locked && name.eq_ignore_ascii_case("developer")
    }

    /// The index slot of `name`, case-insensitively. Code reads cvars by
    /// their lowercase names many times a frame, so those skip the copy.
    fn slot(&self, name: &str) -> Option<usize> {
        let slot = if name.bytes().any(|b| !b.is_ascii() || b.is_ascii_uppercase()) {
            self.index.get(&name.to_lowercase())
        } else {
            self.index.get(name)
        };
        slot.copied()
    }

    fn find(&self, name: &str) -> Option<&CvarInstance> {
        self.slot(name).map(|i| &self.cvars[i])
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut CvarInstance> {
        self.slot(name).map(|i| &mut self.cvars[i])
    }

    fn insert(&mut self, cvar: CvarInstance) {
        match self.index.get(&cvar.name) {
            Some(&i) => self.cvars[i] = cvar,
            None => {
                self.index.insert(cvar.name.clone(), self.cvars.len());
                self.cvars.push(cvar);
            }
        }
    }

    fn remove(&mut self, name: &str) {
        if let Some(i) = self.index.remove(name) {
            self.cvars.remove(i);
            for slot in self.index.values_mut() {
                if *slot > i {
                    *slot -= 1;
                }
            }
        }
    }

    /// `Cvar.register`.
    pub fn register(&mut self, name: &str, default_value: impl Into<CvarValue>, options: CvarOptions) -> &CvarInstance {
        let lower = name.to_lowercase();
        let locked = self.is_locked(&lower);
        let mut options = options;
        let mut default_value = default_value.into();
        if locked {
            default_value = CvarValue::Number(0.0);
            options.kind = Some(CvarType::Int);
            options.flags.readonly = true;
            options.flags.persist = false;
        }
        let kind = options.kind.unwrap_or_else(|| detect_type(&default_value));
        let validated = coerce_value(&default_value, kind, options.min, options.max);
        let mut cvar = CvarInstance {
            name: lower.clone(),
            kind,
            value: validated.clone(),
            default_value: validated,
            min: options.min,
            max: options.max,
            description: options.description,
            flags: options.flags,
            registered: true,
        };
        if !locked
            && let Some(existing) = self.find(&lower)
            && valid_value(&existing.value, Some(kind))
        {
            // Keep the persisted value, held to the type and bounds declared now.
            cvar.value = coerce_value(&existing.value, kind, options.min, options.max);
        }
        self.insert(cvar);
        self.serial += 1;
        self.find(&lower).expect("inserted above")
    }

    /// `Cvar.get`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&CvarValue> {
        if self.is_locked(name) {
            return Some(&CvarValue::Number(0.0));
        }
        self.find(name).map(|c| &c.value)
    }

    /// `Cvar.getString`.
    #[must_use]
    pub fn get_string(&self, name: &str) -> String {
        self.get(name).map(ToString::to_string).unwrap_or_default()
    }

    /// `Cvar.getNumber`.
    #[must_use]
    pub fn get_number(&self, name: &str) -> f64 {
        match self.get(name) {
            None => 0.0,
            Some(CvarValue::Bool(b)) => f64::from(u8::from(*b)),
            Some(CvarValue::Number(n)) => *n,
            Some(CvarValue::String(s)) => parse_or_zero(s),
        }
    }

    /// `Cvar.getBool`.
    #[must_use]
    pub fn get_bool(&self, name: &str) -> bool {
        match self.get(name) {
            None => false,
            Some(CvarValue::Bool(b)) => *b,
            Some(CvarValue::Number(n)) => *n != 0.0,
            Some(CvarValue::String(s)) => {
                let lower = s.to_lowercase();
                lower != "0" && lower != "false" && !s.is_empty()
            }
        }
    }

    /// `Cvar.getInt`.
    #[must_use]
    pub fn get_int(&self, name: &str) -> f64 {
        js_round(self.get_number(name))
    }

    /// `Cvar.set`: false when unknown, read-only or not a value of its type.
    pub fn set(&mut self, name: &str, value: impl Into<CvarValue>) -> bool {
        let value = value.into();
        if self.is_locked(name) || !valid_value(&value, None) {
            return false;
        }
        let Some(cvar) = self.find_mut(name) else {
            return false;
        };
        if cvar.flags.readonly {
            return false;
        }
        // A numeric cvar refuses text rather than reading it as zero.
        if matches!(cvar.kind, CvarType::Int | CvarType::Float)
            && let CvarValue::String(s) = &value
        {
            let text = js_trim(s);
            if text.is_empty() || !js_string_to_number(text).is_finite() {
                return false;
            }
        }
        let new_value = coerce_value(&value, cvar.kind, cvar.min, cvar.max);
        if cvar.value == new_value {
            return true;
        }
        cvar.value = new_value;
        let persist = cvar.flags.persist;
        self.serial += 1;
        if persist {
            self.save_to_storage();
        }
        true
    }

    /// `Cvar.toggle`.
    pub fn toggle(&mut self, name: &str) -> bool {
        let Some(cvar) = self.find(name) else {
            return false;
        };
        if cvar.kind != CvarType::Bool {
            let current = self.get_number(name);
            return self.set(name, if current == 0.0 { 1 } else { 0 });
        }
        let next = !self.get_bool(name);
        self.set(name, next)
    }

    /// `Cvar.reset`.
    pub fn reset(&mut self, name: &str) -> bool {
        let Some(cvar) = self.find(name) else {
            return false;
        };
        if !cvar.registered {
            let lower = cvar.name.clone();
            self.remove(&lower);
            self.serial += 1;
            self.save_to_storage();
            return true;
        }
        let default = cvar.default_value.clone();
        self.set(name, default)
    }

    /// `Cvar.resetAll`.
    pub fn reset_all(&mut self) {
        let names: Vec<String> = self
            .cvars
            .iter()
            .filter(|c| !c.flags.readonly)
            .map(|c| c.name.clone())
            .collect();
        for name in names {
            self.reset(&name);
        }
    }

    /// `Cvar.getCvar`.
    #[must_use]
    pub fn get_cvar(&self, name: &str) -> Option<&CvarInstance> {
        self.find(name)
    }

    /// `Cvar.list`: visible cvars starting with `filter`, by name.
    #[must_use]
    pub fn list(&self, filter: Option<&str>) -> Vec<&CvarInstance> {
        let filter = filter.map(str::to_lowercase);
        let mut out: Vec<&CvarInstance> = self
            .cvars
            .iter()
            .filter(|c| !c.flags.hidden)
            .filter(|c| filter.as_ref().is_none_or(|f| c.name.starts_with(f.as_str())))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Every registered cvar's declaration as JSON (`name`, `type`,
    /// `default`, `min`, `max`, `description` and the flags), by name: what a
    /// page needs to `Cvar.register` the same cvars (WEBVIEW_LOBBY.md 8).
    #[must_use]
    pub fn declarations(&self) -> Vec<Value> {
        let mut out: Vec<&CvarInstance> = self.cvars.iter().filter(|c| c.registered).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.into_iter()
            .map(|c| {
                let kind = match c.kind {
                    CvarType::Bool => "bool",
                    CvarType::Int => "int",
                    CvarType::Float => "float",
                    CvarType::String => "string",
                };
                let mut d = serde_json::json!({
                    "name": c.name,
                    "type": kind,
                    "default": c.default_value.to_json(),
                    "persist": c.flags.persist,
                    "readonly": c.flags.readonly,
                    "cheat": c.flags.cheat,
                    "hidden": c.flags.hidden,
                });
                if let Some(min) = c.min {
                    d["min"] = Value::from(min);
                }
                if let Some(max) = c.max {
                    d["max"] = Value::from(max);
                }
                if !c.description.is_empty() {
                    d["description"] = Value::String(c.description.clone());
                }
                d
            })
            .collect()
    }

    /// `Cvar.getNames`, for autocomplete.
    #[must_use]
    pub fn get_names(&self) -> Vec<String> {
        self.list(None).into_iter().map(|c| c.name.clone()).collect()
    }

    /// `Cvar.exists`.
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// `Cvar.isRegistered`.
    #[must_use]
    pub fn is_registered(&self, name: &str) -> bool {
        self.find(name).is_some_and(|c| c.registered)
    }

    /// `Cvar.replacePersistedValues`: settings sync's download.
    pub fn replace_persisted_values(&mut self, values: &Map<String, Value>, accepts: impl Fn(&str) -> bool) {
        let incoming: Vec<(String, CvarValue)> = values
            .iter()
            .filter_map(|(k, v)| Some((k.to_lowercase(), CvarValue::from_json(v)?)))
            .collect();
        let names: Vec<String> = self.cvars.iter().map(|c| c.name.clone()).collect();
        for name in names {
            let Some(cvar) = self.find(&name) else { continue };
            if !accepts(&name) || !cvar.flags.persist || cvar.flags.readonly {
                continue;
            }
            if !cvar.registered {
                self.remove(&name);
            } else if let Some((_, v)) = incoming.iter().find(|(k, _)| *k == name) {
                let v = v.clone();
                self.set(&name, v);
            } else {
                self.reset(&name);
            }
        }
        for (name, value) in incoming {
            if accepts(&name) && !self.exists(&name) && valid_value(&value, None) {
                self.preload(&name, value);
            }
        }
        self.serial += 1;
        self.save_to_storage();
    }

    /// `Cvar.formatInfo`.
    #[must_use]
    pub fn format_info(&self, name: &str) -> String {
        let Some(cvar) = self.find(name) else {
            return format!("Unknown cvar: {name}");
        };
        let mut info = format!("\"{}\" is \"{}\"", cvar.name, cvar.value);
        if cvar.value != cvar.default_value {
            info += &format!(" (default: \"{}\")", cvar.default_value);
        }
        if cvar.min.is_some() || cvar.max.is_some() {
            let n = |v: Option<f64>| v.map(js_number_to_string).unwrap_or_default();
            info += &format!(" [{}..{}]", n(cvar.min), n(cvar.max));
        }
        if !cvar.description.is_empty() {
            info += &format!("\n  {}", cvar.description);
        }
        if cvar.flags.readonly {
            info += " [readonly]";
        }
        info
    }

    // ── Persistence ──

    fn load_from_storage(&mut self) {
        let Some(path) = &self.storage else { return };
        // A save still on its way to disk is the newest text.
        crate::core::atomic_file::flush_pending(std::time::Duration::from_secs(2));
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        for (name, value) in &data {
            if self.is_locked(name) {
                continue;
            }
            let Some(value) = CvarValue::from_json(value) else {
                continue;
            };
            let kind = self.find(name).map(|c| c.kind);
            if !valid_value(&value, kind) {
                continue;
            }
            match self.find_mut(name) {
                Some(cvar) if cvar.flags.persist => {
                    cvar.value = coerce_value(&value, cvar.kind, cvar.min, cvar.max);
                }
                Some(_) => {}
                None => self.preload(name, value),
            }
        }
    }

    fn preload(&mut self, name: &str, value: CvarValue) {
        self.insert(CvarInstance {
            name: name.to_lowercase(),
            kind: detect_type(&value),
            default_value: value.clone(),
            value,
            min: None,
            max: None,
            description: String::new(),
            flags: CvarFlags::default(),
            registered: false,
        });
    }

    /// The persisted object, as `localStorage["engine_cvars"]` holds it.
    #[must_use]
    pub fn persisted(&self) -> Map<String, Value> {
        let mut data = Map::new();
        for cvar in &self.cvars {
            if cvar.flags.persist && (!cvar.registered || cvar.value != cvar.default_value) {
                data.insert(cvar.name.clone(), cvar.value.to_json());
            }
        }
        data
    }

    fn save_to_storage(&self) {
        let Some(path) = &self.storage else { return };
        let text = Value::Object(self.persisted()).to_string();
        // A failed write keeps the values for this run, as the browser does;
        // a crash mid write keeps the last whole file. Off the game thread:
        // a slider sets its cvar every frame. The game flushes before it
        // exits (`atomic_file::flush_pending`).
        crate::core::atomic_file::write_atomic_later(path, text.into_bytes());
    }

    // ── Batch ──

    /// `Cvar.exportConfig`.
    #[must_use]
    pub fn export_config(&self) -> String {
        let mut lines: Vec<String> = self
            .cvars
            .iter()
            .filter(|c| c.value != c.default_value && !c.flags.hidden)
            .map(|c| format!("{} {}", c.name, Value::String(c.value.to_string())))
            .collect();
        lines.sort();
        lines.join("\n")
    }

    /// `Cvar.execConfig`.
    pub fn exec_config(&mut self, config: &str) -> ExecResult {
        let mut result = ExecResult::default();
        for (index, line) in config.split('\n').enumerate() {
            let trimmed = js_trim(line);
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }
            let Some((name, value)) = split_config_line(trimmed) else {
                result
                    .rejected
                    .push((index + 1, "Expected a setting name and value".into()));
                continue;
            };
            let reason = match self.find(&name) {
                None => Some("Unknown setting"),
                Some(c) if c.flags.readonly => Some("Read-only setting"),
                _ => None,
            };
            if let Some(reason) = reason {
                result.rejected.push((index + 1, reason.into()));
            } else if self.set(&name, value) {
                result.accepted += 1;
            } else {
                result.rejected.push((index + 1, "Invalid value".into()));
            }
        }
        result
    }
}

thread_local! {
    /// `Cvar`'s static registry: one per thread, as the browser has one per page.
    static CVARS: std::cell::RefCell<Cvars> = std::cell::RefCell::new(Cvars::default());
}

/// `Cvar.init`: the registry, loaded from `storage`. Registrations made
/// before keep their declarations and take the stored values.
pub fn init_cvars(storage: Option<PathBuf>, developer_locked: bool) {
    CVARS.with(|cell| {
        let before = std::mem::take(&mut *cell.borrow_mut());
        let mut cvars = Cvars::new(storage, developer_locked);
        for c in before.cvars.into_iter().filter(|c| c.registered) {
            let options = CvarOptions {
                flags: c.flags,
                kind: Some(c.kind),
                min: c.min,
                max: c.max,
                description: c.description,
            };
            cvars.register(&c.name, c.default_value, options);
        }
        *cell.borrow_mut() = cvars;
    });
}

/// The registry, for the length of `f`. Do not call it again inside `f`.
pub fn with_cvars<R>(f: impl FnOnce(&mut Cvars) -> R) -> R {
    CVARS.with(|cell| f(&mut cell.borrow_mut()))
}

/// `/^(\S+)\s+("(?:[^"\\]|\\.)*"|[^"\s]+)$/`, with the quoted value read as JSON.
fn split_config_line(line: &str) -> Option<(String, String)> {
    let ws = shagger_shared::js::is_js_whitespace;
    let name_end = line.find(ws)?;
    let name = &line[..name_end];
    let rest = line[name_end..].trim_start_matches(ws);
    if rest.is_empty() {
        return None;
    }
    if let Some(body) = rest.strip_prefix('"') {
        // A quoted string with escapes, ending the line.
        let mut chars = body.char_indices();
        let mut end = None;
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    chars.next()?;
                }
                '"' => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let end = end?;
        if end + 1 != body.len() {
            return None;
        }
        let quoted = &rest[..end + 2];
        let value = serde_json::from_str::<String>(quoted).unwrap_or_else(|_| body[..end].to_owned());
        return Some((name.to_owned(), value));
    }
    if rest.contains(|c: char| c == '"' || ws(c)) {
        return None;
    }
    Some((name.to_owned(), rest.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Cvars {
        let mut c = Cvars::new(None, false);
        c.register("r_bloom", true, CvarOptions::default());
        c.register("cl_fov", 90, CvarOptions::default().range(60.0, 130.0));
        c.register("s_volume", 0.8, CvarOptions::default().range(0.0, 1.0));
        c.register("name", "Washiki", CvarOptions::default());
        c
    }

    #[test]
    fn values_hold_their_type_and_bounds() {
        let mut c = registry();
        assert!(c.set("cl_fov", "200"));
        assert_eq!(c.get_number("cl_fov"), 130.0);
        assert!(c.set("cl_fov", 99.5));
        assert_eq!(c.get_number("cl_fov"), 100.0, "Math.round");
        assert!(!c.set("cl_fov", "abc"), "a typo is refused, not zero");
        assert!(c.set("r_bloom", "false"));
        assert!(!c.get_bool("r_bloom"));
        assert!(c.toggle("r_bloom"));
        assert!(c.get_bool("r_bloom"));
        assert!(c.set("S_VOLUME", 0.25));
        assert_eq!(c.get_string("s_volume"), "0.25");
        assert!(!c.set("nope", 1));
    }

    #[test]
    fn config_text_round_trips() {
        let mut c = registry();
        c.set("cl_fov", 110);
        c.set("name", "Ramen \"the\" Cat");
        let text = c.export_config();
        assert_eq!(text, "cl_fov \"110\"\nname \"Ramen \\\"the\\\" Cat\"");
        let mut d = registry();
        let r = d.exec_config(&format!("{text}\n// note\nbogus 1\ncl_fov\n"));
        assert_eq!(r.accepted, 2);
        assert_eq!(
            r.rejected,
            vec![
                (4, "Unknown setting".into()),
                (5, "Expected a setting name and value".into())
            ]
        );
        assert_eq!(d.get_string("name"), "Ramen \"the\" Cat");
    }

    #[test]
    fn declarations_describe_registered_cvars_only() {
        let mut c = Cvars::new(None, false);
        c.register(
            "r_bloom",
            0.0,
            CvarOptions::describe("Bloom").range(0.0, 3.0).kind(CvarType::Float),
        );
        c.register("cl_hints", true, CvarOptions::default());
        c.preload("r_unowned", CvarValue::Number(1.0));
        let d = c.declarations();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0]["name"], "cl_hints");
        assert_eq!(d[0]["type"], "bool");
        assert_eq!(d[0]["default"], true);
        assert!(d[0].get("min").is_none());
        assert_eq!(d[1]["name"], "r_bloom");
        assert_eq!(d[1]["type"], "float");
        assert_eq!(d[1]["default"], 0);
        assert_eq!(d[1]["max"], 3.0);
        assert_eq!(d[1]["description"], "Bloom");
        assert_eq!(d[1]["persist"], true);
    }

    #[test]
    fn persisted_values_survive_a_restart_and_wait_for_their_owner() {
        let dir = std::env::temp_dir().join(format!("shagger-cvar-{}", std::process::id()));
        let path = dir.join("cvars.json");
        let _ = std::fs::remove_file(&path);
        {
            let mut c = Cvars::new(Some(path.clone()), false);
            c.register("cl_fov", 90, CvarOptions::default());
            c.set("cl_fov", 105);
        }
        // A key from the browser's export, for a cvar not registered yet.
        crate::core::atomic_file::flush_pending(std::time::Duration::from_secs(5));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, r#"{"cl_fov":105}"#);
        std::fs::write(&path, r#"{"cl_fov":105,"g_later":"x"}"#).unwrap();
        let mut c = Cvars::new(Some(path.clone()), false);
        assert_eq!(c.get_number("cl_fov"), 105.0, "preloaded");
        assert!(!c.is_registered("cl_fov"));
        c.register("cl_fov", 90, CvarOptions::default().range(60.0, 100.0));
        assert_eq!(c.get_number("cl_fov"), 100.0, "held to the bounds declared later");
        assert_eq!(c.get_string("g_later"), "x");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn production_locks_developer() {
        let mut c = Cvars::new(None, true);
        c.register("developer", 1, CvarOptions::default());
        assert_eq!(c.get_number("developer"), 0.0);
        assert!(!c.set("developer", 1));
    }
}
