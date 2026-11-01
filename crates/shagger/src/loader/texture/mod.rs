//! Textures (SHAG `src/engine/loader/texture`).

pub mod texture_extensions;

use super::pk3::pk3_loader::Pk3Loader;
use texture_extensions::texture_candidates;

/// A decoded texture: RGBA8, rows top to bottom.
#[derive(Debug, Clone)]
pub struct DecodedTexture {
    /// The package file it came from.
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The file behind a texture name, tried in `resolveAndLoadTexture`'s order
/// (see [`texture_candidates`]), decoded by its content rather than its
/// extension, as `PK3TextureLoader` sniffs magic bytes. KTX (ETC1) is not
/// decoded natively (RUST.md 20.6), so it only counts when nothing else is
/// there, and then gives `None`.
#[must_use]
pub fn resolve_and_load_texture(pk3: &mut Pk3Loader, base_path: &str) -> Option<DecodedTexture> {
    for candidate in texture_candidates(base_path) {
        if !pk3.has_file(&candidate) {
            continue;
        }
        if candidate.to_lowercase().ends_with(".ktx") {
            return None;
        }
        let bytes = pk3.get_file(&candidate)?;
        let decoded = decode_image(&candidate, &bytes)?;
        return Some(DecodedTexture {
            path: candidate,
            ..decoded
        });
    }
    None
}

/// Decodes WebP, TGA, PNG or JPEG bytes. TGA has no magic number, so it is
/// tried by name when the content is not recognized.
#[must_use]
pub fn decode_image(name: &str, bytes: &[u8]) -> Option<DecodedTexture> {
    let format = image::guess_format(bytes)
        .ok()
        .or_else(|| name.to_lowercase().ends_with(".tga").then_some(image::ImageFormat::Tga))?;
    let image = image::load_from_memory_with_format(bytes, format).ok()?.into_rgba8();
    Some(DecodedTexture {
        path: name.to_owned(),
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}
