//! A frame's 2D shapes and text, as vertices over one glyph atlas.
//!
//! Coordinates are CSS pixels from the top left; `scale` maps them to the
//! target's pixels (the browser's `devicePixelRatio`). Colours are
//! straight (not premultiplied) RGBA in 0 to 1, in the display's gamma, as
//! CSS colours are.

use bytemuck::{Pod, Zeroable};
use swash::scale::ScaleContext;

use crate::ui::font::Font;
use crate::ui::glyph_atlas::GlyphAtlas;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct HudVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// A font loaded into a [`Hud2d`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub usize);

/// Text alignment against the x given (`text-align`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// How a run of text looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    /// `font-size` in CSS pixels.
    pub size: f32,
    pub color: [f32; 4],
    /// `letter-spacing` in CSS pixels.
    pub letter_spacing: f32,
    pub align: Align,
    /// `text-shadow`: offset in CSS pixels and colour (no blur).
    pub shadow: Option<([f32; 2], [f32; 4])>,
}

impl TextStyle {
    #[must_use]
    pub fn new(font: FontId, size: f32, color: [f32; 4]) -> Self {
        Self {
            font,
            size,
            color,
            letter_spacing: 0.0,
            align: Align::Left,
            shadow: None,
        }
    }

    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    #[must_use]
    pub fn spacing(mut self, letter_spacing: f32) -> Self {
        self.letter_spacing = letter_spacing;
        self
    }

    #[must_use]
    pub fn shadow(mut self, offset: [f32; 2], color: [f32; 4]) -> Self {
        self.shadow = Some((offset, color));
        self
    }
}

/// `#rrggbb` or `#rrggbbaa` (or `#rgb`) with an alpha on top.
#[must_use]
pub fn hex(color: &str, alpha: f32) -> [f32; 4] {
    let digits = color.trim_start_matches('#');
    let channel = |i: usize, n: usize| -> f32 {
        let s = &digits[i * n..i * n + n];
        let v = u8::from_str_radix(s, 16).unwrap_or(0);
        let v = if n == 1 { v * 17 } else { v };
        f32::from(v) / 255.0
    };
    match digits.len() {
        3 => [channel(0, 1), channel(1, 1), channel(2, 1), alpha],
        8 => [channel(0, 2), channel(1, 2), channel(2, 2), channel(3, 2) * alpha],
        6 => [channel(0, 2), channel(1, 2), channel(2, 2), alpha],
        _ => [1.0, 1.0, 1.0, alpha],
    }
}

/// The frame's shapes, the fonts and their atlas.
pub struct Hud2d {
    fonts: Vec<Font>,
    pub atlas: GlyphAtlas,
    context: ScaleContext,
    pub vertices: Vec<HudVertex>,
    pub indices: Vec<u32>,
    /// The target's size in its pixels, and CSS pixels to target pixels.
    pub target: [f32; 2],
    pub scale: f32,
}

impl Default for Hud2d {
    fn default() -> Self {
        Self::new()
    }
}

