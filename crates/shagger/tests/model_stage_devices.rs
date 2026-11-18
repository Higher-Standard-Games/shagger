//! A model stage kept across two windows' GPU devices, as the lobby's
//! settings previews are across a match: it makes its GPU half again on
//! the second device and tells its scene, where using the first device's
//! buffers on the second's queue panicked in wgpu. Needs a GPU adapter,
//! software ones included; it reports a skip without.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use shagger::rendering::glam::{Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::ModelRenderer;
use shagger::rendering::model_stage::{ModelStage, StageScene, StageView};
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};

/// Draws nothing, but has the stage prepare its renderer every frame.
struct Empty {
    lost: Arc<AtomicUsize>,
}

impl StageScene for Empty {
    fn frame(&mut self, _: &Gpu, _: &mut ModelRenderer, _: (u32, u32), _: f64) -> Option<StageView> {
        Some(StageView {
            view_proj: Mat4::IDENTITY,
            eye: Vec3::ZERO,
            ..StageView::default()
        })
    }

    fn renderer_lost(&mut self) {
        self.lost.fetch_add(1, Ordering::Relaxed);
    }
}

/// Two frames of a window holding `stage`, on `gpu`.
fn show(gpu: &Gpu, stage: &mut ModelStage<Empty>) {
    let (w, h) = (64u32, 64u32);
    let mut layer = EguiLayer::offscreen(gpu, OFFSCREEN_FORMAT);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32, h as f32),
        )),
        ..Default::default()
    };
    for _ in 0..2 {
        let frame = layer.run_input(input(), |ui| {
            stage.ui(ui, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(32.0, 32.0)));
        });
        render_encoded_to_rgba(gpu, w, h, wgpu::Color::BLACK, |encoder, view| {
            layer.paint(gpu, encoder, view, [w, h], frame);
        });
    }
}

#[test]
fn a_stage_moves_to_a_new_device() {
    let (Ok(first), Ok(second)) = (Gpu::headless(), Gpu::headless()) else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let lost = Arc::new(AtomicUsize::new(0));
    let mut stage = ModelStage::new(Empty { lost: lost.clone() });
    show(&first, &mut stage);
    assert_eq!(lost.load(Ordering::Relaxed), 0);
    show(&second, &mut stage);
    assert_eq!(lost.load(Ordering::Relaxed), 1);
    // And back again, as the next match and lobby do.
    show(&first, &mut stage);
    assert_eq!(lost.load(Ordering::Relaxed), 2);
}
