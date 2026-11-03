//! Files written whole or not at all: the text goes to `<name>.part`, is
//! flushed to disk, then renamed over the file. A crash or power loss mid
//! write leaves the old file, never a cut-off one (the browser's
//! `localStorage` gives the same guarantee).

use std::io::Write;
use std::path::Path;

/// Writes `bytes` to `path` whole, making its folder if needed.
///
/// # Errors
/// The folder could not be made, or the write or rename failed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let mut part = path.as_os_str().to_os_string();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let mut file = std::fs::File::create(&part)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&part, path)
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
        assert!(!dir.join("deep").join("cvars.json.part").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
