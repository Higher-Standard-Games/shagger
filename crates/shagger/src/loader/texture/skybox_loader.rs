//! SHAG `src/engine/loader/texture/SkyboxLoader.ts`: the six-face far box of
//! a Quake 3 or Warsow sky, as `loadSkybox` reads it from the packages and
//! lays it out for Babylon's `RawCubeTexture`.
//!
//! The faces come out in the slot order `loadSkybox` hands Babylon, which
//! uploads slot `i` to `TEXTURE_CUBE_MAP_POSITIVE_X + i` (+X, −X, +Y, −Y, +Z,
//! −Z), and a renderer that uploads them to cube layers 0 to 5 and samples
//! them with the Babylon-space direction (`q3ToBabylon`) shows what SHAG
//! shows. The cloud layers, the sky sphere and the debug box are not ported
//! yet: no shipped map draws them.

use super::ktx1_loader::{can_software_decode, gl_formats as gl, parse_ktx1, software_decode_mip};
use super::texture_extensions::PACKAGED_TEXTURE_EXTENSION;
use crate::loader::pk3::pk3_loader::Pk3Loader;

/// `Q3_FACE_SUFFIXES`, in Quake 3's `tr_shader.c` order.
pub const Q3_FACE_SUFFIXES: [&str; 6] = ["_rt", "_bk", "_lf", "_ft", "_up", "_dn"];

/// `FaceTransform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceTransform {
    None,
    FlipH,
    FlipV,
    Rotate180,
    Rotate90Cw,
    Rotate90Ccw,
    Rotate90CwFlipH,
    Rotate90CcwFlipH,
}

/// `Q3_TO_BABYLON_FACE_MAP`: for each cube slot, the Quake face (an index
/// into [`Q3_FACE_SUFFIXES`]) and how it is turned.
pub const Q3_TO_BABYLON_FACE_MAP: [(usize, FaceTransform); 6] = [
    (2, FaceTransform::FlipH),
    (0, FaceTransform::FlipH),
    (4, FaceTransform::Rotate90CcwFlipH),
    (5, FaceTransform::Rotate90CwFlipH),
    (1, FaceTransform::FlipH),
    (3, FaceTransform::FlipH),
];

/// Decoded face pixels: RGBA8, `size` taken from the width as the
/// TypeScript does (faces are assumed square).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFace {
    pub data: Vec<u8>,
    pub size: u32,
}

/// A byte of a typed array: out of range reads as 0 (`undefined` stored
/// into a `Uint8Array`).
fn at(data: &[u8], i: usize) -> u8 {
    data.get(i).copied().unwrap_or(0)
}

/// `scaleFaceTo`: an undersized face brought up to the cube's face size by
/// nearest neighbour.
#[must_use]
pub fn scale_face_to(data: &[u8], size: u32, target: u32) -> Vec<u8> {
    if size == target || size == 0 {
        return data.to_vec();
    }
    let (size, target) = (size as usize, target as usize);
    let mut scaled = vec![0u8; target * target * 4];
    for y in 0..target {
        let src_y = (size - 1).min(y * size / target);
        for x in 0..target {
            let src_x = (size - 1).min(x * size / target);
            let src = (src_y * size + src_x) * 4;
            let dst = (y * target + x) * 4;
            for c in 0..4 {
                scaled[dst + c] = at(data, src + c);
            }
        }
    }
    scaled
}

/// `transformFace`.
#[must_use]
pub fn transform_face(data: &[u8], size: u32, transform: FaceTransform) -> Vec<u8> {
    if transform == FaceTransform::None {
        return data.to_vec();
    }
    let size = size as usize;
    let mut result = vec![0u8; data.len()];
    for y in 0..size {
        for x in 0..size {
            let src = (y * size + x) * 4;
            let (dx, dy) = match transform {
                FaceTransform::None => (x, y),
                FaceTransform::FlipH => (size - 1 - x, y),
                FaceTransform::FlipV => (x, size - 1 - y),
                FaceTransform::Rotate180 => (size - 1 - x, size - 1 - y),
                FaceTransform::Rotate90Cw => (size - 1 - y, x),
                FaceTransform::Rotate90Ccw => (y, size - 1 - x),
                FaceTransform::Rotate90CwFlipH => (y, x),
                FaceTransform::Rotate90CcwFlipH => (size - 1 - y, size - 1 - x),
            };
            let dst = (dy * size + dx) * 4;
            for c in 0..4 {
                // A typed array ignores writes past its end.
                if let Some(out) = result.get_mut(dst + c) {
                    *out = at(data, src + c);
                }
            }
        }
    }
    result
}

