//! A chromosome: one bit per edge, 1 meaning the edge is removed.
//!
//! Bits are packed into `u64` words. That is not premature optimization,
//! it is the efficiency contribution of the source work under test: Rust's
//! `Vec<bool>` costs one byte per element, so an unpacked representation
//! would make the published memory result impossible to reproduce, and it
//! would stream eight times the bytes through the hottest loop in the
//! algorithm.
//!
//! The type is deliberately opaque. Operators go through `get`, `set`,
//! `flip`, and `count_ones` and never see the backing store, so switching
//! to the source work's contiguous population layout later is a change to
//! this file alone rather than to every operator and test.

use crate::rng::Rng;

const BITS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chromosome {
    words: Vec<u64>,
    len: usize,
}

impl Chromosome {
    /// All edges present, nothing removed.
    pub fn zeros(len: usize) -> Self {
        Chromosome {
            words: vec![0; len.div_ceil(BITS)],
            len,
        }
    }

    /// Each bit set to 1 (edge removed) with probability `one_rate`.
    pub fn random(len: usize, one_rate: f64, rng: &mut Rng) -> Self {
        let mut c = Chromosome::zeros(len);
        for i in 0..len {
            if rng.unit() < one_rate {
                c.set(i, true);
            }
        }
        c
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(&self, i: usize) -> bool {
        assert!(
            i < self.len,
            "index {i} out of range for length {}",
            self.len
        );
        (self.words[i / BITS] >> (i % BITS)) & 1 == 1
    }

    #[inline]
    pub fn set(&mut self, i: usize, value: bool) {
        assert!(
            i < self.len,
            "index {i} out of range for length {}",
            self.len
        );
        let (word, bit) = (i / BITS, i % BITS);
        if value {
            self.words[word] |= 1u64 << bit;
        } else {
            self.words[word] &= !(1u64 << bit);
        }
    }

    #[inline]
    pub fn flip(&mut self, i: usize) {
        assert!(
            i < self.len,
            "index {i} out of range for length {}",
            self.len
        );
        self.words[i / BITS] ^= 1u64 << (i % BITS);
    }

    /// Number of removed edges.
    ///
    /// Correct only because every mutator refuses indices at or past
    /// `len`, so the padding bits in the final word are always zero.
    pub fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeros_is_all_clear() {
        let c = Chromosome::zeros(200);
        assert_eq!(c.len(), 200);
        assert_eq!(c.count_ones(), 0);
        assert!((0..200).all(|i| !c.get(i)));
    }

    #[test]
    fn set_get_flip_round_trip() {
        let mut c = Chromosome::zeros(130);
        c.set(0, true);
        c.set(64, true);
        c.set(129, true);
        assert!(c.get(0) && c.get(64) && c.get(129));
        assert_eq!(c.count_ones(), 3);

        c.flip(64);
        assert!(!c.get(64));
        assert_eq!(c.count_ones(), 2);

        c.set(0, false);
        assert_eq!(c.count_ones(), 1);
    }

    #[test]
    fn word_boundaries_are_independent() {
        let mut c = Chromosome::zeros(128);
        c.set(63, true);
        assert!(c.get(63));
        assert!(!c.get(64));
        c.set(64, true);
        assert!(c.get(63) && c.get(64));
        assert_eq!(c.count_ones(), 2);
    }

    /// The padding bits past `len` in the final word must stay zero, or
    /// `count_ones` silently over-reports.
    #[test]
    fn padding_bits_never_counted() {
        for len in [1usize, 7, 63, 64, 65, 127, 128, 129] {
            let mut c = Chromosome::zeros(len);
            for i in 0..len {
                c.set(i, true);
            }
            assert_eq!(c.count_ones(), len, "length {len} miscounted");
        }
    }

    #[test]
    fn random_respects_rate_bounds() {
        let mut rng = Rng::new(1);
        let none = Chromosome::random(500, 0.0, &mut rng);
        assert_eq!(none.count_ones(), 0);
        let all = Chromosome::random(500, 1.0, &mut rng);
        assert_eq!(all.count_ones(), 500);
    }

    #[test]
    fn random_is_reproducible_from_a_seed() {
        let a = Chromosome::random(300, 0.4, &mut Rng::new(77));
        let b = Chromosome::random(300, 0.4, &mut Rng::new(77));
        assert_eq!(a, b);
    }

    #[test]
    fn random_rate_is_approximately_honored() {
        let mut rng = Rng::new(5);
        let c = Chromosome::random(20_000, 0.25, &mut rng);
        let observed = c.count_ones() as f64 / 20_000.0;
        assert!((observed - 0.25).abs() < 0.02, "observed rate {observed}");
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn get_past_end_panics() {
        Chromosome::zeros(10).get(10);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn set_past_end_panics() {
        Chromosome::zeros(10).set(10, true);
    }
}
