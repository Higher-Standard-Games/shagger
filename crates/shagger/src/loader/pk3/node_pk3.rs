//! SHAG `src/engine/loader/pk3/NodePk3.ts`: packages read off disk into a
//! [`Pk3Loader`], for tools and machines that have the data folder.

use std::path::Path;

use super::pk3_loader::{Pk3Error, Pk3Loader, normalize_path};

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

/// Native: the `*.pk3` in `dir` that hold any of `paths`, read whole into
/// `loader`, in name order. Only each package's directory is read to
/// decide, so a screen that needs a few files (the lobby's music) does not
/// read every package. Returns how many were read.
pub fn ingest_pk3_dir_holding(loader: &mut Pk3Loader, dir: &Path, paths: &[String]) -> usize {
    let wanted: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
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
        .filter(|f| {
            let Ok(file) = std::fs::File::open(dir.join(f)) else {
                return false;
            };
            let Ok(zip) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
                return false;
            };
            zip.file_names()
                .filter_map(Result::ok)
                .any(|name| wanted.contains(&normalize_path(&name)))
        })
        .filter(|f| matches!(ingest_pk3_file(loader, dir, f), Ok(true)))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(files: &[&str]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for f in files {
            zip.start_file(*f, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(f.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn holding_reads_only_the_packages_with_the_files() {
        let dir = std::env::temp_dir().join(format!("shagger-holding-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a_maps.pk3"), package(&["maps/a.bsp"])).unwrap();
        std::fs::write(dir.join("b_music.pk3"), package(&["music/Track.mp3"])).unwrap();
        std::fs::write(dir.join("c_broken.pk3"), b"not a zip").unwrap();
        let mut loader = Pk3Loader::new();
        let read = ingest_pk3_dir_holding(&mut loader, &dir, &["music/track.mp3".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read, 1);
        assert_eq!(loader.package_names().collect::<Vec<_>>(), ["b_music.pk3"]);
        assert!(loader.has_file("music/Track.mp3"));
    }
}
