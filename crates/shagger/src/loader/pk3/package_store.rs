//! The published packages kept on disk: the native client's counterpart of
//! SHAG `src/engine/loader/pk3/PackageStore.ts`, which keeps them in the
//! browser's CacheStorage and fetches each one as a map first needs it.
//!
//! Here [`sync_packages`] follows the startup of RUST.md 20.2 for every
//! package at once: the manifest, the index it names (refused unless
//! [`parse_published_index`] accepts the whole snapshot), then each package
//! the directory does not already hold, fetched with `?sha256=` as the
//! browser's cache key, checked against the index's size and SHA-256, and
//! moved into place only once whole. Packages the index no longer lists are
//! removed, so the directory can be mounted as it stands (`ingest_pk3_dir`).
//! The index is written last, as `index.json`: a directory with one holds a
//! complete set, which is what a start without the network can use.
//!
//! The browser's memory and disk budgets, priorities and lazy per-map
//! loading are left out: the native client keeps every package (about 200
//! MB for Washoi today) and mounts them all.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::published_index::{Pk3IndexData, has_published_index, parse_published_index};
use crate::network::client::http_fetch::{FetchError, fetch_at_most};

/// How long a transfer may go quiet before it is given up.
const IDLE: Duration = Duration::from_secs(30);
/// How long the manifest or the index may take.
const SMALL_TOTAL: Duration = Duration::from_secs(60);
/// How long one package may take.
const PACKAGE_TOTAL: Duration = Duration::from_secs(30 * 60);
/// The index kept beside the packages once they are complete.
pub const SYNCED_INDEX: &str = "index.json";
/// The last complete index, set aside while a sync changes the packages:
/// what it lists is still the store's to prune if the sync stops half way.
const PREVIOUS_INDEX: &str = "index.previous.json";
/// Held while a sync runs, so two copies of the game do not write the same files.
const LOCK_FILE: &str = "sync.lock";

/// Where a sync is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncProgress {
    /// The package being fetched, or empty while the index is.
    pub package: String,
    /// Bytes of every package to fetch, and how many have arrived.
    pub received: u64,
    pub total: u64,
}

/// Why a sync stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    Fetch {
        url: String,
        error: FetchError,
    },
    /// The manifest is not one a client accepts.
    BadManifest,
    /// The index does not match its manifest, or breaks a rule.
    BadIndex,
    /// A package arrived with another size or digest than the index's.
    Mismatch {
        pk3: String,
    },
    Io(String),
    /// Another copy of the game is updating this directory.
    Busy,
    Cancelled,
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch { url, error } => write!(f, "{url}: {error:?}"),
            Self::BadManifest => f.write_str("the package manifest is not valid"),
            Self::BadIndex => f.write_str("the package index does not match its manifest"),
            Self::Mismatch { pk3 } => write!(f, "{pk3} does not match the index"),
            Self::Io(why) => f.write_str(why),
            Self::Busy => f.write_str("another copy of the game is updating the data"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

/// What a sync did.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncReport {
    pub index: Pk3IndexData,
    pub downloaded: Vec<String>,
    pub removed: Vec<String>,
    /// Extra files that could not be fetched; any earlier copy stays.
    pub extras_failed: Vec<String>,
}

/// The most an extra file may be.
const EXTRA_MAX: u64 = 1 << 20;

fn io(e: &std::io::Error, path: &Path) -> SyncError {
    SyncError::Io(format!("{}: {e}", path.display()))
}

/// The SHA-256 of a file, as lowercase hex.
fn file_sha256(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Some(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The index name a manifest gives: `pk3_index.<hash>.json` on current
/// deployments, `pk3_index.json` before them. Anything else could reach
/// outside the data directory and is refused.
fn index_file_name(name: Option<&str>) -> Option<String> {
    let name = name.unwrap_or("pk3_index.json");
    let stem = name.strip_prefix("pk3_index")?.strip_suffix(".json")?;
    let hash = stem.strip_prefix('.').unwrap_or(stem);
    (stem.is_empty() || (stem.starts_with('.') && !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit())))
        .then(|| name.to_owned())
}

/// The packages the last complete sync left, read from its index.
#[must_use]
pub fn synced_index(dir: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(dir.join(SYNCED_INDEX)).ok()?).ok()
}

