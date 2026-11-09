//! The zone's deterministic random stream (SplitMix64).
//!
//! The 64-bit state is canonical: checkpoints store it, and recovery resumes
//! the same stream. A zone seeds it from its declared content RNG seed and zone ID,
//! so different zones retain separate streams; content may explicitly retain
//! a seed when an economy-only revision must preserve simulation behavior. Draws
//! happen only at fixed points of the tick order, in unit order.

use crate::ZoneId;

const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// SplitMix64: one state word, a fixed increment and an integer finaliser.
/// Every draw advances the state by exactly one step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZoneRng {
    state: u64,
}

impl ZoneRng {
    /// Resumes a stream from its canonical state.
    #[must_use]
    pub const fn from_state(state: u64) -> Self {
        Self { state }
    }

    /// The initial stream of a zone.
    #[must_use]
    pub const fn seeded(content_seed: u64, zone_id: ZoneId) -> Self {
        Self {
            state: mix(content_seed ^ mix(zone_id.get() as u64)),
        }
    }

    /// The canonical state.
    #[must_use]
    pub const fn state(self) -> u64 {
        self.state
    }

    pub const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        mix(self.state)
    }

    /// Uniform-ish in `0..bound` by multiply-shift on the high 32 bits. There
    /// is no rejection loop, so each draw costs exactly one step; the bias is
    /// below `bound / 2^32`. `bound = 0` yields 0.
    pub const fn below(&mut self, bound: u32) -> u32 {
        (((self.next_u64() >> 32) * bound as u64) >> 32) as u32
    }

    /// Uniform-ish in `low..=high` with one step. Requires `low <= high`.
    pub const fn inclusive(&mut self, low: u32, high: u32) -> u32 {
        let span = (high - low) as u64 + 1;
        low + (((self.next_u64() >> 32) * span) >> 32) as u32
    }
}

/// SplitMix64 finaliser.
const fn mix(value: u64) -> u64 {
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_is_pinned_and_resumable() {
        // Reference SplitMix64 output for state 0.
        let mut rng = ZoneRng::from_state(0);
        assert_eq!(rng.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next_u64(), 0x6e78_9e6a_a1b9_65f4);
        let mut resumed = ZoneRng::from_state(rng.state());
        assert_eq!(resumed.next_u64(), rng.next_u64());
    }

    #[test]
    fn seeds_differ_by_zone_and_content() {
        let base = ZoneRng::seeded(7, ZoneId::new(1));
        assert_eq!(base, ZoneRng::seeded(7, ZoneId::new(1)));
        assert_ne!(base, ZoneRng::seeded(7, ZoneId::new(2)));
        assert_ne!(base, ZoneRng::seeded(8, ZoneId::new(1)));
    }

    #[test]
    fn bounded_draws_cover_their_range_and_never_leave_it() {
        let mut rng = ZoneRng::seeded(42, ZoneId::new(3));
        let mut seen = [0_u32; 6];
        for _ in 0..6_000 {
            seen[rng.below(6) as usize] += 1;
            let value = rng.inclusive(150, 450);
            assert!((150..=450).contains(&value));
        }
        assert!(seen.iter().all(|&count| count > 800), "{seen:?}");
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.inclusive(9, 9), 9);
        let full = rng.inclusive(0, u32::MAX);
        let _ = full;
    }
}
