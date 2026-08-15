//! Choosing who breeds and who survives.
//!
//! Both functions work on a slice of fitness values indexed the same way as
//! the population, and they hand back indices rather than individuals, so
//! nothing here has to know what an individual looks like.
//!
//! Fitness is compared with [`f64::total_cmp`] throughout. An objective
//! that divides by zero somewhere can hand back a NaN, and
//! `partial_cmp().unwrap()` would abort a long run at that point. A total
//! order sorts the NaN to one end and keeps going.

use crate::rng::Rng;

/// True if `a` should be ranked ahead of `b`.
///
/// Ties go to the lower index. Without that the winner of a tie would
/// depend on draw order or sort internals, and identical seeds could
/// produce different runs.
fn ranks_ahead(fitness_a: f64, index_a: usize, fitness_b: f64, index_b: usize) -> bool {
    match fitness_a.total_cmp(&fitness_b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Equal => index_a < index_b,
        std::cmp::Ordering::Less => false,
    }
}

/// Run one tournament and return `(best, second_best)`.
///
/// `pool_size` competitors are drawn uniformly at random with replacement,
/// then the two fittest of them are returned. Sampling with replacement is
/// what keeps the selection pressure independent of population size, and it
/// is cheaper than sampling without.
///
/// Because the draw is with replacement it can land on the same individual
/// every time. Rather than return a parent paired with itself, the pool is
/// topped up with further draws until it holds at least two distinct
/// individuals, so the two returned indices are always different. That
/// terminates as long as there are at least two individuals to draw from.
///
/// # Panics
///
/// If `pool_size` is below 2, or if there are fewer than 2 individuals.
/// Neither can produce two distinct parents, and silently returning one
/// would corrupt the generation instead of stopping.
pub fn tournament(fitness: &[f64], pool_size: usize, rng: &mut Rng) -> (usize, usize) {
    assert!(
        pool_size >= 2,
        "tournament needs a pool of at least 2, got {pool_size}"
    );
    assert!(
        fitness.len() >= 2,
        "tournament needs at least 2 individuals, got {}",
        fitness.len()
    );

    let n = fitness.len() as u64;
    let mut pool: Vec<usize> = (0..pool_size).map(|_| rng.below(n) as usize).collect();
    while pool.iter().all(|&i| i == pool[0]) {
        pool.push(rng.below(n) as usize);
    }

    let mut best = pool[0];
    for &i in &pool[1..] {
        if ranks_ahead(fitness[i], i, fitness[best], best) {
            best = i;
        }
    }

    // The top up above guarantees the pool holds something other than
    // `best`, so this always finds a runner up.
    let mut second = None;
    for &i in &pool {
        if i == best {
            continue;
        }
        match second {
            None => second = Some(i),
            Some(s) if ranks_ahead(fitness[i], i, fitness[s], s) => second = Some(i),
            Some(_) => {}
        }
    }

    (best, second.expect("pool holds two distinct individuals"))
}

