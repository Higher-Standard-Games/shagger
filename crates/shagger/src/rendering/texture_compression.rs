//! Native only: map and model textures compressed to BC7 on the GPU as they
//! upload. SHAG's browser build uploads its textures as RGBA8, four bytes a
//! texel; BC7 takes one, so washoi_dm5's textures need a quarter of the
//! memory. The compressor is `block_compression`, Intel's ISPC texture
//! compressor as WGSL compute shaders, so it runs on whatever GPU draws.
//!
//! BC is sampled by every desktop GPU wgpu runs on (Apple silicon and Intel
//! Macs, Windows, Linux); where the device lacks it, or `r_texture_compression`
//! is 0, or a texture's size is not a multiple of the 4×4 block, textures
//! upload uncompressed as before. A software renderer (llvmpipe, WARP)
//! decodes BC7 on every sample and has no video memory to save: drawing
//! washoi_dm5 on llvmpipe went from 5.4 to 0.4 frames a second. So a CPU
//! device keeps RGBA8 unless `r_texture_compression` is 2. BC7 is lossy:
//! the picture differs from the browser's by the compression error, which
//! `r_texture_compression 0` takes away.

use std::sync::Mutex;

use block_compression::{BC7Settings, CompressionVariant, GpuBlockCompressor};

use super::gpu::Gpu;
use super::world_renderer::Rgba8Image;

/// The cvar: 0 off, 1 on a GPU (the default), 2 on a software renderer too.
pub const CVAR: &str = "r_texture_compression";

/// Bytes in one BC7 block of 4×4 texels.
const BLOCK_BYTES: u32 = 16;

/// The BC7 compressor, made with the first texture it compresses: its
/// pipelines take a moment to build.
#[derive(Default)]
pub struct Compressor(Mutex<Option<GpuBlockCompressor>>);

/// Whether `r_texture_compression` asks for compression on `gpu`; 1 when
/// unregistered.
fn wanted(gpu: &Gpu) -> bool {
    let level = crate::core::cvar::with_cvars(|c| if c.exists(CVAR) { c.get_number(CVAR) } else { 1.0 });
    let software = gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    gpu.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC) && level >= if software { 2.0 } else { 1.0 }
}

/// How many of `levels`, from the first, can be BC7: those whose sides are
/// whole blocks. The texture keeps only these; past them, sampling stays on
/// the last.
fn block_levels(levels: &[Rgba8Image]) -> usize {
    levels
        .iter()
        .take_while(|l| l.width >= 4 && l.height >= 4 && l.width % 4 == 0 && l.height % 4 == 0)
        .count()
}

/// Uploads `levels` (a mip chain, largest first) as a BC7 texture, or
/// `None` when it should upload uncompressed (see the module comment).
pub(crate) fn upload_bc7(gpu: &Gpu, label: &str, levels: &[Rgba8Image]) -> Option<wgpu::Texture> {
    if !wanted(gpu) {
        return None;
    }
    compress(gpu, label, levels)
}