impl Hud2d {
    #[must_use]
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            atlas: GlyphAtlas::new(1024),
            context: ScaleContext::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            target: [1.0, 1.0],
            scale: 1.0,
        }
    }

    pub fn add_font(&mut self, font: Font) -> FontId {
        self.fonts.push(font);
        FontId(self.fonts.len() - 1)
    }

    /// Starts a frame on a target `width` by `height` pixels.
    pub fn begin(&mut self, width: f32, height: f32, scale: f32) {
        self.vertices.clear();
        self.indices.clear();
        self.target = [width.max(1.0), height.max(1.0)];
        self.scale = scale.max(0.01);
    }

    /// The viewport in CSS pixels.
    #[must_use]
    pub fn size(&self) -> [f32; 2] {
        [self.target[0] / self.scale, self.target[1] / self.scale]
    }

    fn quad(&mut self, p0: [f32; 2], p1: [f32; 2], uv0: [f32; 2], uv1: [f32; 2], color: [f32; 4]) {
        let base = self.vertices.len() as u32;
        for (position, uv) in [
            ([p0[0], p0[1]], [uv0[0], uv0[1]]),
            ([p1[0], p0[1]], [uv1[0], uv0[1]]),
            ([p0[0], p1[1]], [uv0[0], uv1[1]]),
            ([p1[0], p1[1]], [uv1[0], uv1[1]]),
        ] {
            self.vertices.push(HudVertex { position, uv, color });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    /// A filled rectangle in CSS pixels.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        let s = self.scale;
        let uv = self.atlas.white_uv();
        self.quad([x * s, y * s], [(x + w) * s, (y + h) * s], uv, uv, color);
    }

    /// A rectangle's border, `width` wide inside it.
    pub fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, color: [f32; 4]) {
        self.rect(x, y, w, width, color);
        self.rect(x, y + h - width, w, width, color);
        self.rect(x, y + width, width, h - 2.0 * width, color);
        self.rect(x + w - width, y + width, width, h - 2.0 * width, color);
    }

    /// A horizontal fill from `from` to `to` in the colours given
    /// (`linear-gradient(to right, ...)`).
    pub fn gradient_rect(&mut self, x: f32, y: f32, w: f32, h: f32, from: [f32; 4], to: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let s = self.scale;
        let uv = self.atlas.white_uv();
        let base = self.vertices.len() as u32;
        for (position, color) in [
            ([x * s, y * s], from),
            ([(x + w) * s, y * s], to),
            ([x * s, (y + h) * s], from),
            ([(x + w) * s, (y + h) * s], to),
        ] {
            self.vertices.push(HudVertex { position, uv, color });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    /// A convex polygon in CSS pixels, a colour per point, filled as a fan
    /// from the first (a `clip-path: polygon(...)` with its background
    /// sampled at the points).
    pub fn polygon(&mut self, points: &[[f32; 2]], colors: &[[f32; 4]]) {
        if points.len() < 3 || colors.len() != points.len() {
            return;
        }
        let s = self.scale;
        let uv = self.atlas.white_uv();
        let base = self.vertices.len() as u32;
        for (p, &color) in points.iter().zip(colors) {
            self.vertices.push(HudVertex {
                position: [p[0] * s, p[1] * s],
                uv,
                color,
            });
        }
        for i in 1..points.len() as u32 - 1 {
            self.indices.extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }

    /// A canvas `createRadialGradient` from one centre: `stops` are
    /// (radius, colour) in CSS pixels, ascending. Inside the first radius
    /// is its colour; past the last, nothing is drawn.
    pub fn radial_gradient(&mut self, cx: f32, cy: f32, stops: &[(f32, [f32; 4])]) {
        const SEGMENTS: usize = 64;
        let Some(&(r0, c0)) = stops.first() else {
            return;
        };
        let at = |r: f32, i: usize| {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            [cx + r * a.cos(), cy + r * a.sin()]
        };
        if c0[3] > 0.0 && r0 > 0.0 {
            let mut points = Vec::with_capacity(SEGMENTS + 1);
            points.push([cx, cy]);
            points.extend((0..=SEGMENTS).map(|i| at(r0, i)));
            self.polygon(&points, &vec![c0; points.len()]);
        }
        for pair in stops.windows(2) {
            let ((ra, ca), (rb, cb)) = (pair[0], pair[1]);
            if ca[3] <= 0.0 && cb[3] <= 0.0 {
                continue;
            }
            for i in 0..SEGMENTS {
                self.polygon(&[at(ra, i), at(rb, i), at(rb, i + 1), at(ra, i + 1)], &[ca, cb, cb, ca]);
            }
        }
    }

    /// The width of `text` in CSS pixels.
    #[must_use]
    pub fn measure(&self, text: &str, style: &TextStyle) -> f32 {
        let Some(font) = self.fonts.get(style.font.0) else {
            return 0.0;
        };
        let px = (style.size * self.scale).round().max(1.0);
        let width: f32 = text
            .chars()
            .map(|ch| font.advance(font.glyph(ch), px) + style.letter_spacing * self.scale)
            .sum();
        width / self.scale
    }

    /// Ascent and descent of a style's font in CSS pixels.
    #[must_use]
    pub fn line_metrics(&self, style: &TextStyle) -> (f32, f32) {
        let Some(font) = self.fonts.get(style.font.0) else {
            return (style.size * 0.8, style.size * 0.2);
        };
        let px = (style.size * self.scale).round().max(1.0);
        let (ascent, descent) = font.vertical(px);
        (ascent / self.scale, descent / self.scale)
    }

    /// Draws `text` with its baseline at `y` (CSS pixels); returns its
    /// width.
    pub fn text(&mut self, x: f32, y: f32, text: &str, style: &TextStyle) -> f32 {
        let width = self.measure(text, style);
        let left = match style.align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        if let Some((offset, color)) = style.shadow {
            self.run(left + offset[0], y + offset[1], text, style, color);
        }
        self.run(left, y, text, style, style.color);
        width
    }

    /// Draws `text` centred vertically in a box from `top` that is
    /// `height` tall (a flex row's `align-items: center`).
    pub fn text_in_box(&mut self, x: f32, top: f32, height: f32, text: &str, style: &TextStyle) -> f32 {
        let (ascent, descent) = self.line_metrics(style);
        let baseline = top + (height + ascent - descent) / 2.0;
        self.text(x, baseline, text, style)
    }

    fn run(&mut self, x: f32, y: f32, text: &str, style: &TextStyle, color: [f32; 4]) {
        if self.fonts.get(style.font.0).is_none() || color[3] <= 0.0 {
            return;
        }
        let px = (style.size * self.scale).round().max(1.0);
        let mut pen = (x * self.scale).round();
        let baseline = (y * self.scale).round();
        let inv = 1.0 / self.atlas.size as f32;
        for ch in text.chars() {
            let font = &self.fonts[style.font.0];
            let glyph = font.glyph(ch);
            let advance = font.advance(glyph, px);
            let key = (style.font.0, glyph, px as u32);
            let placed = match self.atlas.get(key) {
                Some(placed) => placed,
                None => {
                    let bitmap = font.rasterize(&mut self.context, glyph, px).unwrap_or_default();
                    let placed = self.atlas.insert(
                        key,
                        bitmap.left,
                        bitmap.top,
                        bitmap.width,
                        bitmap.height,
                        &bitmap.coverage,
                    );
                    if placed.is_none() && bitmap.width > 0 && self.atlas.get(key).is_none() {
                        // Full: start over; this frame's earlier glyphs
                        // draw from texels that change, for one frame.
                        self.atlas.clear();
                        self.atlas.insert(
                            key,
                            bitmap.left,
                            bitmap.top,
                            bitmap.width,
                            bitmap.height,
                            &bitmap.coverage,
                        )
                    } else {
                        placed
                    }
                }
            };
            if let Some(g) = placed {
                let [gx, gy, gw, gh] = g.rect;
                let p0 = [pen + g.left as f32, baseline - g.top as f32];
                let p1 = [p0[0] + gw as f32, p0[1] + gh as f32];
                let uv0 = [gx as f32 * inv, gy as f32 * inv];
                let uv1 = [(gx + gw) as f32 * inv, (gy + gh) as f32 * inv];
                self.quad(p0, p1, uv0, uv1, color);
            }
            pen += (advance + style.letter_spacing * self.scale).round();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_css_colours() {
        assert_eq!(hex("#ff8000", 1.0), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        assert_eq!(hex("#fff", 0.5), [1.0, 1.0, 1.0, 0.5]);
        assert_eq!(hex("#00000080", 1.0)[3], 128.0 / 255.0);
    }

    #[test]
    fn rects_scale_to_the_target() {
        let mut hud = Hud2d::new();
        hud.begin(200.0, 100.0, 2.0);
        assert_eq!(hud.size(), [100.0, 50.0]);
        hud.rect(10.0, 5.0, 20.0, 10.0, [1.0; 4]);
        hud.outline(0.0, 0.0, 10.0, 10.0, 1.0, [1.0; 4]);
        assert_eq!(hud.vertices[0].position, [20.0, 10.0]);
        assert_eq!(hud.vertices[3].position, [60.0, 30.0]);
        assert_eq!(hud.indices.len(), 6 * 5);
        hud.polygon(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]], &[[1.0; 4]; 4]);
        assert_eq!(hud.indices.len(), 6 * 6);
        assert_eq!(hud.vertices.last().unwrap().position, [0.0, 8.0]);
        // Text without a font draws nothing and measures nothing.
        let style = TextStyle::new(FontId(0), 16.0, [1.0; 4]);
        assert_eq!(hud.text(0.0, 0.0, "x", &style), 0.0);
    }
}

#[cfg(test)]
mod with_a_font {
    use super::*;

    #[test]
    fn lays_out_text_when_given_a_font() {
        let Ok(path) = std::env::var("SHAGGER_TEST_FONT") else {
            return;
        };
        let mut hud = Hud2d::new();
        let font = hud.add_font(Font::from_bytes(std::fs::read(path).unwrap()).unwrap());
        hud.begin(800.0, 600.0, 1.0);
        let style = TextStyle::new(font, 32.0, [1.0; 4]);
        let one = hud.measure("8", &style);
        assert!(one > 8.0 && one < 32.0, "{one}");
        assert!((hud.measure("88", &style.spacing(2.0)) - 2.0 * (one + 2.0)).abs() < 0.01);
        let w = hud.text(400.0, 300.0, "8:05", &style.align(Align::Center));
        assert_eq!(hud.indices.len(), 6 * 4);
        let (left, right) = (hud.vertices[0].position[0], hud.vertices[12].position[0]);
        assert!(
            left >= 400.0 - w / 2.0 - 1.0 && right < 400.0 + w / 2.0,
            "{left} {right} {w}"
        );
        // The glyphs are in the atlas now, and a second run adds none.
        let used = hud.atlas.pixels.iter().filter(|&&p| p > 0).count();
        hud.text(0.0, 50.0, "58:0", &style);
        assert_eq!(hud.atlas.pixels.iter().filter(|&&p| p > 0).count(), used);
    }
}
