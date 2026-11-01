//! SHAG `src/engine/loader/model/TagsLoader.ts`: `tags.txt` attachment tags.
//!
//! One tag per line: `tag "<bone>" "<tag_name>" <fwd> <right> <up> <pitch>
//! <yaw> <roll>`; `//` starts a comment. An empty bone means the model root.

use glam::Vec3;

#[derive(Debug, Clone, PartialEq)]
pub struct TagDef {
    /// Bone name in the skeleton (empty: the model root).
    pub bone_name: String,
    /// Tag identifier (`tag_barrel`, `tag_weapon`).
    pub tag_name: String,
    /// Position offset: forward, right, up.
    pub offset: Vec3,
    /// Rotation offset: pitch, yaw, roll, in radians.
    pub rotation: Vec3,
}

/// `parseFloat` over a `[-\d.]+` match: the longest prefix that reads as a number.
fn parse_float(text: &str) -> f32 {
    (1..=text.len())
        .rev()
        .find_map(|end| text[..end].parse::<f64>().ok())
        .map_or(f32::NAN, |v| v as f32)
}

/// A quoted field at the start of `rest`: its text and what follows it.
fn quoted(rest: &str, allow_empty: bool) -> Option<(&str, &str)> {
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    if end == 0 && !allow_empty {
        return None;
    }
    Some((&rest[..end], &rest[end + 1..]))
}

/// `parseTags`.
#[must_use]
pub fn parse_tags(content: &str) -> Vec<TagDef> {
    let mut tags = Vec::new();
    for raw_line in content.split('\n') {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let Some(rest) = line.strip_prefix("tag") else { continue };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((bone, rest)) = quoted(rest.trim_start(), true) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((name, rest)) = quoted(rest.trim_start(), false) else {
            continue;
        };
        // Six `[-\d.]+` fields, each after whitespace.
        let mut numbers = Vec::with_capacity(6);
        let mut rest = rest;
        for _ in 0..6 {
            if !rest.starts_with(char::is_whitespace) {
                break;
            }
            let trimmed = rest.trim_start();
            let end = trimmed
                .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.'))
                .unwrap_or(trimmed.len());
            if end == 0 {
                break;
            }
            numbers.push(parse_float(&trimmed[..end]));
            rest = &trimmed[end..];
        }
        if numbers.len() != 6 {
            continue;
        }
        let radians = |d: f32| (f64::from(d) * std::f64::consts::PI / 180.0) as f32;
        tags.push(TagDef {
            bone_name: bone.to_owned(),
            tag_name: name.to_owned(),
            offset: Vec3::new(numbers[0], numbers[1], numbers[2]),
            rotation: Vec3::new(radians(numbers[3]), radians(numbers[4]), radians(numbers[5])),
        });
    }
    tags
}

/// `findTag`.
#[must_use]
pub fn find_tag<'a>(tags: &'a [TagDef], tag_name: &str) -> Option<&'a TagDef> {
    tags.iter().find(|t| t.tag_name == tag_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_plasmagun_tags() {
        let text = "// comment\n\n// Plasmagun has \"tag_barrel2\"\ntag \"tag_barrel2\" \"tag_barrel\" 0 0 0 0 0 0\n";
        let tags = parse_tags(text);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].bone_name, "tag_barrel2");
        assert_eq!(find_tag(&tags, "tag_barrel").map(|t| t.offset), Some(Vec3::ZERO));
    }

    #[test]
    fn reads_offsets_and_degrees() {
        let tags =
            parse_tags("  tag \"\" \"tag_weapon\" -2 1.5 0.25 90 0 -180  \nnot a tag\ntag \"a\" \"\" 0 0 0 0 0 0");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].offset, Vec3::new(-2.0, 1.5, 0.25));
        assert!((tags[0].rotation.x - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((tags[0].rotation.z + std::f32::consts::PI).abs() < 1e-6);
    }
}
