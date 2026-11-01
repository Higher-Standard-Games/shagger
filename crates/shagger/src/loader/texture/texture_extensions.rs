//! SHAG `src/engine/loader/texture/TextureExtensions.ts`: texture names and
//! the files behind them. Shaders and models name textures with their source
//! extension (`foo.tga`), as Quake does, but map texture packages carry their
//! TGA and PNG sources as WebP under the same name, so the WebP form is looked
//! for first.

/// `PACKAGED_TEXTURE_EXTENSION`.
pub const PACKAGED_TEXTURE_EXTENSION: &str = ".webp";

/// `TEXTURE_EXTENSION_PATTERN`: `/\.(webp|ktx|tga|png|jpe?g)$/i`.
const EXTENSIONS: [&str; 6] = [".webp", ".ktx", ".tga", ".png", ".jpg", ".jpeg"];

/// `stripTextureExtension`.
#[must_use]
pub fn strip_texture_extension(path: &str) -> &str {
    for ext in EXTENSIONS {
        if path.len() >= ext.len() {
            let (head, tail) = path.split_at(path.len() - ext.len());
            if tail.eq_ignore_ascii_case(ext) {
                return head;
            }
        }
    }
    path
}

/// `packagedTexturePath`.
#[must_use]
pub fn packaged_texture_path(path: &str) -> String {
    format!("{}{PACKAGED_TEXTURE_EXTENSION}", strip_texture_extension(path))
}

/// The files `PK3TextureLoader.resolveAndLoadTexture` tries for a texture
/// name, in order, without duplicates (compared in lower case): the packaged
/// WebP, the name as given, then TGA, PNG, JPG and the lossy ETC1 KTX last.
/// The KTX2 sidecar it checks first is not shipped (RUST.md 20.9).
#[must_use]
pub fn texture_candidates(base_path: &str) -> Vec<String> {
    let clean = base_path.strip_prefix('/').unwrap_or(base_path);
    let stripped = strip_texture_extension(clean);
    let all = [
        format!("{stripped}{PACKAGED_TEXTURE_EXTENSION}"),
        clean.to_owned(),
        format!("{stripped}.tga"),
        format!("{stripped}.png"),
        format!("{stripped}.jpg"),
        format!("{stripped}.ktx"),
    ];
    let mut seen = std::collections::HashSet::new();
    all.into_iter().filter(|c| seen.insert(c.to_lowercase())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_the_typescript_order() {
        assert_eq!(strip_texture_extension("textures/a/b.TGA"), "textures/a/b");
        assert_eq!(strip_texture_extension("textures/a/b.jpeg"), "textures/a/b");
        assert_eq!(strip_texture_extension("textures/a/b.bmp"), "textures/a/b.bmp");
        assert_eq!(
            texture_candidates("/textures/a/b.tga"),
            [
                "textures/a/b.webp",
                "textures/a/b.tga",
                "textures/a/b.png",
                "textures/a/b.jpg",
                "textures/a/b.ktx"
            ]
        );
        assert_eq!(
            texture_candidates("textures/a/b"),
            [
                "textures/a/b.webp",
                "textures/a/b",
                "textures/a/b.tga",
                "textures/a/b.png",
                "textures/a/b.jpg",
                "textures/a/b.ktx"
            ]
        );
    }
}
