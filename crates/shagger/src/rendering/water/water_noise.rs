//! Port of SHAG `src/engine/rendering/water/WaterNoise.ts`: the tileable
//! value-noise normal map every liquid surface scrolls. Generated, not
//! shipped, so it tiles exactly and costs no download.

use shagger_shared::js::js_round;

/// `WATER_NOISE_SIZE`.
pub const WATER_NOISE_SIZE: usize = 128;

/// `latticeValue`: an integer hash of a wrapped lattice point, 0 to 1.
fn lattice_value(x: f64, y: f64, period: f64, seed: i32) -> f64 {
    let xi = ((x % period) + period) % period;
    let yi = ((y % period) + period) % period;
    // `Math.imul` on the integer lattice coordinates.
    let (xi, yi) = (xi as i32, yi as i32);
    let mut h = xi.wrapping_mul(374_761_393) ^ yi.wrapping_mul(668_265_263) ^ seed.wrapping_mul(1_274_126_177);
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    h ^= ((h as u32) >> 16) as i32;
    f64::from(h as u32) / 4_294_967_296.0
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn value_noise(x: f64, y: f64, period: f64, seed: i32) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = smoothstep(x - x0);
    let fy = smoothstep(y - y0);
    let v00 = lattice_value(x0, y0, period, seed);
    let v10 = lattice_value(x0 + 1.0, y0, period, seed);
    let v01 = lattice_value(x0, y0 + 1.0, period, seed);
    let v11 = lattice_value(x0 + 1.0, y0 + 1.0, period, seed);
    let top = v00 + (v10 - v00) * fx;
    let bottom = v01 + (v11 - v01) * fx;
    top + (bottom - top) * fy
}

/// `waterHeightAt`: fractal value noise, 0 to 1, tiling over the unit square.
#[must_use]
pub fn water_height_at(u: f64, v: f64, octaves: u32) -> f64 {
    let mut amplitude = 1.0;
    let mut period = 4.0;
    let mut sum = 0.0;
    let mut total = 0.0;
    for o in 0..octaves {
        sum += value_noise(u * period, v * period, period, o as i32 + 1) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        period *= 2.0;
    }
    sum / total
}

/// `generateWaterNormalMap`: RGBA8, tangent-space normals from the height
/// field's central differences.
#[must_use]
pub fn generate_water_normal_map(size: usize, strength: f64) -> Vec<u8> {
    let mut data = vec![0u8; size * size * 4];
    let step = 1.0 / size as f64;
    for y in 0..size {
        for x in 0..size {
            let u = x as f64 / size as f64;
            let v = y as f64 / size as f64;
            let h_l = water_height_at(u - step, v, 4);
            let h_r = water_height_at(u + step, v, 4);
            let h_d = water_height_at(u, v - step, 4);
            let h_u = water_height_at(u, v + step, 4);
            let mut nx = (h_l - h_r) * strength;
            let mut ny = (h_d - h_u) * strength;
            let nz = 1.0;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            nx /= len;
            ny /= len;
            let nzn = nz / len;
            let i = (y * size + x) * 4;
            data[i] = js_round((nx * 0.5 + 0.5) * 255.0) as u8;
            data[i + 1] = js_round((ny * 0.5 + 0.5) * 255.0) as u8;
            data[i + 2] = js_round((nzn * 0.5 + 0.5) * 255.0) as u8;
            data[i + 3] = 255;
        }
    }
    data
}
