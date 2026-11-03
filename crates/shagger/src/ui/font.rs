//! A TrueType or OpenType font read with `swash` (MIT or Apache-2.0): the
//! game ships its own (Orbitron, Rajdhani), as the browser loads them with
//! `@font-face`.

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::{CacheKey, FontRef};

/// A font's bytes and where its table directory is.
pub struct Font {
    data: Vec<u8>,
    offset: u32,
    key: CacheKey,
}

/// One glyph drawn at a size: coverage rows and where they sit against the
/// pen (`left` right of it, `top` above the baseline).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlyphBitmap {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
}

impl Font {
    /// The first font in a file.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        let (offset, key) = {
            let font = FontRef::from_index(&data, 0).ok_or("not a font")?;
            (font.offset, font.key)
        };
        Ok(Self { data, offset, key })
    }

    fn font(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }

    /// The glyph for a character (0 when the font has none).
    #[must_use]
    pub fn glyph(&self, ch: char) -> u16 {
        self.font().charmap().map(ch)
    }

    /// How far the pen moves after `glyph` at `size` pixels to the em.
    #[must_use]
    pub fn advance(&self, glyph: u16, size: f32) -> f32 {
        self.font().glyph_metrics(&[]).scale(size).advance_width(glyph)
    }

    /// Ascent and descent (both positive) at `size`.
    #[must_use]
    pub fn vertical(&self, size: f32) -> (f32, f32) {
        let m = self.font().metrics(&[]).scale(size);
        (m.ascent, m.descent)
    }

    /// Draws one character's glyph at `size` with a context of its own, for
    /// a one-off atlas.
    #[must_use]
    pub fn rasterize_char(&self, ch: char, size: f32) -> Option<GlyphBitmap> {
        self.rasterize(&mut ScaleContext::new(), self.glyph(ch), size)
    }

    /// Draws a glyph's coverage at `size`, hinted.
    pub fn rasterize(&self, context: &mut ScaleContext, glyph: u16, size: f32) -> Option<GlyphBitmap> {
        let mut scaler = context.builder(self.font()).size(size).hint(true).build();
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, glyph)?;
        Some(GlyphBitmap {
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            coverage: image.data,
        })
    }
}
