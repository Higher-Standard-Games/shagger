//! Text drawn as its outline only: CSS's `color: transparent;
//! -webkit-text-stroke: 2px ...`, which the menus' huge backdrop words
//! (`lobby__ghost`, `.twin__ghost`) use. egui draws glyphs from a bitmap
//! atlas and cannot stroke them, so this reads the glyphs' outlines from
//! the font file and strokes them as paths. No SHAG source: the browser
//! does this in CSS.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

/// Collects a glyph's contours as polylines, curves flattened.
struct Contours {
    origin: Pos2,
    contours: Vec<Vec<Pos2>>,
    at: Pos2,
}

impl Contours {
    /// Font units are y-up from the baseline; the screen is y-down.
    fn point(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.origin.x + x, self.origin.y - y)
    }

    fn push(&mut self, p: Pos2) {
        if let Some(c) = self.contours.last_mut() {
            c.push(p);
        }
        self.at = p;
    }
}

/// Segments per curve: plenty at the sizes these words are drawn.
const STEPS: usize = 10;

impl OutlinePen for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.contours.push(vec![p]);
        self.at = p;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.push(p);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (a, c, b) = (self.at, self.point(cx0, cy0), self.point(x, y));
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let u = 1.0 - t;
            self.push(pos2(
                u * u * a.x + 2.0 * u * t * c.x + t * t * b.x,
                u * u * a.y + 2.0 * u * t * c.y + t * t * b.y,
            ));
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, c0, c1, b) = (self.at, self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.push(pos2(
                w0 * a.x + w1 * c0.x + w2 * c1.x + w3 * b.x,
                w0 * a.y + w1 * c0.y + w2 * c1.y + w3 * b.y,
            ));
        }
    }

    fn close(&mut self) {}
}

/// The outlines of `text` set in `font` (a TrueType or OpenType file) at
/// `size` px, its baseline starting at `baseline`, `spacing` px between
/// letters (CSS `letter-spacing`). Returns the width it took; nothing is
/// drawn when the font cannot be read.
pub fn paint_outline_text(
    painter: &Painter,
    font: &[u8],
    text: &str,
    size: f32,
    spacing: f32,
    baseline: Pos2,
    stroke: Stroke,
) -> f32 {
    paint_outline_text_styled(
        painter,
        font,
        text,
        size,
        spacing,
        baseline,
        &OutlineStyle {
            stroke,
            ..OutlineStyle::default()
        },
    )
}

/// How [`paint_outline_text_styled`] draws the letters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineStyle {
    /// The outline (`-webkit-text-stroke`).
    pub stroke: Stroke,
    /// The letters' fill (`color`); transparent for none.
    pub fill: Color32,
    /// CSS `skewX(-angle)` as a slant: each point moves right by this many
    /// px for each px it stands above the baseline (`tan(angle)`).
    pub slant: f32,
}

impl Default for OutlineStyle {
    fn default() -> Self {
        Self {
            stroke: Stroke::NONE,
            fill: Color32::TRANSPARENT,
            slant: 0.0,
        }
    }
}

/// [`paint_outline_text`], slanted and filled as `style` says. The fill is
/// laid in one-pixel rows by the even-odd rule, so a letter's counters
/// (the holes in an O) stay open.
pub fn paint_outline_text_styled(
    painter: &Painter,
    font: &[u8],
    text: &str,
    size: f32,
    spacing: f32,
    baseline: Pos2,
    style: &OutlineStyle,
) -> f32 {
    let Ok(font) = FontRef::new(font) else {
        return 0.0;
    };
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
    let mut x = baseline.x;
    for ch in text.chars() {
        let Some(id) = charmap.map(ch) else { continue };
        if let Some(glyph) = outlines.get(id) {
            let mut pen = Contours {
                origin: pos2(x, baseline.y),
                contours: Vec::new(),
                at: pos2(x, baseline.y),
            };
            let settings = DrawSettings::unhinted(Size::new(size), LocationRef::default());
            if glyph.draw(settings, &mut pen).is_ok() {
                let mut contours = Vec::new();
                for mut contour in pen.contours {
                    // A point twice in a row (a contour's closing point, a
                    // curve ending where the next starts) has no direction,
                    // and its join is drawn as a spike.
                    contour.dedup_by(|a, b| a.distance(*b) < 0.01);
                    if contour.len() > 2 && contour[0].distance(contour[contour.len() - 1]) < 0.01 {
                        contour.pop();
                    }
                    if contour.len() > 2 {
                        if style.slant != 0.0 {
                            for p in &mut contour {
                                p.x += style.slant * (baseline.y - p.y);
                            }
                        }
                        contours.push(contour);
                    }
                }
                if style.fill.a() > 0 {
                    fill_even_odd(painter, &contours, style.fill);
                }
                if style.stroke.width > 0.0 {
                    for contour in contours {
                        painter.add(Shape::closed_line(contour, style.stroke));
                    }
                }
            }
        }
        x += metrics.advance_width(id).unwrap_or(size * 0.6) + spacing;
    }
    x - baseline.x - spacing
}

/// The inside of closed `contours` by the even-odd rule, as one-pixel rows
/// of spans (egui fills only convex shapes).
fn fill_even_odd(painter: &Painter, contours: &[Vec<Pos2>], color: Color32) {
    let (top, bottom) = contours
        .iter()
        .flatten()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    if top > bottom {
        return;
    }
    let mut mesh = Mesh::default();
    let mut crossings = Vec::new();
    let mut y = top.floor();
    while y < bottom {
        // Each row is sampled through its middle.
        let at = y + 0.5;
        crossings.clear();
        for contour in contours {
            for (i, a) in contour.iter().enumerate() {
                let b = contour[(i + 1) % contour.len()];
                if (a.y <= at) != (b.y <= at) {
                    crossings.push(a.x + (at - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        crossings.sort_by(f32::total_cmp);
        for [left, right] in crossings.as_chunks::<2>().0 {
            mesh.add_colored_rect(Rect::from_min_max(pos2(*left, y), pos2(*right, y + 1.0)), color);
        }
        y += 1.0;
    }
    painter.add(mesh);
}

/// The width `text` takes, as [`paint_outline_text`] sets it.
#[must_use]
pub fn outline_text_width(font: &[u8], text: &str, size: f32, spacing: f32) -> f32 {
    let Ok(font) = FontRef::new(font) else {
        return 0.0;
    };
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
    let advances: f32 = text
        .chars()
        .filter_map(|c| charmap.map(c))
        .map(|id| metrics.advance_width(id).unwrap_or(size * 0.6) + spacing)
        .sum();
    advances - spacing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_that_cannot_be_read_draws_nothing() {
        assert_eq!(outline_text_width(b"not a font", "DUEL", 100.0, 0.0), 0.0);
    }
}
