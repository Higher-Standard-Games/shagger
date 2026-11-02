// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// packages/dev/core/src/Materials/imageProcessingConfiguration.ts (the
// settings and `bind`) and PostProcesses/imageProcessingPostProcess.ts.
//
// Changes: only exposure, contrast and the vignette; one full-screen pass
// from an HDR target (`fromLinearSpace`, as DefaultRenderingPipeline sets
// it) into the output.

//! Babylon's image processing as a post-process (`applyByPostProcess`).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::rendering::gpu::Gpu;

/// `VIGNETTEMODE_MULTIPLY` or `VIGNETTEMODE_OPAQUE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VignetteBlendMode {
    #[default]
    Multiply,
    Opaque,
}

/// The vignette's settings (`vignette*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vignette {
    pub weight: f64,
    pub stretch: f64,
    pub camera_fov: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub color: [f64; 3],
    pub blend_mode: VignetteBlendMode,
}

impl Default for Vignette {
    /// Babylon's defaults: weight 1.5, no stretch, a 0.5 field of view,
    /// centred, black, multiplied.
    fn default() -> Self {
        Self {
            weight: 1.5,
            stretch: 0.0,
            camera_fov: 0.5,
            center_x: 0.0,
            center_y: 0.0,
            color: [0.0; 3],
            blend_mode: VignetteBlendMode::Multiply,
        }
    }
}

/// `ImageProcessingConfiguration`, the settings the post-process takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageProcessingConfiguration {
    pub exposure: f64,
    pub contrast: f64,
    /// `vignetteEnabled`, with its settings.
    pub vignette: Option<Vignette>,
}

impl Default for ImageProcessingConfiguration {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            contrast: 1.0,
            vignette: None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Settings {
    params: [f32; 4],
    vignette_settings1: [f32; 4],
    vignette_settings2: [f32; 4],
    inverse_screen: [f32; 4],
}

/// `Scalar.Lerp` (`Mix` in the configuration's bind).
fn mix(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

impl ImageProcessingConfiguration {
    /// `bind`'s uniforms for an output of `width` × `height`.
    fn settings(&self, width: u32, height: u32) -> Settings {
        let inverse_width = 1.0 / f64::from(width.max(1));
        let inverse_height = 1.0 / f64::from(height.max(1));
        let mut settings = Settings {
            params: [self.exposure as f32, self.contrast as f32, 0.0, 0.0],
            vignette_settings1: [0.0; 4],
            vignette_settings2: [0.0; 4],
            inverse_screen: [inverse_width as f32, inverse_height as f32, 0.0, 0.0],
        };
        if let Some(v) = self.vignette {
            let aspect_ratio = inverse_height / inverse_width;
            let mut scale_y = (v.camera_fov * 0.5).tan();
            let mut scale_x = scale_y * aspect_ratio;
            let geometric_mean = (scale_x * scale_y).sqrt();
            scale_x = mix(scale_x, geometric_mean, v.stretch);
            scale_y = mix(scale_y, geometric_mean, v.stretch);
            settings.params[2] = 1.0;
            settings.params[3] = if v.blend_mode == VignetteBlendMode::Opaque {
                1.0
            } else {
                0.0
            };
            settings.vignette_settings1 = [
                scale_x as f32,
                scale_y as f32,
                (-scale_x * v.center_x) as f32,
                (-scale_y * v.center_y) as f32,
            ];
            settings.vignette_settings2 = [
                v.color[0] as f32,
                v.color[1] as f32,
                v.color[2] as f32,
                (-2.0 * v.weight) as f32,
            ];
        }
        settings
    }
}

/// `ImageProcessingPostProcess`: the HDR target through image processing
/// into the output.
pub struct ImageProcessingPostProcess {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    settings: wgpu::Buffer,
}

impl ImageProcessingPostProcess {
    #[must_use]
    pub fn new(gpu: &Gpu, output_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image processing"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image processing"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("image_processing.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image processing"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("image processing"),
            contents: bytemuck::bytes_of(&ImageProcessingConfiguration::default().settings(1, 1)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            pipeline,
            layout,
            settings,
        }
    }

    /// Runs the pass: `source` (the HDR target, `width` × `height`) into
    /// `output`.
    pub fn apply(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        config: &ImageProcessingConfiguration,
        source: &wgpu::TextureView,
        output: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        gpu.queue
            .write_buffer(&self.settings, 0, bytemuck::bytes_of(&config.settings(size.0, size.1)));
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image processing"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.settings.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("image processing"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vignette_settings_follow_bind() {
        let config = ImageProcessingConfiguration {
            exposure: 1.0,
            contrast: 1.05,
            vignette: Some(Vignette {
                weight: 0.3,
                stretch: 0.5,
                ..Vignette::default()
            }),
        };
        let s = config.settings(1280, 720);
        let scale_y = 0.25_f64.tan();
        let scale_x = scale_y * 1280.0 / 720.0;
        let mean = (scale_x * scale_y).sqrt();
        assert!((f64::from(s.vignette_settings1[0]) - (scale_x + mean) / 2.0).abs() < 1e-6);
        assert!((f64::from(s.vignette_settings1[1]) - (scale_y + mean) / 2.0).abs() < 1e-6);
        assert_eq!(s.vignette_settings1[2], 0.0);
        assert!((s.vignette_settings2[3] + 0.6).abs() < 1e-6);
        assert_eq!(s.params, [1.0, 1.05, 1.0, 0.0]);
    }
}
