//! Deterministic random numbers (SplitMix64, Vigna 2014): the same seed gives
//! the same wall on every machine.

/// A stream of random numbers.
#[derive(Clone, Debug)]
pub struct Random {
    state: u64,
}

impl Random {
    /// A stream from a seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed ^ 0x2545_f491_4f6c_dd1d }
    }

    /// The next raw number.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number from 0 to 1 (not including 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A number from -1 to 1.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// A number from `low` to `high`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let (mut a, mut b) = (Random::new(7), Random::new(7));
        assert!((0..100).all(|_| a.next_u64() == b.next_u64()));
        assert_ne!(Random::new(7).next_u64(), Random::new(8).next_u64());
    }

    #[test]
    fn units_stay_in_range() {
        let mut random = Random::new(1);
        assert!((0..10_000).map(|_| random.unit()).all(|x| (0.0..1.0).contains(&x)));
    }
}
