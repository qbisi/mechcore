//! Build-2259 `GRRandom` type 1: `RanState` xoshiro256** plus its integer projection.

#[derive(Debug, Clone, Copy)]
pub(crate) struct GrRandom {
    state: [u64; 4],
}

impl GrRandom {
    pub(crate) fn new(seed: u64) -> Self {
        let mut state = Self {
            state: [seed, 255, 0, 0],
        };
        for _ in 0..16 {
            state.next_u64();
        }
        state
    }

    fn next_u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = self.state;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t02 = s2 ^ s0;
        let t31 = s3 ^ s1;
        self.state[0] = s0 ^ t31;
        self.state[1] = t02 ^ s1;
        self.state[2] = t02 ^ (s1 << 17);
        self.state[3] = t31.rotate_left(45);
        result
    }

    fn next_inclusive(&mut self, low: i64, high: i64) -> i64 {
        debug_assert!(low <= high);
        let range = high.wrapping_sub(low).cast_unsigned();
        let mask = range.next_power_of_two().wrapping_sub(1);
        loop {
            let projected = self.next_u64() & mask;
            if projected <= range {
                return low.wrapping_add(projected.cast_signed());
            }
        }
    }

    pub(crate) fn next_between_inclusive(&mut self, low: i32, high: i32) -> i32 {
        i32::try_from(self.next_inclusive(i64::from(low), i64::from(high)))
            .expect("the sample remains inside the i32 input range")
    }

    pub(crate) fn next_in_range(&mut self, range: i32) -> i32 {
        debug_assert!(range > 0);
        i32::try_from(self.next_inclusive(i64::from(1 - range), i64::from(range - 1)))
            .expect("the sample remains inside the i32 input range")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_native_ran_state_attack_stream() {
        let mut random = GrRandom::new(4_444);
        let actual = (0..12).map(|_| random.next_in_range(6)).collect::<Vec<_>>();
        assert_eq!(actual, [5, -1, 5, 4, 0, -2, -4, 1, 0, 3, 4, -4]);
    }

    /// The stream a unit's first attack interval is staggered by, read back
    /// out of the game.
    ///
    /// Three Marksmen in one round-one fight stored intervals of 55, 65 and 56
    /// ticks against a description of 62, and their `interval_offset` of
    /// `0.6` seconds is twelve ticks. Those three numbers are this stream's
    /// first three draws in that range, in spawn order, which is what says the
    /// stagger comes from the team stream seeded `(round + team) * 4444` and
    /// not from the match seed — the same layout answers the same numbers
    /// under any seed. `docs/rules/combat.md` carries the measurement.
    #[test]
    fn the_first_interval_stagger_is_this_stream() {
        // Round one, team zero: `(round + teamIndex) * 4444`.
        let mut random = GrRandom::new(4_444);
        let drawn = (0..3).map(|_| random.next_in_range(12)).collect::<Vec<_>>();
        assert_eq!(drawn, [-7, 3, -6]);
        let stored = drawn.iter().map(|draw| 62 + draw).collect::<Vec<_>>();
        assert_eq!(stored, [55, 65, 56], "what the game stored for each");
    }
}