/// `parseImageToRGBA`: by the extension the file was found under.
#[must_use]
pub fn parse_image_to_rgba(data: &[u8], ext: &str) -> Option<RgbaFace> {
    match ext {
        ".tga" => parse_tga_to_rgba(data),
        ".ktx" => parse_ktx_to_rgba(data),
        _ => parse_standard_image_to_rgba(data, ext),
    }
}

/// `parseKTXToRGBA`: the base level, rows as stored (no flip, unlike the
/// world texture path).
fn parse_ktx_to_rgba(data: &[u8]) -> Option<RgbaFace> {
    let ktx = parse_ktx1(data).ok()?;
    let mip = ktx.mip_levels.first()?;
    let pixel_count = (mip.width * mip.height) as usize;
    let d = mip.data;
    if !ktx.is_compressed {
        let base = ktx.header.gl_base_internal_format;
        let rgba = if base == gl::RGBA && d.len() >= pixel_count * 4 {
            d.to_vec()
        } else if base == gl::RGB {
            (0..pixel_count)
                .flat_map(|i| [at(d, i * 3), at(d, i * 3 + 1), at(d, i * 3 + 2), 255])
                .collect()
        } else if base == gl::LUMINANCE {
            (0..pixel_count)
                .flat_map(|i| [at(d, i), at(d, i), at(d, i), 255])
                .collect()
        } else {
            d.to_vec()
        };
        return Some(RgbaFace {
            data: rgba,
            size: mip.width,
        });
    }
    let format = ktx.header.gl_internal_format;
    if !can_software_decode(format) {
        return None;
    }
    Some(RgbaFace {
        data: software_decode_mip(d, mip.width, mip.height, format),
        size: mip.width,
    })
}

/// `parseTGAToRGBA`: uncompressed and RLE true colour and grey, rows top to
/// bottom.
fn parse_tga_to_rgba(data: &[u8]) -> Option<RgbaFace> {
    if data.len() < 18 {
        return None;
    }
    let id_length = data[0] as usize;
    let color_map_type = data[1];
    let image_type = data[2];
    let width = u16::from_le_bytes([data[12], data[13]]) as usize;
    let height = u16::from_le_bytes([data[14], data[15]]) as usize;
    let bpp = data[16];
    let descriptor = data[17];
    if ![2, 3, 10, 11].contains(&image_type) {
        return None;
    }
    let grey = image_type == 3 || image_type == 11;
    if color_map_type != 0 {
        return None;
    }
    let bytes_per_pixel = (bpp / 8) as usize;
    if grey {
        if bytes_per_pixel != 1 && bytes_per_pixel != 2 {
            return None;
        }
    } else if bytes_per_pixel != 3 && bytes_per_pixel != 4 {
        return None;
    }
    let mut src = 18 + id_length;
    let mut pixels = vec![0u8; width * height * 4];
    let top_to_bottom = descriptor & 0x20 != 0;
    // One source pixel as RGBA.
    let read = |o: usize| -> [u8; 4] {
        if grey {
            let lum = at(data, o);
            [lum, lum, lum, if bytes_per_pixel == 2 { at(data, o + 1) } else { 255 }]
        } else {
            [
                at(data, o + 2),
                at(data, o + 1),
                at(data, o),
                if bytes_per_pixel == 4 { at(data, o + 3) } else { 255 },
            ]
        }
    };
    let mut put = |index: usize, rgba: [u8; 4]| {
        let (y, x) = (index / width, index % width);
        let dest_y = if top_to_bottom { y } else { height - 1 - y };
        let d = (dest_y * width + x) * 4;
        pixels[d..d + 4].copy_from_slice(&rgba);
    };
    let total = width * height;
    if image_type == 2 || image_type == 3 {
        for index in 0..total {
            put(index, read(src));
            src += bytes_per_pixel;
        }
    } else {
        let mut index = 0;
        while index < total {
            let packet = at(data, src);
            src += 1;
            let count = (packet & 0x7f) as usize + 1;
            if packet & 0x80 != 0 {
                let rgba = read(src);
                src += bytes_per_pixel;
                for _ in 0..count {
                    // Past the last pixel a typed array write is dropped.
                    if index < total {
                        put(index, rgba);
                    }
                    index += 1;
                }
            } else {
                for _ in 0..count {
                    if index < total {
                        put(index, read(src));
                    }
                    src += bytes_per_pixel;
                    index += 1;
                }
            }
        }
    }
    Some(RgbaFace {
        data: pixels,
        size: width as u32,
    })
}

