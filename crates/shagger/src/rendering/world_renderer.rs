//! Draws [`WorldGeometry`] with wgpu: one vertex and index buffer for the map,
//! the lightmap pages as one array texture (and the deluxe pages as another),
//! and a base texture per batch.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use super::gpu::Gpu;
use super::world_geometry::{WorldGeometry, WorldVertex};
use crate::loader::bsp::bsp_lighting_profile::bsp_lighting_profile;
use crate::loader::bsp::bsp_loader::BspFile;
use crate::loader::bsp::mesh::lightmaps::bsp_lightmap_array_rgba;

/// The depth buffer format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    lighting: [f32; 4],
    ambient: [f32; 4],
}

/// A decoded RGBA8 image.
#[derive(Debug, Clone)]
pub struct Rgba8Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba8Image {
    /// One white pixel: what a surface whose texture is missing draws with.
    #[must_use]
    pub fn white() -> Self {
        Self {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        }
    }
}

struct DrawBatch {
    bind_group: usize,
    two_sided: bool,
    first_index: u32,
    index_count: u32,
}

pub struct WorldRenderer {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_bind_groups: Vec<wgpu::BindGroup>,
    batches: Vec<DrawBatch>,
    pipeline_culled: wgpu::RenderPipeline,
    pipeline_two_sided: wgpu::RenderPipeline,
    lighting: [f32; 4],
    ambient: [f32; 4],
}

fn mip_chain(image: &Rgba8Image) -> Vec<Rgba8Image> {
    let mut levels = vec![image.clone()];
    let mut current = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
        .expect("the pixel count matches the size");
    while current.width() > 1 || current.height() > 1 {
        let (w, h) = ((current.width() / 2).max(1), (current.height() / 2).max(1));
        current = image::imageops::resize(&current, w, h, image::imageops::FilterType::Triangle);
        levels.push(Rgba8Image {
            width: w,
            height: h,
            pixels: current.as_raw().clone(),
        });
    }
    levels
}

fn upload_texture(gpu: &Gpu, label: &str, image: &Rgba8Image) -> wgpu::Texture {
    let levels = mip_chain(image);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Babylon samples and writes these bytes as they are (no sRGB decode).
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, data) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
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
            wgpu::Extent3d {
                width: data.width,
                height: data.height,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

fn upload_layers(gpu: &Gpu, label: &str, size: u32, layers: u32, rgba: &[u8]) -> wgpu::Texture {
    gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba,
    )
}

fn array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

impl WorldRenderer {
    /// Uploads the map. `textures` gives each batch's base image by texture
    /// name (`None` draws white), and `color_format` is the target's format.
    pub fn new(
        gpu: &Gpu,
        bsp: &BspFile,
        geometry: &WorldGeometry,
        mut textures: impl FnMut(&str) -> Option<Rgba8Image>,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        let device = &gpu.device;
        let profile = bsp_lighting_profile(bsp);

        // Lightmap pages as layers; deluxe pages, when present, as a second array.
        let size = bsp.lightmap_size;
        let stride = geometry.lightmap_stride;
        let raw = bsp.common.header.is_fbsp;
        let pages: Vec<&[u8]> = bsp.lightmaps.iter().step_by(stride).map(Vec::as_slice).collect();
        let white_page = vec![255u8; (size * size * 3) as usize];
        let pages = if pages.is_empty() {
            vec![white_page.as_slice()]
        } else {
            pages
        };
        let lightmap = upload_layers(
            gpu,
            "lightmap_layers",
            size,
            pages.len() as u32,
            &bsp_lightmap_array_rgba(&pages, size as usize, raw, false),
        );
        let directions: Vec<&[u8]> = if stride == 2 {
            bsp.lightmaps.iter().skip(1).step_by(2).map(Vec::as_slice).collect()
        } else {
            vec![white_page.as_slice()]
        };
        let direction = upload_layers(
            gpu,
            "lightmap_direction_layers",
            size,
            directions.len() as u32,
            &bsp_lightmap_array_rgba(&directions, size as usize, true, true),
        );

        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let base_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("base"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, wgpu::TextureViewDimension::D2Array),
                texture_entry(2, wgpu::TextureViewDimension::D2Array),
                sampler_entry(3),
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world base texture"),
            entries: &[texture_entry(0, wgpu::TextureViewDimension::D2), sampler_entry(1)],
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&array_view(&lightmap)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&array_view(&direction)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
            ],
        });

        // One bind group per distinct texture name.
        let mut by_name: HashMap<String, usize> = HashMap::new();
        let mut texture_bind_groups = Vec::new();
        let mut batches = Vec::new();
        for batch in &geometry.batches {
            let key = batch.texture_name.to_lowercase();
            let bind_group = *by_name.entry(key).or_insert_with(|| {
                let image = textures(&batch.texture_name).unwrap_or_else(Rgba8Image::white);
                let texture = upload_texture(gpu, &batch.texture_name, &image);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                texture_bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&batch.texture_name),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&base_sampler),
                        },
                    ],
                }));
                texture_bind_groups.len() - 1
            });
            batches.push(DrawBatch {
                bind_group,
                two_sided: batch.two_sided,
                first_index: batch.first_index,
                index_count: batch.index_count,
            });
        }

        let shader = device.create_shader_module(wgpu::include_wgsl!("world.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = |cull: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<WorldVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // The Q3-to-Babylon swap mirrors the map and the builders
                    // reverse the winding, so a face seen from its front is
                    // counter-clockwise on screen (checked on washoi_dm2:
                    // the other way culls the walls you look at).
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline_culled = pipeline(Some(wgpu::Face::Back));
        let pipeline_two_sided = pipeline(None);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world vertices"),
            contents: bytemuck::cast_slice(&geometry.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world indices"),
            contents: bytemuck::cast_slice(&geometry.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            vertex_buffer,
            index_buffer,
            globals_buffer,
            globals_bind_group,
            texture_bind_groups,
            batches,
            pipeline_culled,
            pipeline_two_sided,
            lighting: [profile.gain as f32, if stride == 2 { 1.0 } else { 0.0 }, 0.0, 0.0],
            ambient: [
                profile.ambient[0] as f32,
                profile.ambient[1] as f32,
                profile.ambient[2] as f32,
                0.0,
            ],
        }
    }

    /// How many base textures were uploaded.
    #[must_use]
    pub fn texture_count(&self) -> usize {
        self.texture_bind_groups.len()
    }

    /// Writes this frame's camera.
    pub fn prepare(&self, gpu: &Gpu, view_proj: Mat4) {
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            lighting: self.lighting,
            ambient: self.ambient,
        };
        gpu.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
    }

    /// Records the world into an open pass (colour and depth already bound).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let mut two_sided = None;
        for batch in &self.batches {
            if two_sided != Some(batch.two_sided) {
                pass.set_pipeline(if batch.two_sided {
                    &self.pipeline_two_sided
                } else {
                    &self.pipeline_culled
                });
                two_sided = Some(batch.two_sided);
            }
            pass.set_bind_group(1, &self.texture_bind_groups[batch.bind_group], &[]);
            pass.draw_indexed(batch.first_index..batch.first_index + batch.index_count, 0, 0..1);
        }
    }
}

fn texture_entry(binding: u32, dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
