//! Deterministic pseudo-random numbers (SplitMix64).
//!
//! The simulation must be reproducible: the same seed and the same sequence of
//! API calls always produce the same world. Using the standard library's
//! `HashMap` hasher or system entropy would break that, so the generator lives
//! here, is seeded explicitly, and is stored inside the world.

/// A small, fast, deterministic generator. Not cryptographically secure.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // The odd constant keeps a seed of 0 from producing a degenerate stream.
        Rng {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        // 53 bits of mantissa is the most an f64 can represent exactly.
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `[low, high)`.
    pub fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next_f64()
    }

    /// Standard normal, via the Box-Muller transform.
    pub fn gauss(&mut self) -> f64 {
        // Clamp away from zero so `ln` stays finite.
        let u1 = self.next_f64().max(f64::MIN_POSITIVE);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn floats_stay_in_range() {
        let mut rng = Rng::new(42);
        for _ in 0..10_000 {
            let value = rng.next_f64();
            assert!((0.0..1.0).contains(&value));
            assert!((2.0..5.0).contains(&rng.range(2.0, 5.0)));
            assert!(rng.gauss().is_finite());
        }
    }
}