/// `parseStandardImageToRGBA`: JPEG, PNG and WebP, which the browser decodes.
fn parse_standard_image_to_rgba(data: &[u8], ext: &str) -> Option<RgbaFace> {
    if ![".jpg", ".jpeg", ".png", ".webp"].contains(&ext) {
        return None;
    }
    // The browser sniffs the content, whatever the MIME type says.
    let image = image::load_from_memory(data).ok()?.into_rgba8();
    Some(RgbaFace {
        size: image.width(),
        data: image.into_raw(),
    })
}

/// A far box ready for a cube texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyboxFaces {
    /// Every face's size.
    pub size: u32,
    /// The faces in cube slot order (+X, −X, +Y, −Y, +Z, −Z).
    pub faces: [Vec<u8>; 6],
    /// The file each Quake face came from, in [`Q3_FACE_SUFFIXES`] order.
    pub loaded_faces: Vec<String>,
}

/// The extensions `loadSkyboxFace` tries, in order.
const FACE_EXTENSIONS: [&str; 6] = [PACKAGED_TEXTURE_EXTENSION, ".ktx", ".tga", ".jpg", ".png", ".jpeg"];

/// `loadSkyboxFace`: the first file under either prefix and any extension
/// that decodes.
#[must_use]
pub fn load_skybox_face(pk3: &mut Pk3Loader, base_path: &str, suffix: &str) -> Option<(RgbaFace, String)> {
    for prefix in ["", "textures/"] {
        for ext in FACE_EXTENSIONS {
            let path = format!("{prefix}{base_path}{suffix}{ext}");
            let Some(bytes) = pk3.get_file(&path) else { continue };
            if let Some(rgba) = parse_image_to_rgba(&bytes, ext) {
                return Some((rgba, path));
            }
        }
    }
    None
}

/// `loadSkybox` up to the cube texture: all six faces, each falling back to
/// the unsuffixed image (Warsow's `skyparmssides`), brought to the largest
/// face's size and turned into Babylon's slots.
pub fn load_skybox(pk3: &mut Pk3Loader, base_path: &str) -> Result<SkyboxFaces, String> {
    let mut faces = Vec::with_capacity(6);
    let mut loaded_faces = Vec::with_capacity(6);
    for suffix in Q3_FACE_SUFFIXES {
        let Some((face, path)) =
            load_skybox_face(pk3, base_path, suffix).or_else(|| load_skybox_face(pk3, base_path, ""))
        else {
            return Err(format!("Failed to load skybox face: {base_path}{suffix}"));
        };
        faces.push(face);
        loaded_faces.push(path);
    }
    let size = faces.iter().map(|f| f.size).max().unwrap_or(0);
    let images: Vec<Vec<u8>> = faces.iter().map(|f| scale_face_to(&f.data, f.size, size)).collect();
    let slots = Q3_TO_BABYLON_FACE_MAP.map(|(index, transform)| transform_face(&images[index], size, transform));
    Ok(SkyboxFaces {
        size,
        faces: slots,
        loaded_faces,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_move_the_corner() {
        // A 2x2 face with only the top-left pixel set.
        let mut face = vec![0u8; 16];
        face[..4].copy_from_slice(&[1, 2, 3, 4]);
        let corner = |t| {
            let out = transform_face(&face, 2, t);
            out.chunks(4).position(|p| p == [1, 2, 3, 4]).unwrap()
        };
        assert_eq!(corner(FaceTransform::FlipH), 1);
        assert_eq!(corner(FaceTransform::FlipV), 2);
        assert_eq!(corner(FaceTransform::Rotate180), 3);
        assert_eq!(corner(FaceTransform::Rotate90Cw), 1);
        assert_eq!(corner(FaceTransform::Rotate90Ccw), 2);
        assert_eq!(corner(FaceTransform::Rotate90CwFlipH), 0);
        assert_eq!(corner(FaceTransform::Rotate90CcwFlipH), 3);
    }

    #[test]
    fn a_stub_face_fills_with_its_colour() {
        let scaled = scale_face_to(&[9, 8, 7, 6], 1, 3);
        assert_eq!(scaled.len(), 36);
        assert!(scaled.chunks(4).all(|p| p == [9, 8, 7, 6]));
    }
}
