//! `Math.random`'s stand-in for what only looks random: the cosmetic
//! spins, shuffles and jitters whose numbers nothing else depends on (the
//! simulation and anything a golden vector checks seed `mulberry32` from
//! `spread_utils` instead). One xorshift64* for them all, in place of a
//! generator written out in each module.

use std::hash::BuildHasher;

/// xorshift64*: numbers in [0, 1) like `Math.random()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathRandom(u64);

impl MathRandom {
    /// A generator seeded with `seed` (made odd: zero would stick at zero).
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// A generator seeded with `text` hashed (FNV-1a), so each name keeps
    /// its own sequence from run to run.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        Self::new(h)
    }

    /// A generator seeded differently each run, as `Math.random` is: the
    /// standard library's per-process random hash keys.
    #[must_use]
    pub fn from_entropy() -> Self {
        Self::new(std::collections::hash_map::RandomState::new().hash_one(0u8))
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// `Math.random()`: in [0, 1), 53 bits of it.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `Math.floor(Math.random() * len)`: an index below `len` (0 for 0).
    pub fn index(&mut self, len: usize) -> usize {
        (self.next_f64() * len as f64) as usize % len.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_stay_in_range_and_repeat_by_seed() {
        let mut a = MathRandom::new(7);
        let mut b = MathRandom::new(7);
        for _ in 0..1000 {
            let x = a.next_f64();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x.to_bits(), b.next_f64().to_bits());
        }
        assert_eq!(MathRandom::new(0), MathRandom::new(1));
        assert_eq!(MathRandom::from_text("item_1"), MathRandom::from_text("item_1"));
        assert_ne!(MathRandom::from_text("item_1"), MathRandom::from_text("item_2"));
        let mut r = MathRandom::from_entropy();
        assert!((0..100).all(|_| r.index(5) < 5));
        assert_eq!(r.index(0), 0);
    }

    /// The sequence the modules it replaced drew (xorshift64* from an odd
    /// seed), so their looks do not change.
    #[test]
    fn matches_the_generators_it_replaced() {
        let mut seed: u64 = 0x2f6b_95d1_c3a7_0e49;
        let mut old = move || {
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            (seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut new = MathRandom::new(0x2f6b_95d1_c3a7_0e49);
        for _ in 0..100 {
            assert_eq!(old().to_bits(), new.next_f64().to_bits());
        }
    }
}