/// Whether `dir` holds a complete set from an earlier sync: its index, and
/// every package that index lists at its size. A sync sets the index
/// aside before it changes anything, so an interrupted one leaves none.
#[must_use]
pub fn is_synced(dir: &Path) -> bool {
    synced_index(dir)
        .as_ref()
        .and_then(|index| index.get("packages")?.as_array().cloned())
        .is_some_and(|packages| {
            packages.iter().all(|p| {
                let (Some(name), Some(size)) = (
                    p.get("pk3").and_then(Value::as_str),
                    p.get("size").and_then(Value::as_u64),
                ) else {
                    return false;
                };
                fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == size)
            })
        })
}

/// The packages an index lists, by name.
fn listed(index: Option<&Value>) -> Vec<String> {
    index
        .and_then(|i| i.get("packages")?.as_array())
        .map_or_else(Vec::new, |packages| {
            packages
                .iter()
                .filter_map(|p| p.get("pk3").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
}

/// Whether the package on disk is the one the index lists: by size, and by
/// digest on every sync, as `PackageStore` checks each cache read, so a
/// file damaged at the same size is fetched again.
fn up_to_date(path: &Path, size: u64, sha256: Option<&str>) -> bool {
    let Ok(meta) = fs::metadata(path) else { return false };
    if meta.len() != size {
        return false;
    }
    sha256.is_none_or(|sha256| file_sha256(path).as_deref() == Some(sha256))
}

/// Whether `dir` holds any package at all: enough to try playing without
/// the network when a sync cannot finish.
#[must_use]
pub fn has_packages(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".pk3"))
    })
}

/// How many times a failed fetch is tried again, and how long apart
/// (`PackageStore`: twice, half a second apart).
const RETRIES: u32 = 2;
const RETRY_DELAY: Duration = Duration::from_millis(500);

/// Whether a failure may pass on its own: a server error, a timeout, too
/// many requests, or a broken connection.
fn retryable(error: &FetchError) -> bool {
    match error {
        FetchError::Status(code) => *code >= 500 || *code == 408 || *code == 429,
        FetchError::Network(_) | FetchError::Stalled => true,
        FetchError::TooLong | FetchError::TooLarge | FetchError::Cancelled => false,
    }
}

/// Fetches `url`, trying a passing failure again, and a body `valid` refuses
/// too (a transfer cut short, or a stale copy on the way), as
/// `PackageStore` validates inside its retry.
fn get(
    url: &str,
    total: Duration,
    max: Option<u64>,
    valid: &dyn Fn(&[u8]) -> Result<(), SyncError>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<Vec<u8>, SyncError> {
    let mut attempt = 0;
    loop {
        match fetch_at_most(url, IDLE, total, max, cancel, &mut |received, _| progress(received)) {
            Ok(bytes) => match valid(&bytes) {
                Ok(()) => return Ok(bytes),
                Err(_) if attempt < RETRIES => {
                    attempt += 1;
                    std::thread::sleep(RETRY_DELAY);
                }
                Err(why) => return Err(why),
            },
            Err(FetchError::Cancelled) => return Err(SyncError::Cancelled),
            Err(error) if attempt < RETRIES && retryable(&error) => {
                attempt += 1;
                std::thread::sleep(RETRY_DELAY);
            }
            Err(error) => {
                return Err(SyncError::Fetch {
                    url: url.to_owned(),
                    error,
                });
            }
        }
    }
}

/// Writes `bytes` beside `path` and moves them into place once they are on
/// the disk, so a crash leaves the old file or the whole new one.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    use std::io::Write;
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let part = path.with_file_name(format!("{name}.part"));
    // The rename replaces a file only where nothing holds it open (Windows).
    let mut file = fs::File::create(&part).map_err(|e| io(&e, &part))?;
    file.write_all(bytes).map_err(|e| io(&e, &part))?;
    file.sync_all().map_err(|e| io(&e, &part))?;
    drop(file);
    fs::rename(&part, path).map_err(|e| io(&e, path))
}

