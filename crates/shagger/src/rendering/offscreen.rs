//! Rendering into an image instead of a window: screenshots, the visual
//! parity harness (RUST.md 8.3) and machines without a display.

use super::gpu::Gpu;
use super::hdr_target::HdrTarget;
use super::world_renderer::DEPTH_FORMAT;
use crate::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess,
};

/// The colour format offscreen frames come out in. Like the browser canvas,
/// it holds the output bytes as they are.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Renders one frame of `width` × `height` and reads it back as RGBA8 rows.
/// `record` draws into a pass that has cleared colour to `clear` and depth to 1.
pub fn render_to_rgba(
    gpu: &Gpu,
    width: u32,
    height: u32,
    clear: wgpu::Color,
    record: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<u8> {
    let color = output_texture(gpu, width, height);
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen depth"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("offscreen"),
    });
    {
        let mut pass = begin_main_pass(&mut encoder, &color_view, &depth_view, clear);
        record(&mut pass);
    }
    read_back(gpu, encoder, &color, width, height)
}

/// Clears a frame to `clear`, lets `record` encode into its colour view
/// (egui paints this way, with its own passes), and reads it back.
pub fn render_encoded_to_rgba(
    gpu: &Gpu,
    width: u32,
    height: u32,
    clear: wgpu::Color,
    record: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
) -> Vec<u8> {
    let color = output_texture(gpu, width, height);
    let view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("offscreen encoded"),
    });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("offscreen clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    record(&mut encoder, &view);
    read_back(gpu, encoder, &color, width, height)
}

/// Renders one frame as the window does: the scene into an HDR target
/// (`HDR_FORMAT` renderers), then image processing into RGBA8, read back.
pub fn render_hdr_to_rgba(
    gpu: &Gpu,
    width: u32,
    height: u32,
    clear: wgpu::Color,
    post: &ImageProcessingPostProcess,
    config: &ImageProcessingConfiguration,
    record: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<u8> {
    let hdr = HdrTarget::new(gpu, width, height);
    let color = output_texture(gpu, width, height);
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("offscreen"),
    });
    {
        let mut pass = begin_main_pass(&mut encoder, &hdr.color, &hdr.depth, clear);
        record(&mut pass);
    }
    hdr.resolve(gpu, &mut encoder, post, config, &color_view);
    read_back(gpu, encoder, &color, width, height)
}

/// [`render_hdr_to_rgba`] with a second pass after the scene: `overlay`
/// draws over its colour with the depth cleared, as the first-person
/// weapon's rendering group does.
#[allow(clippy::too_many_arguments)]
pub fn render_hdr_with_overlay_to_rgba(
    gpu: &Gpu,
    width: u32,
    height: u32,
    clear: wgpu::Color,
    post: &ImageProcessingPostProcess,
    config: &ImageProcessingConfiguration,
    record: impl FnOnce(&mut wgpu::RenderPass<'_>),
    overlay: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<u8> {
    let hdr = HdrTarget::new(gpu, width, height);
    let color = output_texture(gpu, width, height);
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("offscreen"),
    });
    {
        let mut pass = begin_main_pass(&mut encoder, &hdr.color, &hdr.depth, clear);
        record(&mut pass);
    }
    {
        let mut pass = begin_overlay_pass(&mut encoder, &hdr.color, &hdr.depth);
        overlay(&mut pass);
    }
    hdr.resolve(gpu, &mut encoder, post, config, &color_view);
    read_back(gpu, encoder, &color, width, height)
}

/// [`render_hdr_with_overlay_to_rgba`] with a last pass over the output
/// after image processing: the 2D layer.
#[allow(clippy::too_many_arguments)]
pub fn render_hdr_with_ui_to_rgba(
    gpu: &Gpu,
    width: u32,
    height: u32,
    clear: wgpu::Color,
    post: &ImageProcessingPostProcess,
    config: &ImageProcessingConfiguration,
    record: impl FnOnce(&mut wgpu::RenderPass<'_>),
    overlay: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ui: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<u8> {
    let hdr = HdrTarget::new(gpu, width, height);
    let color = output_texture(gpu, width, height);
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("offscreen"),
    });
    {
        let mut pass = begin_main_pass(&mut encoder, &hdr.color, &hdr.depth, clear);
        record(&mut pass);
    }
    {
        let mut pass = begin_overlay_pass(&mut encoder, &hdr.color, &hdr.depth);
        overlay(&mut pass);
    }
    hdr.resolve(gpu, &mut encoder, post, config, &color_view);
    {
        let mut pass = begin_ui_pass(&mut encoder, &color_view);
        ui(&mut pass);
    }
    read_back(gpu, encoder, &color, width, height)
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn output_texture(gpu: &Gpu, width: u32, height: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen colour"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Copies `color` out after the encoder's passes and returns its rows.
fn read_back(gpu: &Gpu, mut encoder: wgpu::CommandEncoder, color: &wgpu::Texture, width: u32, height: u32) -> Vec<u8> {
    let device = &gpu.device;
    let row = (4 * width).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen readback"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        extent(width, height),
    );
    gpu.queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("the readback buffer was mapped above");
    let mut pixels = Vec::with_capacity((4 * width * height) as usize);
    for y in 0..height as usize {
        let start = y * row as usize;
        pixels.extend_from_slice(&data[start..start + 4 * width as usize]);
    }
    pixels
}

/// Begins the main pass: colour cleared to `clear`, depth to 1.
pub fn begin_main_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("main"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Begins a pass over what the main pass drew: colour kept, depth cleared
/// to 1 (`setRenderingAutoClearDepthStencil(1, true, true, false)`).
pub fn begin_overlay_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("overlay"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Begins a pass over the finished output, colour kept, no depth: the 2D
/// layer.
pub fn begin_ui_pass<'e>(encoder: &'e mut wgpu::CommandEncoder, color: &wgpu::TextureView) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ui"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
