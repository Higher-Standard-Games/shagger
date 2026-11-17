//! Files written whole or not at all: the text goes to `<name>.part`, is
//! flushed to disk, then renamed over the file. A crash or power loss mid
//! write leaves the old file, never a cut-off one (the browser's
//! `localStorage` gives the same guarantee).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Writes `bytes` to `path` whole, making its folder if needed.
///
/// # Errors
/// The folder could not be made, or the write or rename failed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_whole(path, bytes, false)
}

/// [`write_atomic`] for a secret, such as the player's account key: on Unix
/// the file is readable and writable by its owner only (0600), where the
/// process's umask would usually leave it readable by every user of the
/// machine. Windows keeps the user profile's own permissions, which other
/// users cannot read.
///
/// # Errors
/// As [`write_atomic`].
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_whole(path, bytes, true)
}

/// Makes `path` readable by its owner only, on Unix, if anyone else can read
/// it now: for a secret written before [`write_atomic_private`] existed.
/// Does nothing elsewhere, or when the file is missing.
pub fn restrict_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path)
            && meta.permissions().mode() & 0o077 != 0
        {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn create_part(part: &Path, private: bool) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        // `mode` applies only to a file it creates: a part file left by a
        // crash would keep its own.
        let _ = std::fs::remove_file(part);
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    options.open(part)
}

fn write_whole(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    // This process's own: two games sharing a folder never write into one
    // part file together.
    let mut part = path.as_os_str().to_os_string();
    part.push(format!(".{}.part", std::process::id()));
    let part = std::path::PathBuf::from(part);
    let written = create_part(&part, private).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(e) = written {
        // A full disk or a refused write leaves no part file behind.
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    // Windows refuses to replace a file another program holds open for a
    // moment (a virus scanner, a sync client): access denied (5) or a
    // sharing violation (32). Try again a few times there; elsewhere a
    // refusal is final.
    let busy = |e: &std::io::Error| cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32));
    let mut tries = 0;
    loop {
        match std::fs::rename(&part, path) {
            Err(e) if busy(&e) && tries < 5 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20 * tries));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
            done => return done,
        }
    }
}

/// Writes waiting for the writer thread: the newest text for each path.
struct Queue {
    jobs: Vec<(PathBuf, Vec<u8>)>,
    writing: bool,
    started: bool,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue {
    jobs: Vec::new(),
    writing: false,
    started: false,
});
static CHANGED: Condvar = Condvar::new();

/// [`write_atomic`] on a writer thread, so the caller never waits for the
/// disk (a slider sets its cvar every frame, and a flush to disk can take
/// tens of milliseconds). A newer write to the same path replaces one not
/// yet started; [`flush_pending`] waits for them all. A failed write is
/// dropped, as a failed synchronous one is by the callers.
pub fn write_atomic_later(path: &Path, bytes: Vec<u8>) {
    let Ok(mut queue) = QUEUE.lock() else { return };
    match queue.jobs.iter_mut().find(|(p, _)| p == path) {
        Some(job) => job.1 = bytes,
        None => queue.jobs.push((path.to_owned(), bytes)),
    }
    if !queue.started {
        let spawned = std::thread::Builder::new().name("file writer".into()).spawn(writer);
        if spawned.is_err() {
            // No thread: write here, as before.
            let jobs = std::mem::take(&mut queue.jobs);
            drop(queue);
            for (path, bytes) in jobs {
                let _ = write_atomic(&path, &bytes);
            }
            return;
        }
        queue.started = true;
    }
    CHANGED.notify_all();
}

fn writer() {
    let Ok(mut queue) = QUEUE.lock() else { return };
    loop {
        if queue.jobs.is_empty() {
            queue = match CHANGED.wait(queue) {
                Ok(q) => q,
                Err(_) => return,
            };
            continue;
        }
        let (path, bytes) = queue.jobs.remove(0);
        queue.writing = true;
        drop(queue);
        let _ = write_atomic(&path, &bytes);
        queue = match QUEUE.lock() {
            Ok(q) => q,
            Err(_) => return,
        };
        queue.writing = false;
        CHANGED.notify_all();
    }
}

/// Waits, up to `limit`, until every [`write_atomic_later`] is on disk:
/// before the process exits.
pub fn flush_pending(limit: Duration) {
    let deadline = Instant::now() + limit;
    let Ok(mut queue) = QUEUE.lock() else { return };
    while !queue.jobs.is_empty() || queue.writing {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        queue = match CHANGED.wait_timeout(queue, left) {
            Ok((q, _)) => q,
            Err(_) => return,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_replaced_whole() {
        let dir = std::env::temp_dir().join(format!("shagger-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("deep").join("cvars.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(
            !dir.join("deep")
                .join(format!("cvars.json.{}.part", std::process::id()))
                .exists()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_secret_is_written_for_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("shagger-atomic-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("lobby-key");
        write_atomic(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        restrict_to_owner(&path);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_file(&path).unwrap();
        write_atomic_private(&path, b"key").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"key");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_no_part_file() {
        let dir = std::env::temp_dir().join(format!("shagger-atomic-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The target is a folder, so the rename fails.
        let path = dir.join("taken");
        std::fs::create_dir_all(path.join("inside")).unwrap();
        assert!(write_atomic(&path, b"x").is_err());
        assert!(!dir.join(format!("taken.{}.part", std::process::id())).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_later_write_lands_by_the_flush_and_the_newest_wins() {
        let dir = std::env::temp_dir().join(format!("shagger-atomic-later-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cvars.json");
        for i in 0..50 {
            write_atomic_later(&path, format!("{i}").into_bytes());
        }
        flush_pending(Duration::from_secs(5));
        assert_eq!(std::fs::read(&path).unwrap(), b"49");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
