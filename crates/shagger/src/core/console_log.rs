//! The console as a file: a game started from its icon has no terminal,
//! so what it prints would be lost. SHAG has no counterpart (a browser
//! keeps its console); this is the native client's own.
//!
//! [`redirect`] points the process's standard output and error at a file
//! when standard output is not a terminal (a Windows GUI build, an
//! AppImage or app bundle started from the desktop). The file is new
//! each run; the run before is kept beside it as `<stem>.old.<ext>`.
//! Started from a terminal, nothing changes.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Where the run before this one's log is kept: `washoi.log` →
/// `washoi.old.log`.
#[must_use]
pub fn previous_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map_or_else(|| "log".into(), |s| s.to_string_lossy().into_owned());
    let name = match path.extension() {
        Some(ext) => format!("{stem}.old.{}", ext.to_string_lossy()),
        None => format!("{stem}.old"),
    };
    path.with_file_name(name)
}

/// Sends standard output and error to `path` unless standard output is a
/// terminal. True when they now go to the file.
pub fn redirect(path: &Path) -> bool {
    if std::io::stdout().is_terminal() {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::rename(path, previous_path(path));
    let Ok(file) = fs::File::create(path) else {
        return false;
    };
    platform::redirect(file)
}

#[cfg(unix)]
#[allow(unsafe_code)]
mod platform {
    use std::os::fd::IntoRawFd;

    pub fn redirect(file: std::fs::File) -> bool {
        let fd = file.into_raw_fd();
        // SAFETY: `fd` is an open descriptor this process owns; dup2 only
        // makes 1 and 2 refer to the same file. `fd` stays open for the
        // life of the process.
        unsafe { libc::dup2(fd, 1) >= 0 && libc::dup2(fd, 2) >= 0 }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};

    pub fn redirect(file: std::fs::File) -> bool {
        // Never closed: both standard handles refer to it for the life of
        // the process. Rust's stdout and stderr look the handle up on each
        // write, so they follow it.
        let handle = file.into_raw_handle();
        // SAFETY: `handle` is a valid file handle this process owns.
        unsafe { SetStdHandle(STD_OUTPUT_HANDLE, handle) != 0 && SetStdHandle(STD_ERROR_HANDLE, handle) != 0 }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    pub fn redirect(_file: std::fs::File) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_path_keeps_the_extension() {
        assert_eq!(
            previous_path(Path::new("/c/washoi.log")),
            PathBuf::from("/c/washoi.old.log")
        );
        assert_eq!(previous_path(Path::new("/c/console")), PathBuf::from("/c/console.old"));
    }
}