/// [`upload_bc7`] without asking whether it is wanted.
fn compress(gpu: &Gpu, label: &str, levels: &[Rgba8Image]) -> Option<wgpu::Texture> {
    let count = block_levels(levels);
    if count == 0 {
        return None;
    }
    let levels = &levels[..count];
    let base = &levels[0];
    let size = |l: &Rgba8Image| wgpu::Extent3d {
        width: l.width,
        height: l.height,
        depth_or_array_layers: 1,
    };
    // The source, uncompressed, only for as long as the compute pass reads it.
    let source = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bc7 source"),
        size: size(base),
        mip_level_count: count as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, data) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * data.width),
                rows_per_image: Some(data.height),
            },
            size(data),
        );
    }
    // Opaque textures spend no bits on alpha.
    let opaque = base.pixels.chunks_exact(4).all(|p| p[3] == 255);
    let settings = if opaque {
        BC7Settings::opaque_basic()
    } else {
        BC7Settings::alpha_basic()
    };
    let variant = CompressionVariant::BC7(settings);
    let offsets: Vec<u64> = levels
        .iter()
        .scan(0u64, |at, l| {
            let offset = *at;
            *at += variant.blocks_byte_size(l.width, l.height) as u64;
            Some(offset)
        })
        .collect();
    let last = &levels[count - 1];
    let total = offsets[count - 1] + variant.blocks_byte_size(last.width, last.height) as u64;
    let blocks = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bc7 blocks"),
        size: total,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bc7") });
    {
        let mut compressor = gpu
            .compressor
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let compressor =
            compressor.get_or_insert_with(|| GpuBlockCompressor::new(gpu.device.clone(), gpu.queue.clone()));
        for (level, (data, offset)) in levels.iter().zip(&offsets).enumerate() {
            let view = source.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level as u32,
                mip_level_count: Some(1),
                ..Default::default()
            });
            compressor.add_compression_task(
                variant,
                &view,
                data.width,
                data.height,
                &blocks,
                None,
                Some(*offset as u32),
            );
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bc7"),
            timestamp_writes: None,
        });
        compressor.compress(&mut pass);
    }
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: size(base),
        mip_level_count: count as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // As the uncompressed textures: Babylon samples the bytes as they are.
        format: wgpu::TextureFormat::Bc7RgbaUnorm,
        // COPY_SRC lets a test read the blocks back.
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    for (level, (data, &offset)) in levels.iter().zip(&offsets).enumerate() {
        let row = data.width / 4 * BLOCK_BYTES;
        let rows = data.height / 4;
        let copy = |encoder: &mut wgpu::CommandEncoder, offset: u64, y: u32, block_rows: u32, bytes_per_row| {
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &blocks,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset,
                        bytes_per_row,
                        rows_per_image: None,
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d { x: 0, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: data.width,
                    height: block_rows * 4,
                    depth_or_array_layers: 1,
                },
            );
        };
        if row.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) {
            copy(&mut encoder, offset, 0, rows, Some(row));
        } else {
            // The blocks are packed, and a copy of several block rows needs
            // them 256-byte aligned: one copy a block row (small levels).
            for r in 0..rows {
                copy(&mut encoder, offset + u64::from(r * row), r * 4, 1, None);
            }
        }
    }
    gpu.queue.submit([encoder.finish()]);
    Some(texture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(width: u32, height: u32) -> Rgba8Image {
        Rgba8Image {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    #[test]
    fn keeps_the_levels_made_of_whole_blocks() {
        let chain = [
            level(64, 32),
            level(32, 16),
            level(16, 8),
            level(8, 4),
            level(4, 2),
            level(2, 1),
        ];
        assert_eq!(block_levels(&chain), 4);
        assert_eq!(block_levels(&[level(850, 96)]), 0);
        assert_eq!(block_levels(&[level(1, 1)]), 0);
    }

    /// Compresses a gradient on whatever adapter there is and reads it back
    /// through a shader-free copy: the texture exists with its levels, and
    /// the compressor wrote blocks (not zeros).
    #[test]
    fn compresses_a_mip_chain_where_the_device_has_bc() {
        let Ok(gpu) = Gpu::headless() else { return };
        if !gpu.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
            return;
        }
        let mut base = level(128, 64);
        for (i, p) in base.pixels.chunks_exact_mut(4).enumerate() {
            let (x, y) = ((i % 128) as u8, (i / 128) as u8);
            p.copy_from_slice(&[x * 2, y * 4, 128, 255]);
        }
        let chain = super::super::world_renderer::mip_chain(&base);
        let texture = compress(&gpu, "test", &chain).expect("compressed");
        assert_eq!(texture.format(), wgpu::TextureFormat::Bc7RgbaUnorm);
        assert_eq!(texture.mip_level_count() as usize, block_levels(&chain));
        // Read the first block row of level 0 back.
        let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 32 * 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 128,
                height: 4,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let bytes = read.slice(..).get_mapped_range().expect("mapped").to_vec();
        assert!(bytes.iter().any(|&b| b != 0));
        // Decoded, the first block is close to the gradient's corner.
        let mut texels = vec![0u8; 4 * 4 * 4];
        block_compression::decode::decompress_blocks_as_rgba8(
            CompressionVariant::BC7(BC7Settings::opaque_basic()),
            4,
            4,
            &bytes[..16],
            &mut texels,
        );
        for (got, want) in texels.chunks_exact(4).zip(
            base.pixels
                .chunks_exact(4 * 128)
                .take(4)
                .flat_map(|row| row[..16].chunks_exact(4)),
        ) {
            for c in 0..3 {
                assert!(got[c].abs_diff(want[c]) <= 8, "{got:?} against {want:?}");
            }
        }
    }
}
