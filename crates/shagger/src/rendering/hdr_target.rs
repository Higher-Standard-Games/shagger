//! The HDR target the scene renders into before image processing, as SHAG's
//! DefaultRenderingPipeline (`hdr: true`) sets it up: half floats, with
//! depth, the size of the output.

use super::gpu::Gpu;
use super::world_renderer::DEPTH_FORMAT;
use crate::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess,
};

/// The format every scene renderer draws into.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The scene's colour and depth, before image processing.
pub struct HdrTarget {
    pub color: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    /// The textures behind the views, for copies of the scene (water).
    pub color_texture: wgpu::Texture,
    pub depth_texture: wgpu::Texture,
    pub size: (u32, u32),
}

impl HdrTarget {
    #[must_use]
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let size = wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        let texture = |label, format, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color_texture = texture(
            "hdr colour",
            HDR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let depth_texture = texture(
            "hdr depth",
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        Self {
            color: color_texture.create_view(&wgpu::TextureViewDescriptor::default()),
            depth: depth_texture.create_view(&wgpu::TextureViewDescriptor::default()),
            color_texture,
            depth_texture,
            size: (size.width, size.height),
        }
    }

    /// Makes the target `width` × `height` again if it is not already.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if self.size != (width.max(1), height.max(1)) {
            *self = Self::new(gpu, width, height);
        }
    }

    /// Image processing from this target into `output`.
    pub fn resolve(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        post: &ImageProcessingPostProcess,
        config: &ImageProcessingConfiguration,
        output: &wgpu::TextureView,
    ) {
        post.apply(gpu, encoder, config, &self.color, output, self.size);
    }
}
