//! The published PK3 index (SHAG `src/engine/loader/pk3/PK3PublishedIndex.ts`).
//!
//! The hash, and the parser with its refusal rules (RUST.md 20.2).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// `PK3_INDEX_VERSION`: the only index version a client accepts.
pub const PK3_INDEX_VERSION: u32 = 2;

/// `pk3IndexHash`: the manifest's `indexHash` and the index's `listChecksum`.
///
/// djb2 with XOR over UTF-16 code units, with JavaScript's 32-bit wrapping,
/// printed as unpadded lowercase hex. It is a cache-consistency check, not a
/// signature. Iterating bytes or `char`s instead of UTF-16 units only agrees
/// for ASCII.
#[must_use]
pub fn pk3_index_hash(text: &str) -> String {
    let mut hash: i32 = 5381;
    for unit in text.encode_utf16() {
        hash = (hash << 5).wrapping_add(hash) ^ i32::from(unit);
    }
    // `hash >>> 0` in JavaScript: the same 32 bits read as unsigned.
    format!("{:x}", hash.cast_unsigned())
}

/// `PK3IndexEntry`, as a published index lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pk3IndexEntry {
    /// The archive's content identity.
    pub sha256: Option<String>,
    /// The package's file name.
    pub pk3: String,
    /// Where it is fetched from: the base path and the name.
    pub path: String,
    pub file_count: u64,
    pub size: u64,
}

/// `PK3IndexData`: every file, shader and map, by the package that owns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pk3IndexData {
    pub list_checksum: String,
    pub files: BTreeMap<String, String>,
    pub shaders: BTreeMap<String, String>,
    pub maps: BTreeMap<String, String>,
    pub packages: Vec<Pk3IndexEntry>,
    pub last_updated: f64,
}

/// The manifest beside the index (`pk3_manifest.json`): its package names,
/// the index's hash and, on current deployments, the index's file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedManifest {
    pub files: Vec<Value>,
    pub index_hash: String,
    pub index_file: Option<String>,
}

/// `hasPublishedIndex`.
#[must_use]
pub fn has_published_index(manifest: &Value) -> Option<PublishedManifest> {
    let record = manifest.as_object()?;
    Some(PublishedManifest {
        files: record.get("files")?.as_array()?.clone(),
        index_hash: record.get("indexHash")?.as_str()?.to_owned(),
        index_file: record.get("indexFile").and_then(Value::as_str).map(str::to_owned),
    })
}

/// `PACKAGE_NAME`: `^[a-z0-9][a-z0-9_.-]*\.pk3$`, any case.
fn is_package_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".pk3") else {
        return false;
    };
    let mut chars = stem.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// `Number.isSafeInteger(value) && value >= 0`.
fn safe_count(value: Option<&Value>) -> Option<u64> {
    let n = value?.as_f64()?;
    (n.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&n)).then_some(n as u64)
}

/// `/^[a-f0-9]{64}$/`.
fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `listChecksum`: the hash of the sorted `name:size` list.
fn list_checksum(packages: &[Pk3IndexEntry]) -> String {
    let mut parts: Vec<String> = packages.iter().map(|p| format!("{}:{}", p.pk3, p.size)).collect();
    // `Array.prototype.sort`: by UTF-16 units, the same as bytes for these names.
    parts.sort();
    pk3_index_hash(&parts.join("|"))
}

