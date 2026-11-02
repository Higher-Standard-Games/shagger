//! egui paints offscreen: a button's label shows up in the read-back frame.
//! Needs a GPU adapter, software ones included; it reports a skip without.

use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};

#[test]
fn egui_draws_into_an_offscreen_frame() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (w, h) = (320u32, 200u32);
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32, h as f32),
        )),
        ..Default::default()
    };
    // The first frame uploads the fonts; the second is the one looked at.
    let draw = |ui: &mut egui::Ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Quick play");
        });
    };
    let first = layer.run_input(input(), draw);
    render_encoded_to_rgba(&gpu, w, h, wgpu::Color::BLACK, |encoder, view| {
        layer.paint(&gpu, encoder, view, [w, h], first);
    });
    let frame = layer.run_input(input(), draw);
    let pixels = render_encoded_to_rgba(&gpu, w, h, wgpu::Color::BLACK, |encoder, view| {
        layer.paint(&gpu, encoder, view, [w, h], frame);
    });
    // The panel fills the frame in egui's dark grey, and the heading is lighter (text is grey 140).
    let bright = pixels.chunks(4).filter(|p| p[0] > 100).count();
    let panel = pixels.chunks(4).filter(|p| p[0] > 20 && p[0] < 40).count();
    assert!(panel > (w * h / 2) as usize, "panel {panel}");
    assert!(bright > 20, "bright {bright}");
}
