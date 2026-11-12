//! SHAG `src/lobby/PlayerIdentity.ts`: the rules for player keys and names,
//! so the client reads a pasted key and judges a name the way the lobby does.
//!
//! A key is 120 random bits written as 24 characters of Crockford base32. The
//! canonical form is what the server hashes; [`format_key`] is how players see
//! it. Keys are made by the lobby, so `encodeKey` is not ported.

pub const KEY_LENGTH: usize = 24;
const KEY_PREFIX: &str = "WASHOI";

/// What a deleted account is called everywhere. [`check_player_name`] keeps
/// anyone from picking it.
pub const DELETED_ACCOUNT_NAME: &str = "[deleted account]";

/// Why a name can't be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    /// `name_invalid`.
    Invalid,
    /// `name_reserved`.
    Reserved,
}

/// Crockford base32 (`[0-9A-HJKMNP-TV-Z]`): no I, L, O or U.
fn canonical_char(c: char) -> bool {
    c.is_ascii_digit() || (c.is_ascii_uppercase() && !matches!(c, 'I' | 'L' | 'O' | 'U'))
}

/// Upper case without whitespace and dashes (`toUpperCase().replace(/[\s-]/g, "")`).
fn squeezed(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

/// O read as 0, I and L as 1.
fn misread(c: char) -> char {
    match c {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    }
}

/// `normalizeKey`: the canonical key in whatever a player pasted, or `None`
/// if it can't be one. Ignores case, spaces, dashes and a leading WASHOI, and
/// reads O as 0 and I or L as 1.
#[must_use]
pub fn normalize_key(input: &str) -> Option<String> {
    let mut key = squeezed(input);
    if key.chars().count() == KEY_PREFIX.len() + KEY_LENGTH && key.starts_with(KEY_PREFIX) {
        key.drain(..KEY_PREFIX.len());
    }
    let key: String = key.chars().map(misread).collect();
    (key.chars().count() == KEY_LENGTH && key.chars().all(canonical_char)).then_some(key)
}

/// `keyCharacters`: the key characters in partly typed or pasted text,
/// canonical and at most [`KEY_LENGTH`] long.
#[must_use]
pub fn key_characters(input: &str) -> String {
    let mut text = squeezed(input);
    if text.starts_with(KEY_PREFIX) {
        text.drain(..KEY_PREFIX.len());
    }
    text.chars()
        .map(misread)
        .filter(|c| canonical_char(*c))
        .take(KEY_LENGTH)
        .collect()
}

/// Splits into groups of four (`match(/.{1,4}/g)`).
#[must_use]
pub fn groups_of_four(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(4).map(|c| c.iter().collect()).collect()
}

/// `formatKey`: a key the way players see it, `WASHOI-XXXX-XXXX-…`.
#[must_use]
pub fn format_key(key: &str) -> String {
    let canonical = normalize_key(key).unwrap_or_else(|| key.to_owned());
    std::iter::once(KEY_PREFIX.to_owned())
        .chain(groups_of_four(&canonical))
        .collect::<Vec<_>>()
        .join("-")
}

/// `NAME_PATTERN`: 2 to 16 letters, digits, underscores or dashes.
#[must_use]
pub fn name_pattern_matches(name: &str) -> bool {
    (2..=16).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `DEFAULT_NAME`: new accounts are named `Player_<number>`; nobody can pick
/// such a name.
fn is_default_name(name: &str) -> bool {
    name.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("player_"))
        && name.len() > 7
        && name[7..].bytes().all(|b| b.is_ascii_digit())
}

/// `checkPlayerName`: why a player can't take `name`, or `None` if they can
/// (unless someone already has it). `is_reserved` adds the application's own
/// reserved names.
#[must_use]
pub fn check_player_name(name: &str, is_reserved: Option<&dyn Fn(&str) -> bool>) -> Option<NameProblem> {
    if !name_pattern_matches(name) {
        return Some(NameProblem::Invalid);
    }
    if is_default_name(name) || is_reserved.is_some_and(|f| f(name)) {
        return Some(NameProblem::Reserved);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789ABCDEFGHJKMNPQ";

    #[test]
    fn a_pasted_key_is_read_as_the_lobby_reads_it() {
        assert_eq!(normalize_key(KEY).as_deref(), Some(KEY));
        assert_eq!(
            normalize_key("washoi-0123-4567-89ab-cdef-ghjk-mnpq").as_deref(),
            Some(KEY)
        );
        assert_eq!(normalize_key(" O123 4567 89AB CDEF GHJK MNPQ ").as_deref(), Some(KEY));
        assert_eq!(normalize_key("0i23456789ABCDEFGHJKMNPQ").as_deref(), Some(KEY));
        assert_eq!(normalize_key("0123456789ABCDEFGHJKMNPU"), None);
        assert_eq!(normalize_key("0123"), None);
        assert_eq!(normalize_key(""), None);
    }

    #[test]
    fn typed_characters_are_kept_canonical_and_short() {
        assert_eq!(key_characters("WASHOI-01o!l"), "0101");
        assert_eq!(key_characters(&format!("{KEY}XYZ")), KEY);
        assert_eq!(key_characters("u u"), "");
    }

    #[test]
    fn a_key_is_shown_in_groups() {
        assert_eq!(format_key(KEY), "WASHOI-0123-4567-89AB-CDEF-GHJK-MNPQ");
        assert_eq!(format_key("ab"), "WASHOI-ab");
    }

    #[test]
    fn names_are_judged_as_the_lobby_judges_them() {
        assert_eq!(check_player_name("Tarmo", None), None);
        assert_eq!(check_player_name("a", None), Some(NameProblem::Invalid));
        assert_eq!(check_player_name("seventeen_chars__", None), Some(NameProblem::Invalid));
        assert_eq!(check_player_name("no spaces", None), Some(NameProblem::Invalid));
        assert_eq!(
            check_player_name(DELETED_ACCOUNT_NAME, None),
            Some(NameProblem::Invalid)
        );
        assert_eq!(check_player_name("PLAYER_12", None), Some(NameProblem::Reserved));
        assert_eq!(check_player_name("Player_", None), None);
        let bots = |n: &str| n.eq_ignore_ascii_case("ramen");
        assert_eq!(check_player_name("Ramen", Some(&bots)), Some(NameProblem::Reserved));
    }
}
