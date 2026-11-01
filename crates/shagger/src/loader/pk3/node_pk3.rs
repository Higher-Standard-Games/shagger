//! SHAG `src/engine/loader/pk3/NodePk3.ts`: packages read off disk into a
//! [`Pk3Loader`], for tools and machines that have the data folder.

use std::path::Path;

use super::pk3_loader::{Pk3Error, Pk3Loader};

/// `ingestPk3File`: one package from `dir`. `Ok(false)` when it is not there.
///
/// # Errors
/// When the file cannot be read or is not a zip archive.
pub fn ingest_pk3_file(loader: &mut Pk3Loader, dir: &Path, pk3_file: &str) -> Result<bool, Pk3Error> {
    let full = dir.join(pk3_file);
    if !full.exists() {
        return Ok(false);
    }
    let bytes = std::fs::read(&full).map_err(|e| Pk3Error(format!("{}: {e}", full.display())))?;
    loader.add_package(pk3_file, bytes)?;
    Ok(true)
}

/// `ingestPk3Dir`: every `*.pk3` in `dir`, in name order. A package that
/// fails is skipped, as the TypeScript warns and goes on. Returns how many
/// were read.
#[must_use]
pub fn ingest_pk3_dir(loader: &mut Pk3Loader, dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| f.ends_with(".pk3"))
        .collect();
    files.sort();
    files
        .iter()
        .filter(|f| matches!(ingest_pk3_file(loader, dir, f), Ok(true)))
        .count()
}
