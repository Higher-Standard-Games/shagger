//! The file side of SHAG `src/engine/loader/pk3/PK3Loader.ts`: packages held
//! in memory and one index from every file path to the package that wins it.
//! Downloading, the published index and package scopes come later
//! (RUST.md 20.2); `NodePk3.ts` reads packages off disk the same way.
//!
//! Paths are matched as the TypeScript matches them: lower case, with `\`
//! turned into `/`. Packages are ordered by file name with `localeCompare`
//! and a later package overrides an earlier one (Quake 3's rule: `z_*.pk3`
//! beats `data*.pk3`).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{Cursor, Read};

use zip::ZipArchive;

/// Why a package could not be read.
#[derive(Debug)]
pub struct Pk3Error(pub String);

impl std::fmt::Display for Pk3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Pk3Error {}

/// `path.toLowerCase().replace(/\\/g, "/")`.
#[must_use]
pub fn normalize_path(path: &str) -> String {
    path.to_lowercase().replace('\\', "/")
}

/// Where an ASCII character sorts under ICU's root collation, which is what
/// `String.prototype.localeCompare` uses in V8: spaces, then punctuation and
/// symbols in ICU's order, then digits, then letters with case only breaking
/// ties. Package names are ASCII; anything else sorts after, by code point.
fn collation_weight(c: char) -> (u32, u32) {
    const PUNCTUATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if let Some(i) = PUNCTUATION.find(c) {
        return (i as u32, 0);
    }
    let base = PUNCTUATION.len() as u32;
    match c {
        '0'..='9' => (base + (c as u32 - '0' as u32), 0),
        'a'..='z' => (base + 10 + (c as u32 - 'a' as u32), 0),
        'A'..='Z' => (base + 10 + (c as u32 - 'A' as u32), 1),
        _ => (base + 36 + c as u32, 0),
    }
}

/// `a.localeCompare(b)` for package file names.
#[must_use]
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let primary = |s: &str| s.chars().map(|c| collation_weight(c).0).collect::<Vec<_>>();
    let tertiary = |s: &str| s.chars().map(|c| collation_weight(c).1).collect::<Vec<_>>();
    primary(a).cmp(&primary(b)).then_with(|| tertiary(a).cmp(&tertiary(b)))
}

struct Package {
    filename: String,
    zip: ZipArchive<Cursor<Vec<u8>>>,
    /// Normalized path → the entry's index in the archive.
    files: HashMap<String, usize>,
    /// The normalized paths in archive order (the TypeScript's `files` Set).
    order: Vec<String>,
}

/// The loaded packages and the merged file index.
#[derive(Default)]
pub struct Pk3Loader {
    packages: Vec<Package>,
    /// Normalized path → index into `packages`.
    file_index: HashMap<String, usize>,
    /// The index's keys in the order a JavaScript `Map` keeps them: first
    /// insertion, packages in priority order, files in archive order.
    file_order: Vec<String>,
}