/// Brings `dir` up to the packages published at `base_url` (such as
/// `https://eu.washoi.net/data`); see the module's comment.
///
/// # Errors
/// Any [`SyncError`]; the directory keeps whatever was whole before.
pub fn sync_packages(
    base_url: &str,
    dir: &Path,
    extras: &[&str],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&SyncProgress),
) -> Result<SyncReport, SyncError> {
    let base = base_url.trim_end_matches('/');
    fs::create_dir_all(dir).map_err(|e| io(&e, dir))?;
    let lock_path = dir.join(LOCK_FILE);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| io(&e, &lock_path))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Err(SyncError::Busy),
        Err(fs::TryLockError::Error(e)) => return Err(io(&e, &lock_path)),
    }
    progress(&SyncProgress {
        package: String::new(),
        received: 0,
        total: 0,
    });
    let manifest_bytes = get(
        &format!("{base}/pk3_manifest.json"),
        SMALL_TOTAL,
        None,
        &|_| Ok(()),
        cancel,
        &mut |_| {},
    )?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|_| SyncError::BadManifest)?;
    let published = has_published_index(&manifest).ok_or(SyncError::BadManifest)?;
    let index_file = index_file_name(published.index_file.as_deref()).ok_or(SyncError::BadManifest)?;
    let index_bytes = get(
        &format!("{base}/{index_file}"),
        SMALL_TOTAL,
        None,
        &|_| Ok(()),
        cancel,
        &mut |_| {},
    )?;
    let index_text = String::from_utf8(index_bytes).map_err(|_| SyncError::BadIndex)?;
    let index = parse_published_index(&index_text, &manifest, &format!("{base}/")).ok_or(SyncError::BadIndex)?;
    // The last complete index, or the one a sync that stopped half way set aside.
    let set_aside = dir.join(PREVIOUS_INDEX);
    let previous = synced_index(dir).or_else(|| serde_json::from_str(&fs::read_to_string(&set_aside).ok()?).ok());
    progress(&SyncProgress {
        package: String::new(),
        received: 0,
        total: 0,
    });
    let wanted: Vec<_> = index
        .packages
        .iter()
        .filter(|p| !up_to_date(&dir.join(&p.pk3), p.size, p.sha256.as_deref()))
        .collect();
    let total: u64 = wanted.iter().map(|p| p.size).sum();
    // What the store put here before and the index no longer lists; a
    // package it never fetched (a player's own, under `--data`) stays.
    let dropped: Vec<String> = listed(previous.as_ref())
        .into_iter()
        .filter(|name| !index.packages.iter().any(|p| p.pk3 == *name))
        .collect();
    // From the first change on, the directory is not a complete set until
    // the new index is written.
    let synced: PathBuf = dir.join(SYNCED_INDEX);
    // the new index is written; the old one is set aside, not lost, so what
    // it lists can still be pruned after an interruption.
    if (!wanted.is_empty() || !dropped.is_empty()) && synced.exists() {
        fs::rename(&synced, &set_aside).map_err(|e| io(&e, &synced))?;
    }
    let mut done = 0;
    let mut downloaded = Vec::new();
    for package in wanted {
        if cancel.load(Ordering::Relaxed) {
            return Err(SyncError::Cancelled);
        }
        let url = match &package.sha256 {
            Some(sha) => format!("{}?sha256={sha}", package.path),
            None => package.path.clone(),
        };
        let mut report = |received: u64| {
            progress(&SyncProgress {
                package: package.pk3.clone(),
                received: done + received.min(package.size),
                total,
            });
        };
        report(0);
        let valid = |bytes: &[u8]| {
            let whole = bytes.len() as u64 == package.size
                && package
                    .sha256
                    .as_ref()
                    .is_none_or(|sha| *sha == hex(&Sha256::digest(bytes)));
            if whole {
                Ok(())
            } else {
                Err(SyncError::Mismatch {
                    pk3: package.pk3.clone(),
                })
            }
        };
        let bytes = get(&url, PACKAGE_TOTAL, Some(package.size), &valid, cancel, &mut report)?;
        write_whole(&dir.join(&package.pk3), &bytes)?;
        done += package.size;
        downloaded.push(package.pk3.clone());
    }
    // What the index no longer lists would still be mounted: it goes, as do
    // the pieces of transfers that never finished.
    let mut removed = Vec::new();
    for name in dropped {
        if fs::remove_file(dir.join(&name)).is_ok() {
            removed.push(name);
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".pk3.part") && fs::remove_file(entry.path()).is_ok() {
                removed.push(name);
            }
        }
    }
    // The extra files beside the packages (the music's playlists), before
    // the index, so a complete set includes them. They are optional: one
    // that cannot be fetched keeps the copy on disk.
    let mut extras_failed = Vec::new();
    for name in extras {
        let url = format!("{base}/{name}");
        match get(&url, SMALL_TOTAL, Some(EXTRA_MAX), &|_| Ok(()), cancel, &mut |_| {}) {
            Ok(bytes) => write_whole(&dir.join(name), &bytes)?,
            Err(SyncError::Cancelled) => return Err(SyncError::Cancelled),
            Err(_) => extras_failed.push((*name).to_owned()),
        }
    }
    write_whole(&synced, index_text.as_bytes())?;
    match fs::remove_file(&set_aside) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(&e, &set_aside)),
        _ => {}
    }
    Ok(SyncReport {
        index,
        downloaded,
        removed,
        extras_failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_names_stay_in_the_directory() {
        assert_eq!(index_file_name(None).as_deref(), Some("pk3_index.json"));
        assert_eq!(
            index_file_name(Some("pk3_index.1a2b.json")).as_deref(),
            Some("pk3_index.1a2b.json")
        );
        assert_eq!(index_file_name(Some("../pk3_index.json")), None);
        assert_eq!(index_file_name(Some("pk3_index./x.json")), None);
        assert_eq!(index_file_name(Some("pk3_index..json")), None);
    }

    #[test]
    fn a_package_on_disk_is_checked_by_size_and_digest() {
        let dir = std::env::temp_dir().join(format!("shagger-package-store-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.pk3");
        fs::write(&path, b"abc").unwrap();
        let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(file_sha256(&path).as_deref(), Some(sha));
        assert!(up_to_date(&path, 3, Some(sha),));
        assert!(!up_to_date(&path, 4, Some(sha)));
        // The same size with other bytes is fetched again.
        assert!(!up_to_date(&path, 3, Some(&"0".repeat(64))));
        assert!(up_to_date(&path, 3, None));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_whole_set_counts_as_synced() {
        let dir = std::env::temp_dir().join(format!("shagger-synced-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(!is_synced(&dir));
        let index = serde_json::json!({"packages": [{"pk3": "a.pk3", "size": 3}, {"pk3": "b.pk3", "size": 2}]});
        fs::write(dir.join(SYNCED_INDEX), index.to_string()).unwrap();
        write_whole(&dir.join("a.pk3"), b"abc").unwrap();
        // b.pk3 never arrived.
        assert!(!is_synced(&dir));
        write_whole(&dir.join("b.pk3"), b"x").unwrap();
        // b.pk3 is short.
        assert!(!is_synced(&dir));
        write_whole(&dir.join("b.pk3"), b"xy").unwrap();
        assert!(is_synced(&dir));
        assert!(!dir.join("b.pk3.part").exists());
        assert_eq!(listed(Some(&index)), ["a.pk3", "b.pk3"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn passing_failures_are_tried_again() {
        assert!(retryable(&FetchError::Status(503)));
        assert!(retryable(&FetchError::Status(429)));
        assert!(retryable(&FetchError::Stalled));
        assert!(!retryable(&FetchError::Status(404)));
        assert!(!retryable(&FetchError::Cancelled));
    }

    use std::collections::HashMap;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::super::published_index::pk3_index_hash;

    type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

    /// An HTTP server answering each path from `files`, and counting the
    /// requests for `/bad` paths that are first answered with other bytes.
    fn serve(files: Files, spoil: Arc<Mutex<HashMap<String, u32>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 2048];
                let n = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                let line = String::from_utf8_lossy(&request[..n]).into_owned();
                let path = line
                    .split(' ')
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap()
                    .to_owned();
                let mut body = files.lock().unwrap().get(&path).cloned();
                if let (Some(b), Some(left)) = (&mut body, spoil.lock().unwrap().get_mut(&path))
                    && *left > 0
                {
                    *left -= 1;
                    b[0] ^= 0xff;
                }
                let (status, body) = body.map_or(("404 Not Found", Vec::new()), |b| ("200 OK", b));
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{addr}")
    }

    /// Publishes `packages` (name and bytes) as a deployment does.
    fn publish(files: &Files, packages: &[(&str, &[u8])]) {
        let entries: Vec<Value> = packages
            .iter()
            .map(|(pk3, bytes)| {
                serde_json::json!({"pk3": pk3, "size": bytes.len(), "fileCount": 1, "sha256": hex(&Sha256::digest(bytes))})
            })
            .collect();
        let mut parts: Vec<String> = packages.iter().map(|(pk3, b)| format!("{pk3}:{}", b.len())).collect();
        parts.sort();
        let index = serde_json::json!({
            "version": 2, "listChecksum": pk3_index_hash(&parts.join("|")), "lastUpdated": 1,
            "packages": entries, "files": {}, "shaders": {}, "maps": {},
        })
        .to_string();
        let hash = pk3_index_hash(&index);
        let manifest = serde_json::json!({
            "files": packages.iter().map(|(pk3, _)| *pk3).collect::<Vec<_>>(),
            "indexHash": hash, "indexFile": format!("pk3_index.{hash}.json"),
        });
        let mut files = files.lock().unwrap();
        files.clear();
        files.insert("/pk3_manifest.json".into(), manifest.to_string().into_bytes());
        files.insert(format!("/pk3_index.{hash}.json"), index.into_bytes());
        files.insert("/game.playlist.json".into(), b"[]".to_vec());
        for (pk3, bytes) in packages {
            files.insert(format!("/{pk3}"), bytes.to_vec());
        }
    }

    #[test]
    fn a_sync_stopped_half_way_still_prunes_and_spoiled_bodies_are_fetched_again() {
        let dir = std::env::temp_dir().join(format!("shagger-sync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let files: Files = Arc::default();
        let spoil: Arc<Mutex<HashMap<String, u32>>> = Arc::default();
        let base = serve(files.clone(), spoil.clone());
        let never = AtomicBool::new(false);
        let extras = ["game.playlist.json"];

        publish(&files, &[("a.pk3", b"aaaa"), ("old.pk3", b"oooo")]);
        sync_packages(&base, &dir, &extras, &never, &mut |_| {}).unwrap();
        assert!(is_synced(&dir));
        assert_eq!(fs::read(dir.join("game.playlist.json")).unwrap(), b"[]");

        // The next deployment drops old.pk3 and adds b.pk3, which is missing
        // from the server: the sync stops after setting the index aside.
        publish(&files, &[("a.pk3", b"aaaa"), ("b.pk3", b"bbbb")]);
        files.lock().unwrap().remove("/b.pk3");
        assert!(sync_packages(&base, &dir, &extras, &never, &mut |_| {}).is_err());
        assert!(!is_synced(&dir));
        assert!(has_packages(&dir));

        // b.pk3 arrives spoiled twice, then whole; old.pk3 is still pruned.
        publish(&files, &[("a.pk3", b"aaaa"), ("b.pk3", b"bbbb")]);
        spoil.lock().unwrap().insert("/b.pk3".into(), 2);
        let report = sync_packages(&base, &dir, &extras, &never, &mut |_| {}).unwrap();
        assert_eq!(report.downloaded, ["b.pk3"]);
        assert_eq!(report.removed, ["old.pk3"]);
        assert!(is_synced(&dir) && !dir.join(PREVIOUS_INDEX).exists());

        // A package damaged at the same size is fetched again.
        fs::write(dir.join("a.pk3"), b"aaaX").unwrap();
        let report = sync_packages(&base, &dir, &extras, &never, &mut |_| {}).unwrap();
        assert_eq!(report.downloaded, ["a.pk3"]);
        assert_eq!(fs::read(dir.join("a.pk3")).unwrap(), b"aaaa");

        // A second copy of the game waits its turn.
        let held = fs::OpenOptions::new().write(true).open(dir.join(LOCK_FILE)).unwrap();
        held.lock().unwrap();
        assert_eq!(
            sync_packages(&base, &dir, &extras, &never, &mut |_| {}).unwrap_err(),
            SyncError::Busy
        );
        drop(held);
        fs::remove_dir_all(&dir).unwrap();
    }
}
