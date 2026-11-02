//! The 2D layer drawn over the 3D view (RUST.md 11): text in the game's
//! fonts, filled rectangles and outlines, in pixels from the top left as
//! CSS lays out. The browser HUD is HTML; each Washoi widget draws here
//! with the same sizes, colours and fonts. `rendering::hud_2d_renderer`
//! puts a frame's batch on screen.

pub mod egui_layer;
pub mod font;
pub mod glyph_atlas;
pub mod hud;
pub mod hud_2d;
