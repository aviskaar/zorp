//! The variation operators: crossover, mutation, and Gene Repair.
//!
//! All three work on the removed-edge encoding, so a gene is one edge and
//! setting it means cutting that edge out of the graph.

use crate::chromosome::Chromosome;
use crate::graph::Graph;
use crate::rng::Rng;

/// Choose the indices at which two parents will exchange a single gene.
///
/// Each index is included independently with probability `point_rate`, so
/// the number of points is binomial rather than fixed. Results come back
/// in ascending order with no duplicates.
///
/// Why not single point or two point crossover: the genes here are edges,
/// and the vertices they join are not linearly ordered. Whether edge 7 sits
/// next to edge 8 in the bit string is an artifact of how the edge list
/// happened to be sorted, not a statement that the two are related. A
/// contiguous crossover segment would therefore inherit a block of genes
/// that share nothing but their sort position, imposing a linkage structure
/// the problem does not have. Exchanging single genes at scattered points
/// makes no such claim.
pub fn crossover_points(len: usize, point_rate: f64, rng: &mut Rng) -> Vec<usize> {
    let mut points = Vec::new();
    for i in 0..len {
        if rng.unit() < point_rate {
            points.push(i);
        }
    }
    points
}

/// Exchange single genes between two parents at exactly the listed indices.
///
/// The children start as copies of the parents, so every index not listed
/// is inherited untouched. At each listed index the two genes trade places,
/// which makes the children complementary there.
///
/// `points` does not have to be sorted, and it may repeat an index. Each
/// entry performs one swap, so an index listed twice swaps twice and lands
/// back where it started. That is a no-op rather than an error, because the
/// caller usually passes the output of [`crossover_points`], and a hand
/// built list with an accidental repeat should degrade quietly instead of
/// stopping a run.
///
/// # Panics
///
/// If the parents differ in length, or if a point is out of range.
pub fn uniform_crossover(
    a: &Chromosome,
    b: &Chromosome,
    points: &[usize],
) -> (Chromosome, Chromosome) {
    assert_eq!(
        a.len(),
        b.len(),
        "crossover parents must have the same length, got {} and {}",
        a.len(),
        b.len()
    );
    let mut child_a = a.clone();
    let mut child_b = b.clone();
    for &p in points {
        let gene_a = child_a.get(p);
        let gene_b = child_b.get(p);
        child_a.set(p, gene_b);
        child_b.set(p, gene_a);
    }
    (child_a, child_b)
}

/// Flip each gene independently with probability `rate`.
///
/// Per bit, not per chromosome. The source work uses a mutation rate of
/// 0.1, which on a graph with thousands of edges means hundreds of edges
/// change state per application, not one.
pub fn mutate(c: &mut Chromosome, rate: f64, rng: &mut Rng) {
    for i in 0..c.len() {
        if rng.unit() < rate {
            c.flip(i);
        }
    }
}

/// The vertices Gene Repair will work on, highest degree first.
///
/// Precomputed once because the ranking depends only on the graph, and
/// resorting every vertex on every repair call would dominate the cost of
/// the operator itself.
#[derive(Clone, Debug)]
pub struct RepairTargets {
    vertices: Vec<usize>,
}

impl RepairTargets {
    /// Take the `size` highest degree vertices, descending by degree.
    ///
    /// `size` is clamped to the vertex count, so asking for more targets
    /// than the graph has vertices returns all of them rather than failing.
    /// Equal degrees are ordered by ascending vertex index. That tie break
    /// is not cosmetic: without it the target set on a regular graph would
    /// depend on sort internals, and two runs from the same seed could
    /// repair different edges.
    pub fn new(graph: &Graph, size: usize) -> Self {
        let n = graph.vertex_count();
        let mut vertices: Vec<usize> = (0..n).collect();
        // Comparing the index second makes the order total, so an unstable
        // sort is still deterministic.
        vertices.sort_unstable_by(|&x, &y| graph.degree(y).cmp(&graph.degree(x)).then(x.cmp(&y)));
        vertices.truncate(size.min(n));
        RepairTargets { vertices }
    }

    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }
}

