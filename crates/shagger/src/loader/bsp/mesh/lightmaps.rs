//! SHAG `src/engine/loader/bsp/mesh/lightmaps.ts`: the bytes of lightmap
//! textures. Creating the GPU textures is the renderer's job.

use super::coordinates::q3_to_babylon;

/// `scaleBspVertexColors`: qfusion scales baked vertex RGB by `gain` and
/// divides by the brightest channel when it passes 1, keeping the hue; alpha
/// is left alone.
#[must_use]
pub fn scale_bsp_vertex_colors(colors: &[f64], gain: f64) -> Vec<f64> {
    colors
        .iter()
        .enumerate()
        .map(|(i, &value)| {
            if i % 4 == 3 {
                return value;
            }
            let start = i - i % 4;
            // Math.max with NaN gives NaN; f64::max would drop it.
            let peak = [colors[start] * gain, colors[start + 1] * gain, colors[start + 2] * gain]
                .into_iter()
                .fold(
                    1.0_f64,
                    |m, x| if m.is_nan() || x.is_nan() { f64::NAN } else { m.max(x) },
                );
            (value * gain) / peak
        })
        .collect()
}

/// The IBSP transfer curve for one byte: `min(255, (c / 255) ** 0.7 * 255 * 2)`,
/// truncated as a `Uint8Array` store truncates.
#[must_use]
pub fn ibsp_lightmap_byte(c: u8) -> u8 {
    let v = (f64::from(c) / 255.0).powf(0.7) * 255.0 * 2.0;
    v.min(255.0) as u8
}

/// `writeBspLightmapRgba`: one `size`² page of RGB into RGBA at `offset`.
/// FBSP pages (`raw`) keep their bytes; IBSP pages go through
/// [`ibsp_lightmap_byte`]. Deluxe pages (`direction`) hold a direction, which
/// is swizzled into Babylon space and keeps alpha 255.
///
/// # Panics
/// When `rgb` or `rgba` is too short for the page.
pub fn write_bsp_lightmap_rgba(rgb: &[u8], size: usize, raw: bool, direction: bool, rgba: &mut [u8], offset: usize) {
    for i in 0..size * size {
        let o = offset + i * 4;
        let (r, g, b) = (rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]);
        if direction {
            let [x, y, z] = q3_to_babylon(r, g, b);
            rgba[o..o + 4].copy_from_slice(&[x, y, z, 255]);
        } else if raw {
            rgba[o..o + 4].copy_from_slice(&[r, g, b, 255]);
        } else {
            rgba[o..o + 4].copy_from_slice(&[ibsp_lightmap_byte(r), ibsp_lightmap_byte(g), ibsp_lightmap_byte(b), 255]);
        }
    }
}

/// The RGBA of one page (`createBspLightmapTexture`'s data).
#[must_use]
pub fn bsp_lightmap_rgba(rgb: &[u8], size: usize, raw: bool, direction: bool) -> Vec<u8> {
    let mut rgba = vec![0; size * size * 4];
    write_bsp_lightmap_rgba(rgb, size, raw, direction, &mut rgba, 0);
    rgba
}

/// Several pages as array layers, in order (`createBspLightmapArray`'s data).
#[must_use]
pub fn bsp_lightmap_array_rgba(pages: &[&[u8]], size: usize, raw: bool, direction: bool) -> Vec<u8> {
    let layer = size * size * 4;
    let mut rgba = vec![0; layer * pages.len()];
    for (i, rgb) in pages.iter().enumerate() {
        write_bsp_lightmap_rgba(rgb, size, raw, direction, &mut rgba, i * layer);
    }
    rgba
}
