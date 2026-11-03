//! SHAG `src/engine/loader/texture/KTX1Loader.ts`: KTX version 1 textures,
//! decoded to RGBA8 on the CPU.
//!
//! SHAG uploads ETC1 and DXT blocks to the GPU when the device takes them and
//! otherwise decodes them in software; desktop GPUs take no ETC1, so this
//! port always takes the software path (`softwareDecodeMip` and the
//! uncompressed conversions of `createTextureFromKTX1`). Only the base level
//! is kept, as SHAG keeps it there; the renderer makes its own mip chain.
//!
//! The arithmetic follows the TypeScript's: 32-bit integer bit operations
//! (`i32`), and the DXT palettes in `f64`, truncated where a `Uint8Array`
//! stores them.

use std::fmt;

const KTX_IDENTIFIER: [u8; 12] = [0xab, 0x4b, 0x54, 0x58, 0x20, 0x31, 0x31, 0xbb, 0x0d, 0x0a, 0x1a, 0x0a];
const KTX_HEADER_SIZE: usize = 64;

/// `GL_FORMATS`: the OpenGL format numbers the loader knows.
pub mod gl_formats {
    pub const COMPRESSED_RGB_S3TC_DXT1_EXT: u32 = 0x83f0;
    pub const COMPRESSED_RGBA_S3TC_DXT1_EXT: u32 = 0x83f1;
    pub const COMPRESSED_RGBA_S3TC_DXT3_EXT: u32 = 0x83f2;
    pub const COMPRESSED_RGBA_S3TC_DXT5_EXT: u32 = 0x83f3;
    pub const COMPRESSED_RGB8_ETC2: u32 = 0x9274;
    pub const COMPRESSED_RGBA8_ETC2_EAC: u32 = 0x9278;
    pub const ETC1_RGB8_OES: u32 = 0x8d64;
    pub const ALPHA: u32 = 0x1906;
    pub const RGB: u32 = 0x1907;
    pub const RGBA: u32 = 0x1908;
    pub const LUMINANCE: u32 = 0x1909;
    pub const LUMINANCE_ALPHA: u32 = 0x190a;
}
use gl_formats as gl;

/// A KTX file the loader refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ktx1Error(pub String);

impl fmt::Display for Ktx1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Ktx1Error {}

fn err<T>(message: &str) -> Result<T, Ktx1Error> {
    Err(Ktx1Error(message.to_owned()))
}

/// `KTX1Header`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ktx1Header {
    pub gl_type: u32,
    pub gl_type_size: u32,
    pub gl_format: u32,
    pub gl_internal_format: u32,
    pub gl_base_internal_format: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub pixel_depth: u32,
    pub number_of_array_elements: u32,
    pub number_of_faces: u32,
    pub number_of_mipmap_levels: u32,
    pub bytes_of_key_value_data: u32,
}

/// `KTX1MipLevel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ktx1MipLevel<'a> {
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}

/// `KTX1ParseResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ktx1ParseResult<'a> {
    pub little_endian: bool,
    pub header: Ktx1Header,
    pub mip_levels: Vec<Ktx1MipLevel<'a>>,
    pub is_compressed: bool,
}

/// `compressedBlock` (ASTC left out: nothing here decodes it).
fn compressed_block(format: u32) -> Option<(u32, u32, u32)> {
    match format {
        0x83f0..=0x83f1 => Some((4, 4, 8)),
        0x83f2..=0x83f3 => Some((4, 4, 16)),
        gl::ETC1_RGB8_OES | gl::COMPRESSED_RGB8_ETC2 => Some((4, 4, 8)),
        gl::COMPRESSED_RGBA8_ETC2_EAC => Some((4, 4, 16)),
        _ => None,
    }
}

