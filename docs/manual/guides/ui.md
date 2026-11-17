# UI

> Behaviour: SHAG's `src/engine/ui` and its CSS (`styles/*.css`). Sizes, colours and fonts follow the browser's, so the native menus and HUD look like the web client's.

Source: `crates/shagger/src/ui/`, `crates/shagger/src/rendering/hud_2d_renderer.rs`.

The browser builds its HUD and menus from HTML and CSS over the canvas. SHAGGER draws them in two layers (RUST.md 11):

| Layer | For | Module |
| --- | --- | --- |
| `Hud2d` | the HUD: text in the game's fonts, filled and gradient shapes, outlines, in CSS pixels | `ui::hud_2d`, drawn by `rendering::hud_2d_renderer` |
| egui | menus, the lobby, settings and the console: what the browser builds from HTML forms | `ui::egui_layer` |

## The HUD batch

`ui::hud_2d::Hud2d` collects a frame's 2D shapes and text as vertices over one glyph atlas. Coordinates are CSS pixels from the top left, and `scale` maps them to the target's pixels, as the browser's `devicePixelRatio` does. Colours are straight (not premultiplied) RGBA from 0 to 1, in the display's gamma, as CSS colours are; `hud_2d::hex("#f4ead5", 1.0)` reads a CSS hex colour.

```rust
use shagger::ui::{font::Font, hud_2d::{Hud2d, TextStyle, hex}};

let mut hud = Hud2d::new();
let rajdhani = hud.add_font(Font::from_bytes(font_bytes)?);

// Each frame:
hud.begin(width_px, height_px, scale); // the target's size in pixels, and CSS pixels to target pixels
hud.rect(16.0, 16.0, 200.0, 40.0, hex("#1a1426", 0.8));
hud.text(24.0, 44.0, "100", &TextStyle::new(rajdhani, 28.0, hex("#f4ead5", 1.0)));
renderer.prepare(&gpu, &mut hud);
renderer.draw(&mut pass);
```

`ui::font` reads TrueType and OpenType fonts with `swash`. The game ships its own (Orbitron, Rajdhani), as the browser loads them with `@font-face`.

## The egui layer

`ui::egui_layer::EguiLayer` runs egui over the game's own wgpu device. One layer serves one window: feed it winit's events with `on_window_event`, build a frame with `run(window, |ui| ...)`, then `paint` it into a render pass over whatever the game drew. `EguiLayer::offscreen` and `run_input` do the same without a window, for tests and screenshots (`tests/egui_offscreen.rs`).

A widget that needs the GPU while it is laid out, such as a model preview, queues a job with `queue_gpu_job(ctx, job)`. The job runs in the next `paint`, before egui draws, with the frame's encoder and the layer's images (`NativeTextures`), and `rendering::model_stage` uses it to draw models into an image a panel shows.

## Shared components

`ui::components` ports SHAG's `src/engine/ui/components` and their CSS: the connecting overlay (the match handoff), the lobby twin (the rail and stage backdrop), the match lobby room view, the raised 3D key, the text field, and outline-only text for the menus' backdrop words. `ui::transfer_loading_view` is the map loading screen and `ui::hud::damage_overlay` the damage vignette.

Washoi's own screens and HUD widgets are built from these in wash-fps-rust.