/// Restore removed edges around high degree vertices, each with
/// probability `chance`.
///
/// This is the repair half of the source work's contribution. Crossover and
/// mutation cut edges without looking at how dense the neighborhood is, and
/// on some networks that left the whole graph as a single cluster even
/// after heavy removal, because the cuts landed inside dense regions where
/// the endpoints stayed connected by other paths. A high degree vertex is
/// expected to have far more edges inside its own community than out of it,
/// so a removed edge touching one is more likely to be cutting through a
/// real community than between two. Putting some of those back biases the
/// search toward cutting the sparse boundaries instead.
///
/// The operator only ever restores. It never removes an edge, so it cannot
/// undo progress that crossover and mutation made, and `count_ones` never
/// increases across a call. An edge shared by two target vertices is
/// considered twice, but the second look finds it already present.
pub fn gene_repair(
    c: &mut Chromosome,
    graph: &Graph,
    targets: &RepairTargets,
    chance: f64,
    rng: &mut Rng,
) {
    for &v in targets.vertices() {
        for &edge in graph.incident(v) {
            if c.get(edge) && rng.unit() < chance {
                c.set(edge, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring plus a few chords. Every vertex has degree at least two, and
    /// the construction cannot produce a self loop or a repeated edge, so
    /// it stays inside whatever validation `Graph::new` applies.
    fn random_graph(n: usize, extra: usize, rng: &mut Rng) -> Graph {
        assert!(n >= 3);
        let mut edges: Vec<(usize, usize)> = (0..n).map(|i| (i, (i + 1) % n)).collect();
        for _ in 0..extra {
            let u = rng.below(n as u64) as usize;
            let v = rng.below(n as u64) as usize;
            if u == v {
                continue;
            }
            let key = (u.min(v), u.max(v));
            if edges.iter().any(|&(a, b)| (a.min(b), a.max(b)) == key) {
                continue;
            }
            edges.push(key);
        }
        Graph::new(n, &edges).expect("a ring plus chords is a valid simple graph")
    }

    #[test]
    fn crossover_points_rate_zero_is_empty() {
        let mut rng = Rng::new(1);
        assert!(crossover_points(500, 0.0, &mut rng).is_empty());
    }

    #[test]
    fn crossover_points_rate_one_is_every_index() {
        let mut rng = Rng::new(2);
        let points = crossover_points(64, 1.0, &mut rng);
        assert_eq!(points, (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn crossover_points_is_reproducible_from_a_seed() {
        let a = crossover_points(400, 0.3, &mut Rng::new(99));
        let b = crossover_points(400, 0.3, &mut Rng::new(99));
        assert_eq!(a, b);
    }

    #[test]
    fn crossover_points_are_ascending_and_unique() {
        let points = crossover_points(300, 0.5, &mut Rng::new(11));
        assert!(points.windows(2).all(|w| w[0] < w[1]));
        assert!(points.iter().all(|&p| p < 300));
    }

    #[test]
    fn crossover_points_rate_is_approximately_honored() {
        let points = crossover_points(20_000, 0.2, &mut Rng::new(13));
        let observed = points.len() as f64 / 20_000.0;
        assert!((observed - 0.2).abs() < 0.02, "observed rate {observed}");
    }

    #[test]
    fn crossover_points_on_an_empty_chromosome_is_empty() {
        assert!(crossover_points(0, 1.0, &mut Rng::new(1)).is_empty());
    }

    #[test]
    fn uniform_crossover_preserves_length() {
        let a = Chromosome::random(137, 0.5, &mut Rng::new(3));
        let b = Chromosome::random(137, 0.5, &mut Rng::new(4));
        let points = crossover_points(137, 0.3, &mut Rng::new(5));
        let (x, y) = uniform_crossover(&a, &b, &points);
        assert_eq!(x.len(), 137);
        assert_eq!(y.len(), 137);
    }

    #[test]
    fn uniform_crossover_at_every_point_exchanges_the_parents() {
        let a = Chromosome::random(200, 0.5, &mut Rng::new(6));
        let b = Chromosome::random(200, 0.5, &mut Rng::new(7));
        let all: Vec<usize> = (0..200).collect();
        let (x, y) = uniform_crossover(&a, &b, &all);
        assert_eq!(x, b);
        assert_eq!(y, a);
    }

    #[test]
    fn uniform_crossover_with_no_points_returns_clones() {
        let a = Chromosome::random(200, 0.5, &mut Rng::new(8));
        let b = Chromosome::random(200, 0.5, &mut Rng::new(9));
        let (x, y) = uniform_crossover(&a, &b, &[]);
        assert_eq!(x, a);
        assert_eq!(y, b);
    }

    #[test]
    fn uniform_crossover_swaps_only_at_the_listed_indices() {
        let a = Chromosome::random(150, 0.5, &mut Rng::new(10));
        let b = Chromosome::random(150, 0.5, &mut Rng::new(12));
        let points = crossover_points(150, 0.4, &mut Rng::new(14));
        let (x, y) = uniform_crossover(&a, &b, &points);
        for i in 0..150 {
            if points.contains(&i) {
                assert_eq!(x.get(i), b.get(i), "child a should hold parent b at {i}");
                assert_eq!(y.get(i), a.get(i), "child b should hold parent a at {i}");
            } else {
                assert_eq!(x.get(i), a.get(i), "child a drifted at untouched {i}");
                assert_eq!(y.get(i), b.get(i), "child b drifted at untouched {i}");
            }
        }
    }

    /// Where the parents disagreed, the children must disagree too. That is
    /// what makes this an exchange rather than a copy of one parent.
    #[test]
    fn uniform_crossover_children_are_complementary_where_parents_differed() {
        let a = Chromosome::random(150, 0.5, &mut Rng::new(15));
        let b = Chromosome::random(150, 0.5, &mut Rng::new(16));
        let points = crossover_points(150, 0.4, &mut Rng::new(17));
        let (x, y) = uniform_crossover(&a, &b, &points);
        for &i in &points {
            if a.get(i) != b.get(i) {
                assert_ne!(x.get(i), y.get(i), "children agree at swapped index {i}");
            }
        }
    }

    #[test]
    fn uniform_crossover_duplicate_point_swaps_twice_and_cancels() {
        let a = Chromosome::random(64, 0.5, &mut Rng::new(18));
        let b = Chromosome::random(64, 0.5, &mut Rng::new(19));
        let (x, y) = uniform_crossover(&a, &b, &[7, 7]);
        assert_eq!(x, a);
        assert_eq!(y, b);
    }

    #[test]
    fn uniform_crossover_ignores_point_order() {
        let a = Chromosome::random(64, 0.5, &mut Rng::new(20));
        let b = Chromosome::random(64, 0.5, &mut Rng::new(21));
        let (x1, y1) = uniform_crossover(&a, &b, &[3, 17, 40]);
        let (x2, y2) = uniform_crossover(&a, &b, &[40, 3, 17]);
        assert_eq!(x1, x2);
        assert_eq!(y1, y2);
    }

    #[test]
    #[should_panic(expected = "same length")]
    fn uniform_crossover_mismatched_lengths_panic() {
        let a = Chromosome::zeros(10);
        let b = Chromosome::zeros(11);
        uniform_crossover(&a, &b, &[]);
    }

    #[test]
    fn mutate_rate_zero_changes_nothing() {
        let mut c = Chromosome::random(300, 0.4, &mut Rng::new(22));
        let before = c.clone();
        mutate(&mut c, 0.0, &mut Rng::new(23));
        assert_eq!(c, before);
    }

    #[test]
    fn mutate_rate_one_flips_every_bit() {
        let mut c = Chromosome::random(200, 0.4, &mut Rng::new(24));
        let before = c.clone();
        mutate(&mut c, 1.0, &mut Rng::new(25));
        assert!((0..200).all(|i| c.get(i) != before.get(i)));
    }

    #[test]
    fn mutate_is_reproducible_from_a_seed() {
        let base = Chromosome::random(300, 0.3, &mut Rng::new(26));
        let mut a = base.clone();
        let mut b = base.clone();
        mutate(&mut a, 0.1, &mut Rng::new(27));
        mutate(&mut b, 0.1, &mut Rng::new(27));
        assert_eq!(a, b);
    }

    /// The source work's `Mrate` of 0.1 is per bit, so a long chromosome
    /// should see close to a tenth of its genes flip.
    #[test]
    fn mutate_flip_fraction_matches_the_rate() {
        let len = 20_000;
        let before = Chromosome::random(len, 0.5, &mut Rng::new(28));
        let mut after = before.clone();
        mutate(&mut after, 0.1, &mut Rng::new(29));
        let flipped = (0..len).filter(|&i| before.get(i) != after.get(i)).count();
        let observed = flipped as f64 / len as f64;
        assert!((observed - 0.1).abs() < 0.01, "observed rate {observed}");
    }

    #[test]
    fn repair_targets_pick_the_highest_degree_vertices() {
        // A path 0-1-2-3-4 with vertex 5 hung off vertex 2, so the degrees
        // are 1, 2, 3, 2, 1, 1.
        let graph = Graph::new(6, &[(0, 1), (1, 2), (2, 3), (3, 4), (2, 5)]).expect("valid graph");
        let targets = RepairTargets::new(&graph, 3);
        assert_eq!(targets.vertices()[0], 2, "vertex 2 has the highest degree");
        let degrees: Vec<usize> = targets
            .vertices()
            .iter()
            .map(|&v| graph.degree(v))
            .collect();
        assert_eq!(degrees, vec![3, 2, 2]);
        assert!(
            degrees.windows(2).all(|w| w[0] >= w[1]),
            "targets must be descending by degree"
        );
    }

    #[test]
    fn repair_targets_clamps_an_oversized_size() {
        let graph = Graph::new(4, &[(0, 1), (1, 2), (2, 3)]).expect("valid graph");
        let targets = RepairTargets::new(&graph, 1000);
        assert_eq!(targets.vertices().len(), 4);
        let mut seen = targets.vertices().to_vec();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3], "clamping must not duplicate");
    }

    #[test]
    fn repair_targets_zero_size_is_empty() {
        let graph = Graph::new(3, &[(0, 1), (1, 2)]).expect("valid graph");
        assert!(RepairTargets::new(&graph, 0).vertices().is_empty());
    }

    /// On a ring every vertex has degree two, so only the tie break decides
    /// the order. It must be ascending index, and it must be the same on
    /// every construction.
    #[test]
    fn repair_targets_break_ties_by_ascending_index() {
        let edges: Vec<(usize, usize)> = (0..8).map(|i| (i, (i + 1) % 8)).collect();
        let graph = Graph::new(8, &edges).expect("valid graph");
        assert_eq!(RepairTargets::new(&graph, 4).vertices(), &[0, 1, 2, 3]);
        assert_eq!(
            RepairTargets::new(&graph, 8).vertices(),
            RepairTargets::new(&graph, 8).vertices()
        );
    }

    /// The invariant the whole operator rests on. Repair may only put edges
    /// back, so no gene may go from present to removed, whatever the graph,
    /// the chromosome, or the chance.
    #[test]
    fn gene_repair_never_removes_an_edge() {
        let mut rng = Rng::new(31);
        for trial in 0..200 {
            let graph = random_graph(12, 10, &mut rng);
            let targets = RepairTargets::new(&graph, 1 + rng.below(12) as usize);
            let one_rate = rng.unit();
            let before = Chromosome::random(graph.edge_count(), one_rate, &mut rng);
            let chance = rng.unit();
            let mut after = before.clone();
            gene_repair(&mut after, &graph, &targets, chance, &mut rng);
            assert!(
                after.count_ones() <= before.count_ones(),
                "trial {trial} increased the removed count"
            );
            for i in 0..before.len() {
                let newly_removed = after.get(i) && !before.get(i);
                assert!(
                    !newly_removed,
                    "trial {trial} removed edge {i}, which was present"
                );
            }
        }
    }

    #[test]
    fn gene_repair_full_chance_restores_every_incident_removed_edge() {
        let mut rng = Rng::new(32);
        let graph = random_graph(20, 25, &mut rng);
        let targets = RepairTargets::new(&graph, 5);
        let mut c = Chromosome::random(graph.edge_count(), 0.7, &mut rng);
        gene_repair(&mut c, &graph, &targets, 1.0, &mut rng);
        for &v in targets.vertices() {
            for &edge in graph.incident(v) {
                assert!(!c.get(edge), "edge {edge} at target {v} was left removed");
            }
        }
    }

    #[test]
    fn gene_repair_zero_chance_changes_nothing() {
        let graph = Graph::new(3, &[(0, 1), (1, 2), (0, 2)]).expect("valid graph");
        let targets = RepairTargets::new(&graph, 3);
        let mut c = Chromosome::random(graph.edge_count(), 0.5, &mut Rng::new(33));
        let before = c.clone();
        gene_repair(&mut c, &graph, &targets, 0.0, &mut Rng::new(34));
        assert_eq!(c, before);
    }

    /// Edges that touch no target vertex are outside the operator's reach,
    /// however high the chance.
    #[test]
    fn gene_repair_leaves_untargeted_edges_alone() {
        // Vertex 0 is the hub, so it is the only target. The edge joining 4
        // and 5 never touches it. `Graph::new` canonicalizes and re-sorts
        // the edge list, so the index is looked up rather than assumed.
        let graph = Graph::new(6, &[(0, 1), (0, 2), (0, 3), (4, 5)]).expect("valid graph");
        let targets = RepairTargets::new(&graph, 1);
        assert_eq!(targets.vertices(), &[0]);
        let far = graph
            .edges()
            .iter()
            .position(|&(a, b)| (a.min(b), a.max(b)) == (4, 5))
            .expect("the 4-5 edge is in the list");
        let mut c = Chromosome::zeros(graph.edge_count());
        for i in 0..graph.edge_count() {
            c.set(i, true);
        }
        gene_repair(&mut c, &graph, &targets, 1.0, &mut Rng::new(35));
        assert!(
            c.get(far),
            "edge {far} touches no target and must stay removed"
        );
        assert_eq!(c.count_ones(), 1);
    }

    #[test]
    fn gene_repair_is_reproducible_from_a_seed() {
        let mut setup = Rng::new(36);
        let graph = random_graph(15, 15, &mut setup);
        let targets = RepairTargets::new(&graph, 4);
        let base = Chromosome::random(graph.edge_count(), 0.6, &mut setup);
        let mut a = base.clone();
        let mut b = base.clone();
        gene_repair(&mut a, &graph, &targets, 0.5, &mut Rng::new(37));
        gene_repair(&mut b, &graph, &targets, 0.5, &mut Rng::new(37));
        assert_eq!(a, b);
    }

    #[test]
    fn gene_repair_with_no_targets_is_a_no_op() {
        let graph = Graph::new(4, &[(0, 1), (1, 2), (2, 3)]).expect("valid graph");
        let targets = RepairTargets::new(&graph, 0);
        let mut c = Chromosome::random(graph.edge_count(), 0.5, &mut Rng::new(38));
        let before = c.clone();
        gene_repair(&mut c, &graph, &targets, 1.0, &mut Rng::new(39));
        assert_eq!(c, before);
    }
}
