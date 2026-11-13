//! A few models, lit and posed, drawn into an image a UI panel shows: the
//! native side of SHAG's preview scenes (wash-fps `WeaponPreview.ts`,
//! `PlayerTeamPreview.ts`), each of which runs a small Babylon engine and
//! scene of its own on a transparent canvas.
//!
//! The stage keeps its own [`ModelRenderer`] (models are uploaded to it, not
//! to the match's), an HDR target with image processing into `Rgba8Unorm`,
//! as the window's frame goes, and the egui image that shows the result.
//! What it draws comes from a [`StageScene`], which the caller owns through
//! the stage. A panel calls [`ModelStage::ui`] while it is laid out: the
//! image shows from the frame after the first, and the scene is drawn
//! again in each [`EguiLayer::paint`](crate::ui::egui_layer::EguiLayer::paint).
//!
//! Drawn at twice the panel's pixels and filtered down, in place of the
//! canvas's multisampling.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use glam::{Mat4, Vec3};

use super::gpu::Gpu;
use super::hdr_target::{HDR_FORMAT, HdrTarget};
use super::model_renderer::{ModelDraw, ModelLight, ModelRenderer};
use super::offscreen::begin_main_pass;
use crate::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess,
};
use crate::ui::egui_layer::{NativeTextures, egui, queue_gpu_job};

/// The image's format, as egui takes a native image.
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Pixels drawn per pixel shown.
const SUPERSAMPLE: f32 = 2.0;

/// What a scene asks the stage to draw this frame.
#[derive(Debug, Clone, Default)]
pub struct StageView {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub lights: Vec<ModelLight>,
    pub draws: Vec<ModelDraw>,
    /// The joint palette the skinned draws index.
    pub joints: Vec<Mat4>,
}

/// What a stage shows. `models` is the stage's own renderer: upload to it
/// on the first frame and keep the handles. `size` is the image in pixels;
/// `time` the seconds since the stage was made. `None` draws nothing (the
/// model is not loaded).
pub trait StageScene: Send {
    fn frame(&mut self, gpu: &Gpu, models: &mut ModelRenderer, size: (u32, u32), time: f64) -> Option<StageView>;
}

/// The stage's GPU half, made in the first paint.
struct Target {
    models: ModelRenderer,
    post: ImageProcessingPostProcess,
    hdr: HdrTarget,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    texture: egui::TextureId,
}

struct Shared<S> {
    scene: S,
    target: Option<Target>,
    /// The image is drawn: the panel may show it.
    shown: Option<egui::TextureId>,
    start: std::time::Instant,
}

/// A stage and its scene. Dropping it lets go of the image.
pub struct ModelStage<S: StageScene + 'static> {
    shared: Arc<Mutex<Shared<S>>>,
    ctx: Option<egui::Context>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn output_texture(gpu: &Gpu, size: (u32, u32)) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("model stage"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

impl<S: StageScene + 'static> Shared<S> {
    fn paint(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        textures: &mut NativeTextures<'_>,
        size: (u32, u32),
    ) {
        let size = (size.0.max(1), size.1.max(1));
        let target = match &mut self.target {
            Some(t) => t,
            None => {
                let output = output_texture(gpu, size);
                let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
                let texture = textures.register(&output_view);
                self.target.insert(Target {
                    models: ModelRenderer::new(gpu, HDR_FORMAT),
                    post: ImageProcessingPostProcess::new(gpu, OUTPUT_FORMAT),
                    hdr: HdrTarget::new(gpu, size.0, size.1),
                    output,
                    output_view,
                    texture,
                })
            }
        };
        if target.hdr.size != size {
            target.hdr = HdrTarget::new(gpu, size.0, size.1);
            target.output = output_texture(gpu, size);
            target.output_view = target.output.create_view(&wgpu::TextureViewDescriptor::default());
            textures.update(target.texture, &target.output_view);
        }
        let time = self.start.elapsed().as_secs_f64();
        let Some(view) = self.scene.frame(gpu, &mut target.models, size, time) else {
            self.shown = None;
            return;
        };
        target.models.set_lights(view.lights);
        target
            .models
            .prepare(gpu, view.view_proj, view.eye, &view.draws, &view.joints);
        {
            let mut pass = begin_main_pass(encoder, &target.hdr.color, &target.hdr.depth, wgpu::Color::TRANSPARENT);
            target.models.draw_opaque(&mut pass);
            target.models.draw_blended(&mut pass);
        }
        target.hdr.resolve(
            gpu,
            encoder,
            &target.post,
            &ImageProcessingConfiguration::default(),
            &target.output_view,
        );
        self.shown = Some(target.texture);
    }
}

impl<S: StageScene + 'static> ModelStage<S> {
    #[must_use]
    pub fn new(scene: S) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                scene,
                target: None,
                shown: None,
                start: std::time::Instant::now(),
            })),
            ctx: None,
        }
    }

    /// The scene, to change what it shows.
    pub fn scene(&self) -> impl std::ops::DerefMut<Target = S> + '_ {
        struct SceneGuard<'a, S>(MutexGuard<'a, Shared<S>>);
        impl<S> std::ops::Deref for SceneGuard<'_, S> {
            type Target = S;
            fn deref(&self) -> &S {
                &self.0.scene
            }
        }
        impl<S> std::ops::DerefMut for SceneGuard<'_, S> {
            fn deref_mut(&mut self) -> &mut S {
                &mut self.0.scene
            }
        }
        SceneGuard(lock(&self.shared))
    }

    /// Shows the last image in `rect` (once there is one) and asks the next
    /// paint for a new one at `rect`'s size.
    pub fn ui(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        let scale = ctx.pixels_per_point() * SUPERSAMPLE;
        let size = (
            (rect.width() * scale).round().clamp(1.0, 4096.0) as u32,
            (rect.height() * scale).round().clamp(1.0, 4096.0) as u32,
        );
        if let Some(texture) = lock(&self.shared).shown {
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let shared = Arc::clone(&self.shared);
        queue_gpu_job(
            &ctx,
            Box::new(move |gpu, encoder, textures| lock(&shared).paint(gpu, encoder, textures, size)),
        );
        self.ctx = Some(ctx);
    }
}

impl<S: StageScene + 'static> Drop for ModelStage<S> {
    fn drop(&mut self) {
        let Some(ctx) = &self.ctx else { return };
        let shared = Arc::clone(&self.shared);
        queue_gpu_job(
            ctx,
            Box::new(move |_, _, textures| {
                if let Some(target) = lock(&shared).target.take() {
                    textures.free(target.texture);
                }
            }),
        );
    }
}

/// `ArcRotateCamera`'s position: `alpha` about the up axis, `beta` from it,
/// `radius` from `target` (Babylon is left-handed with y up, as the
/// models' world here is).
#[must_use]
pub fn arc_rotate_eye(alpha: f32, beta: f32, radius: f32, target: Vec3) -> Vec3 {
    target
        + Vec3::new(
            radius * alpha.cos() * beta.sin(),
            radius * beta.cos(),
            radius * alpha.sin() * beta.sin(),
        )
}
