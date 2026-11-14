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
    /// The glyph of each ASCII character, and every glyph's advance in
    /// font units with the units per em, read once: the HUD lays out its
    /// text every frame, and each lookup through `FontRef` parses the
    /// font's tables again.
    ascii: [u16; 128],
    advances: Vec<f32>,
    units_per_em: f32,
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
        let font = FontRef::from_index(&data, 0).ok_or("not a font")?;
        let (offset, key) = (font.offset, font.key);
        let charmap = font.charmap();
        let ascii = std::array::from_fn(|i| charmap.map(i as u32));
        // Unscaled (a scale of 1): `advance` scales them as swash does.
        let metrics = font.glyph_metrics(&[]);
        let advances = (0..metrics.glyph_count()).map(|g| metrics.advance_width(g)).collect();
        let units_per_em = f32::from(metrics.units_per_em());
        Ok(Self {
            data,
            offset,
            key,
            ascii,
            advances,
            units_per_em,
        })
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
        match self.ascii.get(ch as usize) {
            Some(&glyph) => glyph,
            None => self.font().charmap().map(ch),
        }
    }

    /// How far the pen moves after `glyph` at `size` pixels to the em.
    #[must_use]
    pub fn advance(&self, glyph: u16, size: f32) -> f32 {
        match self.advances.get(usize::from(glyph)) {
            // swash's own sum: the units times `ppem / units_per_em`.
            Some(&units) if self.units_per_em != 0.0 => units * (size / self.units_per_em),
            _ => self.font().glyph_metrics(&[]).scale(size).advance_width(glyph),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables read once give what swash gives each time
    /// (`SHAGGER_TEST_FONT`, a TrueType file).
    #[test]
    fn cached_glyphs_and_advances_match_swash() {
        let Ok(path) = std::env::var("SHAGGER_TEST_FONT") else {
            return;
        };
        let font = Font::from_bytes(std::fs::read(path).unwrap()).unwrap();
        for ch in (0u8..128).map(char::from).chain(['é', 'ä', '€', '·']) {
            let glyph = font.glyph(ch);
            assert_eq!(glyph, font.font().charmap().map(ch), "{ch:?}");
            for size in [9.0, 13.0, 16.0, 21.5, 32.0, 77.0] {
                let swash = font.font().glyph_metrics(&[]).scale(size).advance_width(glyph);
                assert_eq!(font.advance(glyph, size).to_bits(), swash.to_bits(), "{ch:?} at {size}");
            }
        }
    }
}
