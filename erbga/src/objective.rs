//! What a partition is worth.
//!
//! The encoding is the contribution of the source work, not the measure, so
//! the measure is a trait. Anything that reads a clustering and returns a
//! number the search should maximize can be dropped in here.

use crate::graph::{Graph, Partition};

/// Higher is better. The search only ever maximizes.
pub trait Objective {
    fn score(&self, graph: &Graph, partition: &Partition) -> f64;
}

/// Newman and Girvan modularity.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modularity;

impl Objective for Modularity {
    /// `Q = sum_c [ L_c / m - (D_c / 2m)^2 ]`.
    ///
    /// Every quantity is read off the original graph. Only the community
    /// membership comes from the cut. Scoring the cut graph instead would
    /// hand the search a free win: remove every edge and there is nothing
    /// left to be badly placed, so `Q` would read 0.0 rather than the
    /// strongly negative number the original degrees give it.
    fn score(&self, graph: &Graph, partition: &Partition) -> f64 {
        assert_eq!(
            partition.labels().len(),
            graph.vertex_count(),
            "partition does not belong to this graph"
        );

        let m = graph.edge_count();
        if m == 0 {
            return 0.0;
        }

        // Integer accumulation, because summing `1/m` per edge in floating
        // point makes the score depend on the order edges happen to sit in.
        let mut internal = vec![0u64; partition.community_count()];
        let mut degree_sum = vec![0u64; partition.community_count()];
        for v in 0..graph.vertex_count() {
            degree_sum[partition.label(v)] += graph.degree(v) as u64;
        }
        for &(a, b) in graph.edges() {
            let (ca, cb) = (partition.label(a), partition.label(b));
            if ca == cb {
                internal[ca] += 1;
            }
        }

        let m = m as f64;
        let mut q = 0.0;
        for c in 0..partition.community_count() {
            let fraction = degree_sum[c] as f64 / (2.0 * m);
            q += internal[c] as f64 / m - fraction * fraction;
        }
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chromosome::Chromosome;

    const EPS: f64 = 1e-9;

    /// Two triangles joined by the single edge (2, 3). m = 7.
    fn two_triangles() -> Graph {
        Graph::new(6, &[(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (2, 3)]).unwrap()
    }

    fn gene_of(g: &Graph, a: usize, b: usize) -> usize {
        let (hi, lo) = (a.max(b), a.min(b));
        g.edges()
            .iter()
            .position(|&e| e == (hi, lo))
            .unwrap_or_else(|| panic!("({a}, {b}) is not an edge"))
    }

    fn cut(g: &Graph, pairs: &[(usize, usize)]) -> Chromosome {
        let mut c = Chromosome::zeros(g.edge_count());
        for &(a, b) in pairs {
            c.set(gene_of(g, a, b), true);
        }
        c
    }

    fn all_removed(g: &Graph) -> Chromosome {
        let mut c = Chromosome::zeros(g.edge_count());
        for i in 0..c.len() {
            c.set(i, true);
        }
        c
    }

    fn parse_edges(text: &str) -> Graph {
        let mut lines = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'));
        let mut header = lines.next().expect("header line").split_whitespace();
        let n: usize = header.next().unwrap().parse().unwrap();
        let m: usize = header.next().unwrap().parse().unwrap();
        let edges: Vec<(usize, usize)> = lines
            .map(|l| {
                let mut p = l.split_whitespace();
                (
                    p.next().unwrap().parse().unwrap(),
                    p.next().unwrap().parse().unwrap(),
                )
            })
            .collect();
        assert_eq!(edges.len(), m, "header edge count disagrees with the body");
        Graph::new(n, &edges).unwrap()
    }

    #[test]
    fn splitting_the_two_triangles_gives_five_fourteenths() {
        let g = two_triangles();
        let p = g.partition(&cut(&g, &[(2, 3)]));
        assert_eq!(p.community_count(), 2);
        let q = Modularity.score(&g, &p);
        assert!((q - 5.0 / 14.0).abs() < EPS, "expected 5/14, got {q}");
    }

    #[test]
    fn one_community_scores_zero() {
        let g = two_triangles();
        let p = g.partition(&Chromosome::zeros(g.edge_count()));
        assert_eq!(p.community_count(), 1);
        let q = Modularity.score(&g, &p);
        assert!(q.abs() < EPS, "expected 0, got {q}");
    }

    #[test]
    fn all_singletons_score_negative() {
        let g = two_triangles();
        let p = g.partition(&all_removed(&g));
        assert_eq!(p.community_count(), 6);
        let q = Modularity.score(&g, &p);
        // -sum(d_v^2) / (2m)^2 = -34/196.
        assert!((q + 34.0 / 196.0).abs() < EPS, "expected -34/196, got {q}");
        assert!(q < 0.0);
    }

    /// The detail the whole objective turns on. Modularity is read off the
    /// original edges and degrees, with only the communities taken from the
    /// cut. Scored over the cut graph instead, removing everything would
    /// leave no edges and no degrees, which reads as 0.0 and beats most
    /// real clusterings, so the search would converge on the empty graph.
    #[test]
    fn removing_every_edge_does_not_score_well() {
        let g = two_triangles();
        let empty = Modularity.score(&g, &g.partition(&all_removed(&g)));
        let split = Modularity.score(&g, &g.partition(&cut(&g, &[(2, 3)])));
        let whole = Modularity.score(&g, &g.partition(&Chromosome::zeros(g.edge_count())));
        assert!(empty < whole, "{empty} should lose to the single community");
        assert!(empty < split, "{empty} should lose to the two triangles");
    }

    /// The encoding is many-to-one onto partitions, so the score has to
    /// depend on the partition alone. Cutting one edge of a triangle leaves
    /// it connected, so it must score exactly what cutting nothing scores.
    #[test]
    fn chromosomes_inducing_the_same_partition_score_the_same() {
        let g = two_triangles();
        let none = Modularity.score(&g, &g.partition(&Chromosome::zeros(g.edge_count())));
        let redundant = Modularity.score(&g, &g.partition(&cut(&g, &[(0, 1)])));
        assert_eq!(none, redundant);
    }

    #[test]
    fn a_graph_with_no_edges_scores_zero() {
        let g = Graph::new(5, &[]).unwrap();
        let p = g.partition(&Chromosome::zeros(0));
        assert_eq!(Modularity.score(&g, &p), 0.0);
    }

    #[test]
    fn modularity_works_behind_a_trait_object() {
        let g = two_triangles();
        let p = g.partition(&cut(&g, &[(2, 3)]));
        let objective: &dyn Objective = &Modularity;
        assert!((objective.score(&g, &p) - 5.0 / 14.0).abs() < EPS);
    }

    /// Zachary's karate club, cut along the real faction boundary.
    ///
    /// The file is 0-indexed but `fetch.py` numbered the vertices by
    /// sorting the original ids as strings, so file vertex `i` is original
    /// node `order[i]`. The expected value is the unweighted modularity of
    /// Zachary's two factions. Widely quoted figures near 0.39 are for the
    /// weighted graph, which is not what this crate searches over.
    #[test]
    fn karate_club_ground_truth_split_is_scored_correctly() {
        let g = parse_edges(include_str!("../tests/data/karate.edges"));
        assert_eq!(g.vertex_count(), 34);
        assert_eq!(g.edge_count(), 78);

        let mut order: Vec<usize> = (0..34).collect();
        order.sort_by_key(|v| v.to_string());
        const MR_HI: [usize; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 16, 17, 19, 21];
        let faction = |v: usize| MR_HI.contains(&order[v]);

        let mut c = Chromosome::zeros(g.edge_count());
        for (i, &(a, b)) in g.edges().iter().enumerate() {
            if faction(a) != faction(b) {
                c.set(i, true);
            }
        }
        let p = g.partition(&c);
        assert_eq!(p.community_count(), 2, "each faction is connected");

        let q = Modularity.score(&g, &p);
        assert!((q - 0.358_234_714_003_944_7).abs() < EPS, "karate Q is {q}");
    }

    #[test]
    fn karate_club_extremes_bracket_the_ground_truth() {
        let g = parse_edges(include_str!("../tests/data/karate.edges"));
        let whole = Modularity.score(&g, &g.partition(&Chromosome::zeros(g.edge_count())));
        let singletons = Modularity.score(&g, &g.partition(&all_removed(&g)));
        assert!(
            whole.abs() < EPS,
            "one community should score 0, got {whole}"
        );
        assert!(singletons < 0.0, "singletons should score below 0");
        // No partition of any graph can exceed 1.
        assert!(singletons > -1.0 && whole <= 1.0);
    }
}
