//! SHAG `src/shared/SpreadUtils.ts`: `mulberry32`, the seeded PRNG the
//! client's effects and pellet spread draw from, and the pellet cone the
//! server and client build from one seed.
//!
//! The cone uses `Math.tan`, `Math.sqrt` and `Math.atan2`/`Math.asin` (for
//! the look angles); only `sin` and `cos` go through V8's fdlibm here, so a
//! rebuilt pellet can differ from the server's in the last bit. The client
//! only draws them; the server's own pellets judge the hits.

/// `mulberry32(seed)`: a function yielding deterministic floats in [0, 1),
/// bit for bit as V8 computes them.
pub fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s.wrapping_add(0x6D2B_79F5);
        let t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        let t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// `computeSpreadSeed(tick, entityId, shotCount)`.
#[must_use]
pub fn compute_spread_seed(tick: i32, entity_id: i32, shot_count: i32) -> i32 {
    tick.wrapping_mul(1013)
        .wrapping_add(entity_id.wrapping_mul(7))
        .wrapping_add(shot_count.wrapping_mul(31))
}

/// `generateSpreadDirections(seed, count, spreadDeg, yaw, pitch)`: `count`
/// unit directions in the cone of half-angle `spread_deg` around the look
/// (pitch in the Babylon convention, negative looking up).
#[must_use]
pub fn generate_spread_directions(seed: i32, count: usize, spread_deg: f64, yaw: f64, pitch: f64) -> Vec<[f64; 3]> {
    use crate::js::{js_cos, js_sin};
    let mut rng = mulberry32(seed as u32);
    let spread_rad = (spread_deg * std::f64::consts::PI) / 180.0;
    let p = -pitch;
    let forward = [js_sin(yaw) * js_cos(p), js_sin(p), js_cos(yaw) * js_cos(p)];
    let mut right = if forward[1].abs() < 0.999 {
        [forward[2], 0.0, -forward[0]]
    } else {
        [0.0, -forward[2], forward[1]]
    };
    let r_len = (right[0] * right[0] + right[1] * right[1] + right[2] * right[2]).sqrt();
    for c in &mut right {
        *c /= r_len;
    }
    let up = [
        forward[1] * right[2] - forward[2] * right[1],
        forward[2] * right[0] - forward[0] * right[2],
        forward[0] * right[1] - forward[1] * right[0],
    ];
    (0..count)
        .map(|_| {
            let r = rng().sqrt() * spread_rad.tan();
            let theta = rng() * std::f64::consts::PI * 2.0;
            let offset_x = r * js_cos(theta);
            let offset_y = r * js_sin(theta);
            let d: [f64; 3] = std::array::from_fn(|i| forward[i] + right[i] * offset_x + up[i] * offset_y);
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            [d[0] / len, d[1] / len, d[2] / len]
        })
        .collect()
}

/// `yawPitchFromDirection(dir)`: the look angles of a unit direction, pitch
/// in the Babylon convention.
#[must_use]
pub fn yaw_pitch_from_direction(dir: [f64; 3]) -> (f64, f64) {
    let y = dir[1].clamp(-1.0, 1.0);
    (dir[0].atan2(dir[2]), -y.asin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_v8() {
        // node -e 'const r = mulberry32(1); console.log(r(), r(), r())'
        let mut r = mulberry32(1);
        assert_eq!(r(), 0.627_073_940_588_161_3);
        assert_eq!(r(), 0.002_735_721_180_215_478);
        assert_eq!(r(), 0.527_447_039_959_952_2);
    }

    #[test]
    fn pellets_match_v8() {
        // node: generateSpreadDirections(12345, 3, 8, 0.7, -0.2)
        let expected = [
            [0.572_043_310_421_593, 0.323_315_836_705_872_2, 0.753_812_523_600_587_9],
            [
                0.670_537_156_673_003_7,
                0.110_894_139_579_796_67,
                0.733_541_008_620_336_3,
            ],
            [
                0.573_887_191_861_873_8,
                0.277_718_272_747_256_25,
                0.770_406_420_014_250_1,
            ],
        ];
        let got = generate_spread_directions(12345, 3, 8.0, 0.7, -0.2);
        for (g, e) in got.iter().zip(expected) {
            for i in 0..3 {
                assert!((g[i] - e[i]).abs() < 1e-15, "{g:?} {e:?}");
            }
        }
        let (yaw, pitch) = yaw_pitch_from_direction([0.0, 0.5f64.sin(), 0.5f64.cos()]);
        assert!(yaw.abs() < 1e-15 && (pitch + 0.5).abs() < 1e-15);
        assert_eq!(compute_spread_seed(100, 3, 2), 100 * 1013 + 21 + 62);
    }
}