/// Indices of the `count` fittest individuals, best first.
///
/// `count` is clamped to the population size, so asking for more elites
/// than exist returns everyone rather than failing. Ties are broken by
/// ascending index so the carried over elites are the same on every run
/// from a given seed.
pub fn elite_indices(fitness: &[f64], count: usize) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..fitness.len()).collect();
    // Comparing the index second makes the order total, so an unstable sort
    // is still deterministic. `total_cmp` keeps a NaN fitness from
    // panicking here.
    indices.sort_unstable_by(|&a, &b| fitness[b].total_cmp(&fitness[a]).then(a.cmp(&b)));
    indices.truncate(count.min(fitness.len()));
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tournament_returns_two_distinct_valid_indices() {
        let fitness = [0.1, 0.9, 0.5, 0.3];
        let mut rng = Rng::new(1);
        for _ in 0..500 {
            let (a, b) = tournament(&fitness, 3, &mut rng);
            assert_ne!(a, b, "tournament returned the same parent twice");
            assert!(a < fitness.len() && b < fitness.len());
        }
    }

    /// With a pool far larger than the population every individual is drawn,
    /// so the answer must be the global best and runner up.
    #[test]
    fn tournament_picks_the_best_when_the_pool_covers_everything() {
        let fitness = [0.1, 0.9, 0.5, 0.3];
        let mut rng = Rng::new(2);
        for _ in 0..50 {
            assert_eq!(tournament(&fitness, 200, &mut rng), (1, 2));
        }
    }

    #[test]
    fn tournament_is_reproducible_from_a_seed() {
        let fitness = [0.4, 0.1, 0.7, 0.2, 0.9, 0.55];
        let mut a = Rng::new(4242);
        let mut b = Rng::new(4242);
        for _ in 0..200 {
            assert_eq!(
                tournament(&fitness, 3, &mut a),
                tournament(&fitness, 3, &mut b)
            );
        }
    }

    /// Every draw can land on the same individual when the fitnesses are
    /// flat, which is exactly when the top up has to fire.
    #[test]
    fn tournament_handles_uniform_fitness() {
        let fitness = [1.0, 1.0];
        let mut rng = Rng::new(7);
        for _ in 0..500 {
            let (a, b) = tournament(&fitness, 2, &mut rng);
            assert_ne!(a, b);
        }
    }

    #[test]
    fn tournament_does_not_panic_on_nan_fitness() {
        let fitness = [0.5, f64::NAN, 0.9, -f64::NAN];
        let mut rng = Rng::new(8);
        for _ in 0..200 {
            let (a, b) = tournament(&fitness, 3, &mut rng);
            assert_ne!(a, b);
        }
    }

    #[test]
    #[should_panic(expected = "pool of at least 2")]
    fn tournament_panics_on_a_pool_below_two() {
        tournament(&[0.1, 0.2], 1, &mut Rng::new(1));
    }

    #[test]
    #[should_panic(expected = "at least 2 individuals")]
    fn tournament_panics_on_a_population_below_two() {
        tournament(&[0.1], 4, &mut Rng::new(1));
    }

    #[test]
    fn elite_indices_returns_count_sorted_descending() {
        let fitness = [0.1, 0.9, 0.5, 0.3];
        assert_eq!(elite_indices(&fitness, 2), vec![1, 2]);
        assert_eq!(elite_indices(&fitness, 4), vec![1, 2, 3, 0]);
        let picked = elite_indices(&fitness, 3);
        assert!(picked.windows(2).all(|w| fitness[w[0]] >= fitness[w[1]]));
    }

    #[test]
    fn elite_indices_clamps_an_oversized_count() {
        let fitness = [0.1, 0.9, 0.5];
        let elites = elite_indices(&fitness, 99);
        assert_eq!(elites.len(), 3);
        let mut seen = elites.clone();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2], "clamping must not duplicate");
    }

    #[test]
    fn elite_indices_zero_count_is_empty() {
        assert!(elite_indices(&[0.1, 0.9], 0).is_empty());
    }

    #[test]
    fn elite_indices_on_an_empty_population_is_empty() {
        assert!(elite_indices(&[], 5).is_empty());
    }

    #[test]
    fn elite_indices_ties_break_by_ascending_index() {
        let fitness = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(elite_indices(&fitness, 4), vec![0, 1, 2, 3]);
        let mixed = [0.5, 2.0, 0.5, 2.0];
        assert_eq!(elite_indices(&mixed, 4), vec![1, 3, 0, 2]);
    }

    /// An objective that divides by zero can hand back a NaN. Ranking must
    /// survive it rather than abort the run.
    #[test]
    fn elite_indices_handles_nan_without_panicking() {
        let fitness = [0.5, f64::NAN, 0.9, f64::INFINITY, -f64::NAN];
        let elites = elite_indices(&fitness, 5);
        assert_eq!(elites.len(), 5);
        let mut seen = elites.clone();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3, 4], "every index must appear once");
        // The real fitnesses still have to be ordered among themselves.
        let real: Vec<usize> = elites
            .into_iter()
            .filter(|&i| !fitness[i].is_nan())
            .collect();
        assert_eq!(real, vec![3, 2, 0]);
    }

    #[test]
    fn elite_indices_is_stable_across_calls() {
        let fitness = [0.3, 0.3, 0.9, 0.1, 0.9];
        assert_eq!(elite_indices(&fitness, 3), elite_indices(&fitness, 3));
        assert_eq!(elite_indices(&fitness, 3), vec![2, 4, 0]);
    }
}