/// `parsePublishedIndex`: the whole snapshot checked before anything uses
/// it, or none. The package URLs are built from `base_path`, never taken
/// from the index.
#[must_use]
pub fn parse_published_index(text: &str, manifest: &Value, base_path: &str) -> Option<Pk3IndexData> {
    let manifest = has_published_index(manifest)?;
    if pk3_index_hash(text) != manifest.index_hash {
        return None;
    }
    let mut names = BTreeSet::new();
    for filename in &manifest.files {
        let name = filename.as_str()?;
        if !is_package_name(name) || !names.insert(name.to_owned()) {
            return None;
        }
    }
    let data: Value = serde_json::from_str(text).ok()?;
    let data = data.as_object()?;
    let last_updated = data.get("lastUpdated")?.as_f64()?;
    if data.get("version")?.as_f64()? != f64::from(PK3_INDEX_VERSION)
        || data.get("listChecksum")?.as_str().is_none()
        || !last_updated.is_finite()
        || last_updated < 0.0
    {
        return None;
    }
    let mut packages = Vec::new();
    let mut indexed = BTreeSet::new();
    for entry in data.get("packages")?.as_array()? {
        let entry = entry.as_object()?;
        let pk3 = entry.get("pk3")?.as_str()?;
        if !names.contains(pk3) || indexed.contains(pk3) {
            return None;
        }
        let size = safe_count(entry.get("size"))?;
        let file_count = safe_count(entry.get("fileCount"))?;
        let sha256 = match entry.get("sha256") {
            None => None,
            Some(Value::String(s)) if is_sha256(s) => Some(s.clone()),
            // `""` is falsy: kept out like a missing digest.
            Some(Value::String(s)) if s.is_empty() => return None,
            Some(_) => return None,
        };
        indexed.insert(pk3.to_owned());
        packages.push(Pk3IndexEntry {
            sha256,
            pk3: pk3.to_owned(),
            path: format!("{base_path}{pk3}"),
            file_count,
            size,
        });
    }
    if indexed.len() != names.len() {
        return None;
    }
    let checksum = list_checksum(&packages);
    if data.get("listChecksum")?.as_str()? != checksum {
        return None;
    }
    let mut dictionaries = Vec::new();
    for key in ["files", "shaders", "maps"] {
        let mut dictionary = BTreeMap::new();
        for (name, owner) in data.get(key)?.as_object()? {
            let owner = owner.as_str()?;
            if name.is_empty() || *name != name.to_lowercase().replace('\\', "/") || !names.contains(owner) {
                return None;
            }
            dictionary.insert(name.clone(), owner.to_owned());
        }
        dictionaries.push(dictionary);
    }
    let maps = dictionaries.pop()?;
    let shaders = dictionaries.pop()?;
    let files = dictionaries.pop()?;
    for (map, owner) in &maps {
        if files.get(&format!("maps/{map}.bsp")) != Some(owner) {
            return None;
        }
    }
    Some(Pk3IndexData {
        list_checksum: checksum,
        files,
        shaders,
        maps,
        packages,
        last_updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-map deployment as the publisher writes it.
    fn deployment() -> (String, Value) {
        let packages = [("map_a.pk3", 10, "a"), ("tex_a.pk3", 20, "b")];
        let entries: Vec<Value> = packages
            .iter()
            .map(|(pk3, size, digit)| {
                serde_json::json!({"pk3": pk3, "size": size, "fileCount": 1, "sha256": digit.repeat(64)})
            })
            .collect();
        let checksum = pk3_index_hash("map_a.pk3:10|tex_a.pk3:20");
        let index = serde_json::json!({
            "version": 2,
            "listChecksum": checksum,
            "lastUpdated": 1,
            "packages": entries,
            "files": {"maps/a.bsp": "map_a.pk3", "textures/a/wall.webp": "tex_a.pk3"},
            "shaders": {"textures/a/wall": "tex_a.pk3"},
            "maps": {"a": "map_a.pk3"},
        })
        .to_string();
        let manifest = serde_json::json!({
            "files": ["map_a.pk3", "tex_a.pk3"],
            "indexHash": pk3_index_hash(&index),
            "indexFile": format!("pk3_index.{}.json", pk3_index_hash(&index)),
        });
        (index, manifest)
    }

    #[test]
    fn a_deployment_parses() {
        let (index, manifest) = deployment();
        let data = parse_published_index(&index, &manifest, "/data/").unwrap();
        assert_eq!(data.packages.len(), 2);
        assert_eq!(data.packages[0].path, "/data/map_a.pk3");
        assert_eq!(data.packages[1].sha256.as_deref(), Some(&*"b".repeat(64)));
        assert_eq!(data.maps["a"], "map_a.pk3");
        assert_eq!(
            has_published_index(&manifest).unwrap().index_file.unwrap(),
            format!("pk3_index.{}.json", pk3_index_hash(&index))
        );
    }

    #[test]
    fn anything_off_refuses_the_snapshot() {
        let (index, manifest) = deployment();
        // The text changed after the manifest was written.
        assert!(parse_published_index(&index.replace("wall", "wal"), &manifest, "").is_none());
        // A name that could leave the directory.
        let mut bad = manifest.clone();
        bad["files"][0] = Value::from("../map_a.pk3");
        assert!(parse_published_index(&index, &bad, "").is_none());
        // A package the index lists twice, or not at all.
        let mut short = manifest.clone();
        short["files"] = serde_json::json!(["map_a.pk3"]);
        assert!(parse_published_index(&index, &short, "").is_none());
        assert!(!is_package_name("A.pk3x"));
        assert!(is_package_name("Tex_A-1.pk3"));
        assert!(!is_sha256(&"A".repeat(64)));
    }
}