/// `parseKTX1CubeFaces`: a six-face KTX cube split into six one-face KTX
/// files (the header with one face, then each level's size and that face's
/// padded image), each for [`parse_ktx1`], in the file's face order.
pub fn parse_ktx1_cube_faces(data: &[u8]) -> Result<Vec<Vec<u8>>, Ktx1Error> {
    if data.len() < 64 {
        return err("Truncated KTX1 cube header");
    }
    let endian = u32::from_le_bytes(data[12..16].try_into().unwrap());
    if endian != 0x0403_0201 && endian != 0x0102_0304 {
        return err("Invalid KTX1 cube byte order");
    }
    let little = endian == 0x0403_0201;
    let read = |offset: usize| {
        let bytes: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    if read(52) != 6 || read(36) != read(40) {
        return err("KTX1 cube requires six square faces");
    }
    let levels = read(56).max(1);
    if levels > 32 {
        return err("Invalid KTX1 cube mip count");
    }
    let header_size = 64 + read(60) as usize;
    if header_size > data.len() {
        return err("Truncated KTX1 cube metadata");
    }
    let one = if little { 1u32.to_le_bytes() } else { 1u32.to_be_bytes() };
    let mut faces: Vec<Vec<u8>> = (0..6)
        .map(|_| {
            let mut face = data[..header_size].to_vec();
            face[52..56].copy_from_slice(&one);
            face
        })
        .collect();
    let mut offset = header_size;
    for _ in 0..levels {
        if offset + 4 > data.len() {
            return err("Truncated KTX1 cube mip size");
        }
        let size = read(offset) as usize;
        let size_bytes = &data[offset..offset + 4];
        offset += 4;
        for face in &mut faces {
            if offset + size > data.len() {
                return err("Truncated KTX1 cube face");
            }
            let padded = size.div_ceil(4) * 4;
            if offset + padded > data.len() {
                return err("Truncated KTX1 cube padding");
            }
            face.extend_from_slice(size_bytes);
            face.extend_from_slice(&data[offset..offset + padded]);
            offset += padded;
        }
    }
    Ok(faces)
}

/// `parseKTX1`: a one-face 2D KTX and its mip levels.
pub fn parse_ktx1(data: &[u8]) -> Result<Ktx1ParseResult<'_>, Ktx1Error> {
    if data.len() < KTX_HEADER_SIZE {
        return err("Invalid KTX1 file: truncated header");
    }
    if data[..12] != KTX_IDENTIFIER {
        return err("Invalid KTX1 file: bad identifier");
    }
    let endianness = u32::from_le_bytes(data[12..16].try_into().unwrap());
    if endianness != 0x0403_0201 && endianness != 0x0102_0304 {
        return err("Invalid KTX1 byte order");
    }
    let little_endian = endianness == 0x0403_0201;
    let read = |offset: usize| {
        let bytes: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
        if little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    let header = Ktx1Header {
        gl_type: read(16),
        gl_type_size: read(20),
        gl_format: read(24),
        gl_internal_format: read(28),
        gl_base_internal_format: read(32),
        pixel_width: read(36),
        pixel_height: read(40),
        pixel_depth: read(44),
        number_of_array_elements: read(48),
        number_of_faces: read(52),
        number_of_mipmap_levels: read(56),
        bytes_of_key_value_data: read(60),
    };
    let is_compressed = header.gl_type == 0;
    if header.pixel_width == 0
        || header.pixel_height == 0
        || header.pixel_depth != 0
        || header.number_of_array_elements != 0
        || header.number_of_faces != 1
    {
        return err("KTX1 loader requires a non-array 2D texture with one face");
    }
    if is_compressed && (header.gl_format != 0 || header.gl_type_size != 1) {
        return err("Invalid compressed KTX1 type/format");
    }
    let full_mip_count = header.pixel_width.max(header.pixel_height).ilog2() + 1;
    if header.number_of_mipmap_levels > full_mip_count {
        return err("Invalid KTX1 mip count");
    }

    let mut mip_levels = Vec::new();
    let mut offset = KTX_HEADER_SIZE + header.bytes_of_key_value_data as usize;
    let (mut width, mut height) = (header.pixel_width, header.pixel_height);
    for level in 0..header.number_of_mipmap_levels.max(1) {
        if offset + 4 > data.len() {
            // Legacy raw files may omit trailing mips; only mip 0 is uploaded.
            if !is_compressed && !mip_levels.is_empty() {
                break;
            }
            return Err(Ktx1Error(format!("Truncated KTX1: missing mip {level} size")));
        }
        let image_size = read(offset) as usize;
        offset += 4;
        if offset + image_size > data.len() {
            if !is_compressed && !mip_levels.is_empty() {
                break;
            }
            return Err(Ktx1Error(format!("Truncated KTX1: incomplete mip {level}")));
        }
        if is_compressed
            && let Some((bw, bh, bytes)) = compressed_block(header.gl_internal_format)
            && image_size != (width.div_ceil(bw) * height.div_ceil(bh) * bytes) as usize
        {
            return Err(Ktx1Error(format!(
                "Invalid KTX1 compressed payload size at mip {level}"
            )));
        }
        mip_levels.push(Ktx1MipLevel {
            width,
            height,
            data: &data[offset..offset + image_size],
        });
        offset = (offset + image_size).div_ceil(4) * 4;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Ok(Ktx1ParseResult {
        little_endian,
        header,
        mip_levels,
        is_compressed,
    })
}

/// `canSoftwareDecode`.
#[must_use]
pub fn can_software_decode(format: u32) -> bool {
    matches!(
        format,
        gl::COMPRESSED_RGB_S3TC_DXT1_EXT
            | gl::COMPRESSED_RGBA_S3TC_DXT1_EXT
            | gl::COMPRESSED_RGBA_S3TC_DXT3_EXT
            | gl::COMPRESSED_RGBA_S3TC_DXT5_EXT
            | gl::ETC1_RGB8_OES
    )
}

/// A byte as `Uint8Array` stores a number: truncated, then modulo 256.
fn to_uint8(value: f64) -> u8 {
    if value.is_finite() {
        (value.trunc().rem_euclid(256.0)) as u8
    } else {
        0
    }
}

/// `decompressDXT1Block`.
fn decompress_dxt1_block(block: &[u8], out: &mut [u8], out_off: usize, stride: usize) {
    let c0 = u32::from(block[0]) | (u32::from(block[1]) << 8);
    let c1 = u32::from(block[2]) | (u32::from(block[3]) << 8);
    let r0 = f64::from((c0 >> 11) & 0x1f) * 255.0 / 31.0;
    let g0 = f64::from((c0 >> 5) & 0x3f) * 255.0 / 63.0;
    let b0 = f64::from(c0 & 0x1f) * 255.0 / 31.0;
    let r1 = f64::from((c1 >> 11) & 0x1f) * 255.0 / 31.0;
    let g1 = f64::from((c1 >> 5) & 0x3f) * 255.0 / 63.0;
    let b1 = f64::from(c1 & 0x1f) * 255.0 / 31.0;
    let mut pal = [0u8; 16];
    let mut set = |i: usize, rgba: [f64; 4]| {
        for (k, v) in rgba.into_iter().enumerate() {
            pal[i * 4 + k] = to_uint8(v);
        }
    };
    set(0, [r0, g0, b0, 255.0]);
    set(1, [r1, g1, b1, 255.0]);
    if c0 > c1 {
        set(
            2,
            [
                (2.0 * r0 + r1) / 3.0,
                (2.0 * g0 + g1) / 3.0,
                (2.0 * b0 + b1) / 3.0,
                255.0,
            ],
        );
        set(
            3,
            [
                (r0 + 2.0 * r1) / 3.0,
                (g0 + 2.0 * g1) / 3.0,
                (b0 + 2.0 * b1) / 3.0,
                255.0,
            ],
        );
    } else {
        set(2, [(r0 + r1) / 2.0, (g0 + g1) / 2.0, (b0 + b1) / 2.0, 255.0]);
        set(3, [0.0; 4]); // transparent black
    }
    let lookup = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    for row in 0..4 {
        for col in 0..4 {
            let ci = ((lookup >> ((row * 4 + col) * 2)) & 3) as usize;
            let o = out_off + row * stride + col * 4;
            out[o..o + 4].copy_from_slice(&pal[ci * 4..ci * 4 + 4]);
        }
    }
}

/// `ETC1_MODIFIER_TABLE` (Khronos spec table 3.17.2).
const ETC1_MODIFIER_TABLE: [[i32; 2]; 8] = [
    [2, 8],
    [5, 17],
    [9, 29],
    [13, 42],
    [18, 60],
    [24, 80],
    [33, 106],
    [47, 183],
];

/// `decompressETC1Block`.
fn decompress_etc1_block(block: &[u8], out: &mut [u8], out_off: usize, stride: usize) {
    let high = i32::from_be_bytes([block[0], block[1], block[2], block[3]]);
    let low = i32::from_be_bytes([block[4], block[5], block[6], block[7]]);
    let diff_bit = (high >> 1) & 1;
    let flip_bit = high & 1;
    let table_idx0 = ((high >> 5) & 7) as usize;
    let table_idx1 = ((high >> 2) & 7) as usize;
    let expand5 = |c: i32| (c << 3) | (c >> 2);
    let expand4 = |c: i32| (c << 4) | c;
    let ([r0, g0, b0], [r1, g1, b1]) = if diff_bit != 0 {
        let br = (high >> 27) & 0x1f;
        let bg = (high >> 19) & 0x1f;
        let bb = (high >> 11) & 0x1f;
        let signed = |d: i32| if d >= 4 { d - 8 } else { d };
        let sdr = signed((high >> 24) & 0x07);
        let sdg = signed((high >> 16) & 0x07);
        let sdb = signed((high >> 8) & 0x07);
        (
            [expand5(br), expand5(bg), expand5(bb)],
            [expand5(br + sdr), expand5(bg + sdg), expand5(bb + sdb)],
        )
    } else {
        (
            [
                expand4((high >> 28) & 0xf),
                expand4((high >> 20) & 0xf),
                expand4((high >> 12) & 0xf),
            ],
            [
                expand4((high >> 24) & 0xf),
                expand4((high >> 16) & 0xf),
                expand4((high >> 8) & 0xf),
            ],
        )
    };
    for row in 0..4 {
        for col in 0..4 {
            // Pixel indices are column-major.
            let pixel_idx = col * 4 + row;
            let msb = (low >> (pixel_idx + 16)) & 1;
            let lsb = (low >> pixel_idx) & 1;
            let in_sub_block0 = if flip_bit != 0 { row < 2 } else { col < 2 };
            let (base_r, base_g, base_b) = if in_sub_block0 { (r0, g0, b0) } else { (r1, g1, b1) };
            let entry = ETC1_MODIFIER_TABLE[if in_sub_block0 { table_idx0 } else { table_idx1 }];
            let modifier = match (msb << 1) | lsb {
                0 => entry[0],
                1 => entry[1],
                2 => -entry[0],
                _ => -entry[1],
            };
            let o = out_off + row as usize * stride + col as usize * 4;
            out[o] = (base_r + modifier).clamp(0, 255) as u8;
            out[o + 1] = (base_g + modifier).clamp(0, 255) as u8;
            out[o + 2] = (base_b + modifier).clamp(0, 255) as u8;
            out[o + 3] = 255;
        }
    }
}

/// `decompressDXT5Block`.
fn decompress_dxt5_block(block: &[u8], out: &mut [u8], out_off: usize, stride: usize) {
    let (a0, a1) = (f64::from(block[0]), f64::from(block[1]));
    let mut alphas = [0u8; 8];
    alphas[0] = block[0];
    alphas[1] = block[1];
    if a0 > a1 {
        for i in 0..6 {
            alphas[i + 2] = to_uint8(((6 - i) as f64 * a0 + (i + 1) as f64 * a1) / 7.0);
        }
    } else {
        for i in 0..4 {
            alphas[i + 2] = to_uint8(((4 - i) as f64 * a0 + (i + 1) as f64 * a1) / 5.0);
        }
        alphas[6] = 0;
        alphas[7] = 255;
    }
    let alpha_bits = [
        u32::from(block[2]) | (u32::from(block[3]) << 8) | (u32::from(block[4]) << 16),
        u32::from(block[5]) | (u32::from(block[6]) << 8) | (u32::from(block[7]) << 16),
    ];
    decompress_dxt1_block(&block[8..16], out, out_off, stride);
    for i in 0..16 {
        let (row, col) = (i >> 2, i & 3);
        let group = usize::from(i >= 8);
        let bit = (if i < 8 { i } else { i - 8 }) * 3;
        let ai = ((alpha_bits[group] >> bit) & 7) as usize;
        out[out_off + row * stride + col * 4 + 3] = alphas[ai];
    }
}

/// `softwareDecodeMip`: one compressed level to RGBA, rows as stored.
#[must_use]
pub fn software_decode_mip(data: &[u8], width: u32, height: u32, format: u32) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let bw = width.div_ceil(4).max(1);
    let bh = height.div_ceil(4).max(1);
    let stride = width * 4;
    let len = width * height * 4;
    // The TypeScript writes with the image's own stride, so a block that
    // overhangs a row edge spills into the next row, and writes past the end
    // of the typed array are dropped. The slack takes those writes and is cut
    // off after.
    let mut rgba = vec![0u8; len + 4 * stride + 16];
    let is_dxt1 = matches!(
        format,
        gl::COMPRESSED_RGB_S3TC_DXT1_EXT | gl::COMPRESSED_RGBA_S3TC_DXT1_EXT
    );
    let is_etc1 = format == gl::ETC1_RGB8_OES;
    let block_size = if is_dxt1 || is_etc1 { 8 } else { 16 };
    let mut src_off = 0;
    for by in 0..bh {
        for bx in 0..bw {
            let Some(block) = data.get(src_off..src_off + block_size) else {
                break;
            };
            let out_off = by * 4 * stride + bx * 16;
            let padded = &mut rgba;
            match format {
                _ if is_dxt1 => decompress_dxt1_block(block, padded, out_off, stride),
                gl::COMPRESSED_RGBA_S3TC_DXT5_EXT => {
                    decompress_dxt5_block(block, padded, out_off, stride);
                }
                gl::COMPRESSED_RGBA_S3TC_DXT3_EXT => {
                    decompress_dxt1_block(&block[8..16], padded, out_off, stride);
                    for i in 0..16 {
                        let (row, col) = (i >> 2, i & 3);
                        let byte = block[i >> 1];
                        let nibble = if i & 1 == 1 { byte >> 4 } else { byte & 0xf };
                        padded[out_off + row * stride + col * 4 + 3] = nibble * 17;
                    }
                }
                _ if is_etc1 => decompress_etc1_block(block, padded, out_off, stride),
                _ => {}
            }
            src_off += block_size;
        }
    }
    rgba.truncate(len);
    rgba
}

/// A KTX texture's base level as RGBA8, rows top to bottom: what
/// `createTextureFromKTX1` uploads on its uncompressed and software paths.
/// `None` for a compressed format there is no decoder for.
#[must_use]
pub fn ktx1_rgba(ktx: &Ktx1ParseResult<'_>) -> Option<(u32, u32, Vec<u8>)> {
    let mip = ktx.mip_levels.first()?;
    let (w, h) = (mip.width, mip.height);
    let pixel_count = (w * h) as usize;
    let data = mip.data;
    if !ktx.is_compressed {
        let base = ktx.header.gl_base_internal_format;
        let expand = |f: &dyn Fn(usize) -> [u8; 4]| -> Vec<u8> { (0..pixel_count).flat_map(f).collect() };
        let byte = |i: usize| data.get(i).copied().unwrap_or(0);
        let rgba = if base == gl::RGBA && data.len() >= pixel_count * 4 {
            data.to_vec()
        } else if base == gl::ALPHA {
            // Legacy GL_ALPHA samples as white with the stored alpha.
            expand(&|i| [255, 255, 255, byte(i)])
        } else if base == gl::RGB {
            expand(&|i| [byte(i * 3), byte(i * 3 + 1), byte(i * 3 + 2), 255])
        } else if base == gl::LUMINANCE {
            expand(&|i| [byte(i), byte(i), byte(i), 255])
        } else if base == gl::LUMINANCE_ALPHA {
            expand(&|i| {
                let v = byte(i * 2);
                [v, v, v, byte(i * 2 + 1)]
            })
        } else {
            // Unknown: tried as RGBA, as the TypeScript does.
            data.to_vec()
        };
        // Babylon reads a short buffer as zeros.
        let mut rgba = rgba;
        rgba.resize(pixel_count * 4, 0);
        return Some((w, h, rgba));
    }
    let format = ktx.header.gl_internal_format;
    if !can_software_decode(format) {
        return None;
    }
    let mut rgba = software_decode_mip(data, w, h, format);
    // KTX stores rows bottom to top (the OpenGL convention).
    let row_bytes = w as usize * 4;
    let flipped: Vec<u8> = rgba.chunks_exact(row_bytes).rev().flatten().copied().collect();
    rgba.copy_from_slice(&flipped);
    Some((w, h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-level KTX around `payload`.
    fn ktx(gl_type: u32, format: u32, internal: u32, base: u32, w: u32, h: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = KTX_IDENTIFIER.to_vec();
        for v in [
            0x0403_0201,
            gl_type,
            1,
            format,
            internal,
            base,
            w,
            h,
            0,
            0,
            1,
            1,
            0,
            payload.len() as u32,
        ] {
            out.extend_from_slice(&u32::to_le_bytes(v));
        }
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn a_cube_splits_into_its_faces() {
        // 1x1 RGBA, one level: six 4-byte faces after the one size.
        let mut file = ktx(0x1401, 0x1908, 0x8058, 0x1908, 1, 1, &[0, 0, 0, 255]);
        file[52..56].copy_from_slice(&6u32.to_le_bytes());
        for face in 1..6u8 {
            file.extend_from_slice(&[face, face, face, 255]);
        }
        let faces = parse_ktx1_cube_faces(&file).unwrap();
        assert_eq!(faces.len(), 6);
        for (i, face) in faces.iter().enumerate() {
            let parsed = parse_ktx1(face).unwrap();
            assert_eq!(parsed.mip_levels[0].data, &[i as u8, i as u8, i as u8, 255]);
        }
        // Not square, or not six faces: refused.
        file[40..44].copy_from_slice(&2u32.to_le_bytes());
        assert!(parse_ktx1_cube_faces(&file).is_err());
    }

    #[test]
    fn etc1_individual_block_decodes_by_the_spec() {
        // Individual mode, no flip, table 0 both halves; base colours
        // 0x8 and 0x4 per channel (0x88, 0x44); every modifier index 0 (+2).
        let block = [0x84, 0x84, 0x84, 0x00, 0, 0, 0, 0];
        let file = ktx(0, 0, gl::ETC1_RGB8_OES, 0x1907, 4, 4, &block);
        let parsed = parse_ktx1(&file).unwrap();
        let (_, _, rgba) = ktx1_rgba(&parsed).unwrap();
        assert_eq!(&rgba[0..4], &[0x8a, 0x8a, 0x8a, 255]);
        assert_eq!(&rgba[8..12], &[0x46, 0x46, 0x46, 255]);
    }

    #[test]
    fn luminance_alpha_expands() {
        let file = ktx(0x1401, 0x190a, 0x8045, gl::LUMINANCE_ALPHA, 1, 1, &[10, 20, 0, 0]);
        let (_, _, rgba) = ktx1_rgba(&parse_ktx1(&file).unwrap()).unwrap();
        assert_eq!(rgba, [10, 10, 10, 20]);
    }

    #[test]
    fn bad_identifier_is_refused() {
        assert!(parse_ktx1(&[0; 64]).is_err());
    }
}
