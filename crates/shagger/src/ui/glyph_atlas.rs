//! One coverage texture for every glyph drawn so far, packed in shelves,
//! with a white corner for filled shapes.

use std::collections::HashMap;

/// Where a glyph is in the atlas and how it sits against the pen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtlasGlyph {
    /// Texels: x, y, width, height.
    pub rect: [u32; 4],
    pub left: i32,
    pub top: i32,
}

/// (font, glyph, size in whole pixels).
pub type GlyphKey = (usize, u16, u32);

pub struct GlyphAtlas {
    pub size: u32,
    /// One byte of coverage per texel.
    pub pixels: Vec<u8>,
    /// Texels changed since the renderer last uploaded.
    pub dirty: bool,
    glyphs: HashMap<GlyphKey, Option<AtlasGlyph>>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_height: u32,
}

/// The white block filled shapes sample (texels 0 to 3 of row 0 and 1).
pub const WHITE_TEXELS: u32 = 4;
const PADDING: u32 = 1;

impl GlyphAtlas {
    #[must_use]
    pub fn new(size: u32) -> Self {
        let mut pixels = vec![0; (size * size) as usize];
        for y in 0..WHITE_TEXELS {
            for x in 0..WHITE_TEXELS {
                pixels[(y * size + x) as usize] = 255;
            }
        }
        Self {
            size,
            pixels,
            dirty: true,
            glyphs: HashMap::new(),
            shelf_x: WHITE_TEXELS + PADDING,
            shelf_y: 0,
            shelf_height: WHITE_TEXELS,
        }
    }

    /// The middle of the white block, in uv.
    #[must_use]
    pub fn white_uv(&self) -> [f32; 2] {
        let c = WHITE_TEXELS as f32 / 2.0 / self.size as f32;
        [c, c]
    }

    /// A glyph already placed (Some(None) for one with no pixels, such as a
    /// space).
    #[must_use]
    pub fn get(&self, key: GlyphKey) -> Option<Option<AtlasGlyph>> {
        self.glyphs.get(&key).copied()
    }

    /// Places a glyph's coverage; `None` once the atlas is full, after
    /// which the glyph is forgotten so a cleared atlas can try again.
    pub fn insert(
        &mut self,
        key: GlyphKey,
        left: i32,
        top: i32,
        width: u32,
        height: u32,
        coverage: &[u8],
    ) -> Option<AtlasGlyph> {
        if width == 0 || height == 0 {
            self.glyphs.insert(key, None);
            return None;
        }
        if self.shelf_x + width + PADDING > self.size {
            self.shelf_y += self.shelf_height + PADDING;
            self.shelf_x = 0;
            self.shelf_height = 0;
        }
        if width + PADDING > self.size || self.shelf_y + height + PADDING > self.size {
            return None;
        }
        let (x, y) = (self.shelf_x, self.shelf_y);
        for row in 0..height {
            let from = (row * width) as usize;
            let to = ((y + row) * self.size + x) as usize;
            self.pixels[to..to + width as usize].copy_from_slice(&coverage[from..from + width as usize]);
        }
        self.shelf_x += width + PADDING;
        self.shelf_height = self.shelf_height.max(height);
        self.dirty = true;
        let glyph = AtlasGlyph {
            rect: [x, y, width, height],
            left,
            top,
        };
        self.glyphs.insert(key, Some(glyph));
        Some(glyph)
    }

    /// Empties the atlas (when it fills: sizes come and go).
    pub fn clear(&mut self) {
        *self = Self::new(self.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_in_shelves_and_fills_up() {
        let mut atlas = GlyphAtlas::new(16);
        let a = atlas.insert((0, 1, 10), 0, 5, 4, 4, &[9; 16]).unwrap();
        assert_eq!(a.rect, [5, 0, 4, 4]);
        let b = atlas.insert((0, 2, 10), 0, 5, 8, 3, &[7; 24]).unwrap();
        assert_eq!(b.rect, [0, 5, 8, 3]);
        assert_eq!(atlas.pixels[(5 * 16) as usize], 7);
        assert_eq!(atlas.get((0, 2, 10)), Some(Some(b)));
        assert!(atlas.insert((0, 3, 10), 0, 0, 8, 12, &[1; 96]).is_none());
        assert_eq!(atlas.insert((0, 4, 10), 0, 0, 0, 0, &[]), None);
        assert_eq!(atlas.get((0, 4, 10)), Some(None));
    }
}