impl Pk3Loader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `processPK3ArrayBuffer`: open a package and rebuild the index. A
    /// package with the same file name replaces the earlier one.
    ///
    /// # Errors
    /// When the bytes are not a zip archive.
    pub fn add_package(&mut self, filename: &str, bytes: Vec<u8>) -> Result<(), Pk3Error> {
        let zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| Pk3Error(format!("{filename}: {e}")))?;
        let mut files = HashMap::new();
        let mut order = Vec::new();
        // `file_names` runs in entry order; a name that is not valid text is skipped.
        for (index, name) in zip.file_names().enumerate() {
            let Ok(name) = name else { continue };
            let normalized = normalize_path(&name);
            if !normalized.ends_with('/') && files.insert(normalized.clone(), index).is_none() {
                order.push(normalized);
            }
        }
        self.packages.retain(|p| p.filename != filename);
        self.packages.push(Package {
            filename: filename.to_owned(),
            zip,
            files,
            order,
        });
        self.rebuild_file_index();
        Ok(())
    }

    /// `rebuildFileIndex`.
    fn rebuild_file_index(&mut self) {
        self.packages.sort_by(|a, b| locale_compare(&a.filename, &b.filename));
        self.file_index.clear();
        self.file_order.clear();
        for (i, package) in self.packages.iter().enumerate() {
            for path in &package.order {
                if self.file_index.insert(path.clone(), i).is_none() {
                    self.file_order.push(path.clone());
                }
            }
        }
    }

    /// The package file names, lowest priority first.
    pub fn package_names(&self) -> impl Iterator<Item = &str> {
        self.packages.iter().map(|p| p.filename.as_str())
    }

    /// `hasFile`.
    #[must_use]
    pub fn has_file(&self, path: &str) -> bool {
        self.file_index.contains_key(&normalize_path(path))
    }

    /// `getPackageForFile`: the package that serves `path`.
    #[must_use]
    pub fn package_for_file(&self, path: &str) -> Option<&str> {
        let i = *self.file_index.get(&normalize_path(path))?;
        Some(&self.packages[i].filename)
    }

    /// Every indexed path (normalized), in the TypeScript's `Map` order.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.file_order.iter().map(String::as_str)
    }

    /// `listFiles(pattern)`: the paths matching a glob, in [`Self::files`]
    /// order, case-insensitively: `*` is any run of characters, `?` any one,
    /// and everything else is literal.
    #[must_use]
    pub fn list_files(&self, pattern: &str) -> Vec<&str> {
        let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
        self.files()
            .filter(|f| glob_match(&pattern, &f.to_lowercase().chars().collect::<Vec<_>>()))
            .collect()
    }

    /// `getFile`: the bytes of `path` from the package that wins it.
    #[must_use]
    pub fn get_file(&mut self, path: &str) -> Option<Vec<u8>> {
        let normalized = normalize_path(path);
        let i = *self.file_index.get(&normalized)?;
        let package = &mut self.packages[i];
        let index = *package.files.get(&normalized)?;
        let mut entry = package.zip.by_index(index).ok()?;
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

/// `^pattern$` with `*` as any run of characters and `?` as any one.
fn glob_match(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| glob_match(rest, &text[skip..])),
        Some((&p, rest)) => text.first().is_some_and(|&t| p == '?' || p == t) && glob_match(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_compare_puts_punctuation_before_digits_and_letters() {
        let mut names = vec![
            "washoi_weapons.pk3",
            "washoi.pk3",
            "a1.pk3",
            "a_1.pk3",
            "B.pk3",
            "a.pk3",
            "tex_washoi_dm2.pk3",
            "map_washoi_dm2.pk3",
        ];
        names.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(
            names,
            [
                "a_1.pk3",
                "a.pk3",
                "a1.pk3",
                "B.pk3",
                "map_washoi_dm2.pk3",
                "tex_washoi_dm2.pk3",
                "washoi_weapons.pk3",
                "washoi.pk3"
            ]
        );
        assert_eq!(locale_compare("a.pk3", "A.pk3"), Ordering::Less);
    }

    #[test]
    fn globs_read_only_star_and_question_mark_as_wildcards() {
        let m = |p: &str, t: &str| glob_match(&p.chars().collect::<Vec<_>>(), &t.chars().collect::<Vec<_>>());
        assert!(m("scripts/*.shader", "scripts/a.shader"));
        assert!(m("scripts/*.shader", "scripts/sub/a.shader"));
        assert!(!m("scripts/*.shader", "scripts/xshader"));
        assert!(m("maps/dm?.bsp", "maps/dm5.bsp"));
        assert!(m("a+(b).txt", "a+(b).txt"));
        assert!(!m("a+(b).txt", "aa(b)xtxt"));
        assert!(!m("scripts/*.shader", "scripts/a.shader.txt"));
        assert!(!m("maps/*.bsp", "levelshots/a.bsp"));
    }
}
