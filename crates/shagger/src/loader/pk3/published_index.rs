//! The published PK3 index (SHAG `src/engine/loader/pk3/PK3PublishedIndex.ts`).
//!
//! So far only the hash. The parser and its refusal rules follow (RUST.md 20.2).

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
