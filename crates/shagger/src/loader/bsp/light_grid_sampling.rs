//! SHAG `src/engine/loader/bsp/LightGridSampling.ts`: the map's light grid
//! sampled at a point.

use super::bsp_loader::{BspFile, BspLightVol, LightGrid};

/// A light grid sample (`BspLightVol` with interpolated colours).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LightSample {
    pub ambient: [f64; 3],
    pub directional: [f64; 3],
    /// Phi, theta: the nearest site's.
    pub direction: [u8; 2],
}

/// `sampleLightGrid`: trilinear RGB with invalid compressed sites left out
/// and the weights renormalised; `None` outside the grid or where it has no
/// light. The point is in Q3 space (x, y, z up).
#[must_use]
pub fn sample_light_grid(bsp: &BspFile, px: f64, py: f64, pz: f64) -> Option<LightSample> {
    sample_light_vols(bsp.light_grid.as_ref()?, &bsp.light_vols, px, py, pz)
}

/// [`sample_light_grid`] over a map's grid and sites kept apart from the
/// rest of its file.
#[must_use]
pub fn sample_light_vols(
    grid: &LightGrid,
    light_vols: &[BspLightVol],
    px: f64,
    py: f64,
    pz: f64,
) -> Option<LightSample> {
    if light_vols.is_empty() {
        return None;
    }
    let (origin, size, step) = (grid.origin, grid.size, grid.step);
    let gx = (px - origin[0]) / step[0];
    let gy = (py - origin[1]) / step[1];
    let gz = (pz - origin[2]) / step[2];
    if !in_grid(gx, size[0]) || !in_grid(gy, size[1]) || !in_grid(gz, size[2]) {
        return None;
    }
    let cx = gx.min(size[0] - 1.0).max(0.0);
    let cy = gy.min(size[1] - 1.0).max(0.0);
    let cz = gz.min(size[2] - 1.0).max(0.0);
    let (bx, by, bz) = (cx.floor(), cy.floor(), cz.floor());
    let (fx, fy, fz) = (cx - bx, cy - by, cz - bz);
    let mut out = LightSample::default();
    let mut weight = 0.0;
    let mut nearest_weight = -1.0;
    let mut direction = None;
    for z in 0..2u8 {
        for y in 0..2u8 {
            for x in 0..2u8 {
                let w = (if x == 1 { fx } else { 1.0 - fx })
                    * (if y == 1 { fy } else { 1.0 - fy })
                    * (if z == 1 { fz } else { 1.0 - fz });
                if w <= 0.0 {
                    continue;
                }
                let ix = (bx + f64::from(x)).min(size[0] - 1.0);
                let iy = (by + f64::from(y)).min(size[1] - 1.0);
                let iz = (bz + f64::from(z)).min(size[2] - 1.0);
                let index = ix + size[0] * (iy + size[1] * iz);
                // An index past the lump reads `undefined` and is skipped.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let Some(site) = light_vols.get(index as usize).filter(|s| s.valid) else {
                    continue;
                };
                for channel in 0..3 {
                    out.ambient[channel] += f64::from(site.ambient[channel]) * w;
                    out.directional[channel] += f64::from(site.directional[channel]) * w;
                }
                weight += w;
                if w > nearest_weight {
                    nearest_weight = w;
                    direction = Some(site.direction);
                }
            }
        }
    }
    let direction = direction?;
    if weight == 0.0 {
        return None;
    }
    for channel in 0..3 {
        out.ambient[channel] /= weight;
        out.directional[channel] /= weight;
    }
    out.direction = direction;
    Some(out)
}

fn in_grid(value: f64, size: f64) -> bool {
    value.is_finite() && value >= -1.0 && value <= size
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(level: u16) -> BspLightVol {
        BspLightVol {
            valid: true,
            ambient: [level; 3],
            directional: [0; 3],
            direction: [1, 2],
        }
    }

    #[test]
    fn blends_neighbours_and_skips_invalid_sites() {
        // Two sites along x, a step of 64 apart.
        let grid = LightGrid {
            origin: [0.0; 3],
            size: [2.0, 1.0, 1.0],
            step: [64.0, 64.0, 128.0],
        };
        let vols = [site(100), site(200)];
        let mid = sample_light_vols(&grid, &vols, 16.0, 0.0, 0.0).unwrap();
        assert_eq!(mid.ambient, [125.0; 3]);
        assert_eq!(mid.direction, [1, 2]);
        let mut dark = vols;
        dark[1].valid = false;
        assert_eq!(
            sample_light_vols(&grid, &dark, 16.0, 0.0, 0.0).unwrap().ambient,
            [100.0; 3]
        );
        // Out past the grid's edge cell.
        assert_eq!(sample_light_vols(&grid, &vols, 500.0, 0.0, 0.0), None);
    }
}
