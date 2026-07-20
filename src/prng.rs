//! A tiny deterministic PRNG (SplitMix64) — used in exactly one place:
//! breaking ties among equal-best root moves (ADR-0015 §6). No ambient
//! randomness anywhere else in the crate.

/// SplitMix64: tiny, fast, and plenty for tie-breaking.
#[derive(Debug, Clone, Copy)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    /// Seed the generator.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next pseudo-random value.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform index in `0..bound` (`0` when `bound == 0`).
    pub fn next_index(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        usize::try_from(self.next_u64().checked_rem(bound as u64).unwrap_or(0)).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::SplitMix64;

    #[test]
    fn deterministic_under_a_fixed_seed() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..16 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn bounded_index() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..64 {
            assert!(rng.next_index(5) < 5);
        }
        assert_eq!(rng.next_index(0), 0);
    }
}
