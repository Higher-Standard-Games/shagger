//! SHAG `src/shared/SpreadUtils.ts`, so far `mulberry32`, the seeded PRNG
//! the client's effects and pellet spread draw from.

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
}
