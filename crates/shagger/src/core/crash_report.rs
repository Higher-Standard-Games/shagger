//! Crash reports: a panic anywhere writes what happened to a file the
//! player can send. SHAG has no counterpart (a browser keeps its console);
//! this is the native client's own.
//!
//! [`install`] sets a panic hook that, before the default message, writes
//! `crash-<seconds since 1970>.txt` into the given directory: the program
//! and its version, the system, the thread, the message, where it was
//! raised and a backtrace. Only the newest [`KEEP`] reports are kept.
//! Nothing is sent anywhere.

use std::backtrace::Backtrace;
use std::fmt::Write as _;
use std::fs;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How many reports stay in the directory.
pub const KEEP: usize = 20;

/// The report's text.
#[must_use]
pub fn report(program: &str, version: &str, info: &PanicHookInfo<'_>, backtrace: &Backtrace) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned());
    let location = info.location().map_or_else(
        || "(unknown)".to_owned(),
        |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
    );
    let thread = std::thread::current();
    let mut text = String::new();
    let _ = writeln!(text, "{program} {version} crashed");
    let _ = writeln!(text, "system: {} {}", std::env::consts::OS, std::env::consts::ARCH);
    let _ = writeln!(text, "thread: {}", thread.name().unwrap_or("(unnamed)"));
    let _ = writeln!(text, "message: {message}");
    let _ = writeln!(text, "at: {location}");
    let _ = writeln!(text, "\nbacktrace:\n{backtrace}");
    text
}

/// Writes `text` as a new report in `dir` and drops the oldest beyond
/// [`KEEP`]; the report's path, if it could be written.
pub fn write_report(dir: &Path, text: &str) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut path = dir.join(format!("crash-{seconds}.txt"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("crash-{seconds}-{n}.txt"));
        n += 1;
    }
    fs::write(&path, text).ok()?;
    let mut reports: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("crash-"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    reports.sort();
    let extra = reports.len().saturating_sub(KEEP);
    for (_, old) in reports.into_iter().take(extra) {
        if old != path {
            let _ = fs::remove_file(old);
        }
    }
    Some(path)
}

/// Sets the panic hook: each panic writes a report into `dir`, then the
/// default hook prints its message as before.
pub fn install(program: &'static str, version: &'static str, dir: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = report(program, version, info, &Backtrace::force_capture());
        if let Some(path) = write_report(&dir, &text) {
            eprintln!("{program} crashed; the report is in {}", path.display());
        }
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_written_and_pruned() {
        let dir = std::env::temp_dir().join(format!("shagger-crash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = write_report(&dir, "one").unwrap();
        assert_eq!(fs::read_to_string(&first).unwrap(), "one");
        for i in 0..KEEP + 3 {
            write_report(&dir, &i.to_string()).unwrap();
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), KEEP);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_panic_reports_its_message_and_place() {
        let caught = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let into = caught.clone();
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            *into.lock().unwrap() = report("game", "1.2.3", info, &Backtrace::disabled());
        }));
        let _ = std::panic::catch_unwind(|| panic!("the {} went wrong", "frame"));
        std::panic::set_hook(default);
        let text = caught.lock().unwrap().clone();
        assert!(text.starts_with("game 1.2.3 crashed\n"), "{text}");
        assert!(text.contains("message: the frame went wrong"));
        assert!(text.contains("crash_report.rs:"));
    }
}
