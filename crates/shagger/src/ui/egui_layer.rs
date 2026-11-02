//! egui over the game's own wgpu device (RUST.md 11): menus, the lobby,
//! settings and the console. The HUD stays on `Hud2d`, which matches the
//! browser's CSS; egui draws what the browser builds from HTML forms.
//!
//! One layer per window: feed it winit's events, build a frame with
//! [`EguiLayer::run`], then [`EguiLayer::paint`] it into a render pass over
//! whatever the game drew.

pub use egui;
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use winit::event::WindowEvent;
use winit::window::Window;

use crate::rendering::gpu::Gpu;

/// The paint jobs of one frame, waiting for [`EguiLayer::paint`].
pub struct EguiFrame {
    primitives: Vec<egui::ClippedPrimitive>,
    textures: egui::TexturesDelta,
    pixels_per_point: f32,
}

pub struct EguiLayer {
    pub ctx: egui::Context,
    state: egui_winit::State,
    renderer: Renderer,
}

impl EguiLayer {
    #[must_use]
    pub fn new(gpu: &Gpu, window: &Window, format: wgpu::TextureFormat) -> Self {
        let ctx = egui::Context::default();
        let viewport = ctx.viewport_id();
        let state = egui_winit::State::new(ctx.clone(), viewport, window, None, None, None);
        let renderer = Renderer::new(&gpu.device, format, RendererOptions::default());
        Self { ctx, state, renderer }
    }

    /// A window event; true when egui used it (a click on a button, typing
    /// in a field), so the game should not.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// Builds one frame of UI.
    pub fn run(&mut self, window: &Window, ui: impl FnMut(&mut egui::Ui)) -> EguiFrame {
        let input = self.state.take_egui_input(window);
        let output = self.ctx.run_ui(input, ui);
        self.state.handle_platform_output(window, output.platform_output);
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        EguiFrame {
            primitives,
            textures: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        }
    }

    /// Uploads the frame's textures and buffers and draws it into `view`,
    /// keeping what is already there.
    pub fn paint(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: [u32; 2],
        frame: EguiFrame,
    ) {
        let screen = ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: frame.pixels_per_point,
        };
        for (id, deltas) in &frame.textures.set {
            for delta in deltas {
                self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let commands = self
            .renderer
            .update_buffers(&gpu.device, &gpu.queue, encoder, &frame.primitives, &screen);
        if !commands.is_empty() {
            gpu.queue.submit(commands);
        }
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &frame.primitives, &screen);
        }
        for id in &frame.textures.free {
            self.renderer.free_texture(id);
        }
    }
}
