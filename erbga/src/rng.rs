//! A small deterministic PRNG.
//!
//! The benchmark suite is a regression test against published numbers, so
//! the sequence has to be stable across platforms and across releases.
//! `rand`'s `StdRng` explicitly does not promise that, and pulling in
//! `rand` plus `rand_chacha` to get one that does costs more than
//! xorshift64* does. Nothing here is cryptographic and nothing needs to be.

/// xorshift64*, seeded and reproducible.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Zero is a fixed point of xorshift64*: it would emit nothing but
    /// zeros forever. That one bad seed is mapped to a good one rather
    /// than rejected, so callers can pass any `u64`.
    pub fn new(seed: u64) -> Self {
        Rng {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, n)`.
    ///
    /// The short final block is rejected so that low values are not
    /// favored the way a bare `% n` would favor them. `t` is `2^64 % n`,
    /// computed without needing `2^64` itself.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below(0) has no valid output");
        let t = 0u64.wrapping_sub(n) % n;
        loop {
            let v = self.next_u64();
            if v >= t {
                return v % n;
            }
        }
    }

    /// Uniform in `[0.0, 1.0)`.
    ///
    /// 53 bits is an `f64`'s mantissa width. Using all 64 would round and
    /// could hand back exactly `1.0`, which would make `unit() < p` fire
    /// for `p == 1.0` inconsistently.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let differs = (0..100).any(|_| a.next_u64() != b.next_u64());
        assert!(differs, "distinct seeds produced identical output");
    }

    #[test]
    fn zero_seed_does_not_collapse_to_zeros() {
        let mut r = Rng::new(0);
        let any_nonzero = (0..100).any(|_| r.next_u64() != 0);
        assert!(any_nonzero, "zero seed degenerated into an all-zero stream");
    }

    #[test]
    fn below_stays_in_range() {
        let mut r = Rng::new(7);
        for n in 1..=64u64 {
            for _ in 0..200 {
                assert!(r.below(n) < n);
            }
        }
    }

    #[test]
    fn below_is_roughly_uniform() {
        let mut r = Rng::new(99);
        let buckets = 8usize;
        let draws = 80_000;
        let mut counts = vec![0usize; buckets];
        for _ in 0..draws {
            counts[r.below(buckets as u64) as usize] += 1;
        }
        let expected = draws as f64 / buckets as f64;
        for (i, &c) in counts.iter().enumerate() {
            let deviation = (c as f64 - expected).abs() / expected;
            assert!(
                deviation < 0.05,
                "bucket {i} deviated {deviation:.3} from uniform (count {c}, expected {expected})"
            );
        }
    }

    #[test]
    fn unit_stays_in_half_open_range() {
        let mut r = Rng::new(4242);
        for _ in 0..100_000 {
            let u = r.unit();
            assert!((0.0..1.0).contains(&u), "unit() produced {u}");
        }
    }

    #[test]
    #[should_panic(expected = "Rng::below(0)")]
    fn below_zero_panics() {
        Rng::new(1).below(0);
    }
}
