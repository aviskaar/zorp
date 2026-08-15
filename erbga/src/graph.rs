//! The graph, its canonical edge list, and the partition a chromosome induces.
//!
//! Two things here carry the encoding. The edge list is canonical and
//! sorted, so gene `i` names the same edge on every run and in every
//! chromosome, which is what lets crossover mix two parents at all. And
//! `partition` maps a chromosome onto the clustering it induces, which is
//! the only place the many-to-one encoding gets collapsed to something
//! comparable.

use crate::chromosome::Chromosome;

#[derive(Debug, PartialEq, Eq)]
pub enum GraphError {
    VertexOutOfRange { vertex: usize, n_vertices: usize },
    SelfLoop { vertex: usize },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::VertexOutOfRange { vertex, n_vertices } => write!(
                f,
                "vertex {vertex} is out of range for a graph of {n_vertices} vertices"
            ),
            GraphError::SelfLoop { vertex } => write!(
                f,
                "vertex {vertex} has a self loop, which cannot be cut and would waste a gene"
            ),
        }
    }
}

impl std::error::Error for GraphError {}

/// A clustering: one community label per vertex, numbered from 0.
#[derive(Clone, Debug)]
pub struct Partition {
    labels: Vec<usize>,
    count: usize,
}

impl Partition {
    pub fn label(&self, v: usize) -> usize {
        self.labels[v]
    }

    pub fn community_count(&self) -> usize {
        self.count
    }

    /// Labels indexed by vertex. Objectives want the whole array at once.
    pub fn labels(&self) -> &[usize] {
        &self.labels
    }
}

/// An undirected graph with a fixed edge ordering.
///
/// Incidence is stored as a flat array plus offsets rather than as a
/// `Vec<Vec<usize>>`. On the graphs this targets that is one allocation
/// instead of a hundred thousand, and neighbors of a vertex end up
/// contiguous, which matters because walking them is the inner loop of
/// every single fitness evaluation.
#[derive(Clone, Debug)]
pub struct Graph {
    n_vertices: usize,
    edges: Vec<(usize, usize)>,
    incident_offsets: Vec<usize>,
    incident_edges: Vec<usize>,
}

impl Graph {
    /// Validate, canonicalize, deduplicate, and sort by `edge_id`.
    ///
    /// The sorted result is the paper's `EdgeList`, and its order is the
    /// gene order of every chromosome for this graph. Duplicates have to go
    /// before that: two genes for one edge would let a chromosome "half
    /// remove" it, and the second gene would do nothing at all.
    pub fn new(n_vertices: usize, edges: &[(usize, usize)]) -> Result<Self, GraphError> {
        let mut canonical = Vec::with_capacity(edges.len());
        for &(a, b) in edges {
            for v in [a, b] {
                if v >= n_vertices {
                    return Err(GraphError::VertexOutOfRange {
                        vertex: v,
                        n_vertices,
                    });
                }
            }
            if a == b {
                return Err(GraphError::SelfLoop { vertex: a });
            }
            canonical.push((a.max(b), a.min(b)));
        }

        // Sorting the `(hi, lo)` pairs is sorting by `edge_id`: `lo` is
        // always below `n_vertices`, so `n * hi + lo` orders exactly the
        // way the tuples do, without building the ids.
        canonical.sort_unstable();
        canonical.dedup();

        // Counting sort into the flat incidence array. `offsets[v + 1]`
        // starts as the degree of `v`, becomes the end of `v`'s block after
        // the prefix sum, and `cursor` walks each block as edges are filed.
        let mut incident_offsets = vec![0usize; n_vertices + 1];
        for &(hi, lo) in &canonical {
            incident_offsets[hi + 1] += 1;
            incident_offsets[lo + 1] += 1;
        }
        for v in 0..n_vertices {
            incident_offsets[v + 1] += incident_offsets[v];
        }
        let mut cursor = incident_offsets.clone();
        let mut incident_edges = vec![0usize; 2 * canonical.len()];
        for (i, &(hi, lo)) in canonical.iter().enumerate() {
            incident_edges[cursor[hi]] = i;
            cursor[hi] += 1;
            incident_edges[cursor[lo]] = i;
            cursor[lo] += 1;
        }

        Ok(Graph {
            n_vertices,
            edges: canonical,
            incident_offsets,
            incident_edges,
        })
    }

    pub fn vertex_count(&self) -> usize {
        self.n_vertices
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Canonical order: larger endpoint first, ascending by `edge_id`.
    /// The index of an edge here is its gene index.
    pub fn edges(&self) -> &[(usize, usize)] {
        &self.edges
    }

    /// Gene indices of the edges touching `v`, ascending.
    pub fn incident(&self, v: usize) -> &[usize] {
        &self.incident_edges[self.incident_offsets[v]..self.incident_offsets[v + 1]]
    }

    pub fn degree(&self, v: usize) -> usize {
        self.incident_offsets[v + 1] - self.incident_offsets[v]
    }

    /// The clustering `c` induces: connected components over the edges
    /// whose gene is still 0.
    ///
    /// The traversal keeps its own stack because the source's graphs run to
    /// 100k vertices and beyond, where a recursive walk of a long path
    /// overflows the real stack. Components are found by scanning vertices
    /// in ascending order, so the labels depend on the clustering alone and
    /// not on which chromosome produced it.
    pub fn partition(&self, c: &Chromosome) -> Partition {
        assert_eq!(
            c.len(),
            self.edges.len(),
            "chromosome length does not match the edge count"
        );

        const UNLABELED: usize = usize::MAX;
        let mut labels = vec![UNLABELED; self.n_vertices];
        let mut count = 0;
        let mut stack: Vec<usize> = Vec::new();

        for root in 0..self.n_vertices {
            if labels[root] != UNLABELED {
                continue;
            }
            labels[root] = count;
            stack.push(root);
            while let Some(v) = stack.pop() {
                for &e in self.incident(v) {
                    if c.get(e) {
                        continue;
                    }
                    let (hi, lo) = self.edges[e];
                    let other = if hi == v { lo } else { hi };
                    if labels[other] == UNLABELED {
                        labels[other] = count;
                        stack.push(other);
                    }
                }
            }
            count += 1;
        }

        Partition { labels, count }
    }
}

/// A single integer naming an unordered pair, larger endpoint first.
///
/// `u64` rather than `usize` because a 100k vertex graph needs 34 bits and
/// the crate has to behave the same on a 32 bit target.
pub fn edge_id(n_vertices: usize, a: usize, b: usize) -> u64 {
    let (hi, lo) = (a.max(b) as u64, a.min(b) as u64);
    n_vertices as u64 * hi + lo
}

/// The inverse of [`edge_id`], returning `(hi, lo)`. Panics if
/// `n_vertices` is 0, which no real id can have been built from.
pub fn decode_edge_id(n_vertices: usize, id: u64) -> (usize, usize) {
    let n = n_vertices as u64;
    ((id / n) as usize, (id % n) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Two triangles joined by one edge, the fixture the whole crate uses.
    fn two_triangles() -> Graph {
        Graph::new(6, &[(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (2, 3)]).unwrap()
    }

    fn all_removed(g: &Graph) -> Chromosome {
        let mut c = Chromosome::zeros(g.edge_count());
        for i in 0..c.len() {
            c.set(i, true);
        }
        c
    }

    /// Both orderings of a pair have to land on one id, or the dedup in
    /// `Graph::new` would keep `(1, 2)` and `(2, 1)` as two edges and every
    /// gene index after them would shift.
    #[test]
    fn edge_id_round_trips_in_both_orderings() {
        let n = 37;
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let id = edge_id(n, a, b);
                assert_eq!(id, edge_id(n, b, a), "pair ({a}, {b}) is order sensitive");
                assert_eq!(decode_edge_id(n, id), (a.max(b), a.min(b)));
            }
        }
    }

    #[test]
    fn edge_id_is_unique_per_pair() {
        let n = 40;
        let mut seen = HashSet::new();
        for a in 0..n {
            for b in (a + 1)..n {
                assert!(seen.insert(edge_id(n, a, b)), "collision on ({a}, {b})");
            }
        }
    }

    /// The ids of a 100k vertex graph do not fit in 32 bits, which is the
    /// reason the id is a `u64` rather than a `usize` on every platform.
    #[test]
    fn edge_id_survives_a_large_vertex_count() {
        let n = 200_000;
        let id = edge_id(n, 123_456, 199_999);
        assert!(id > u32::MAX as u64);
        assert_eq!(decode_edge_id(n, id), (199_999, 123_456));
    }

    #[test]
    fn new_rejects_a_vertex_past_the_end() {
        let err = Graph::new(3, &[(0, 1), (1, 3)]).unwrap_err();
        assert_eq!(
            err,
            GraphError::VertexOutOfRange {
                vertex: 3,
                n_vertices: 3
            }
        );
        let err = Graph::new(3, &[(9, 1)]).unwrap_err();
        assert_eq!(
            err,
            GraphError::VertexOutOfRange {
                vertex: 9,
                n_vertices: 3
            }
        );
        assert!(Graph::new(0, &[(0, 1)]).is_err());
    }

    #[test]
    fn new_rejects_self_loops() {
        let err = Graph::new(4, &[(0, 1), (2, 2)]).unwrap_err();
        assert_eq!(err, GraphError::SelfLoop { vertex: 2 });
    }

    #[test]
    fn new_deduplicates_and_canonicalizes() {
        let g = Graph::new(3, &[(1, 2), (2, 1), (0, 1), (1, 2)]).unwrap();
        assert_eq!(g.edge_count(), 2);
        // Larger endpoint first, ascending by edge_id.
        assert_eq!(g.edges(), &[(1, 0), (2, 1)]);
    }

    #[test]
    fn edges_come_back_sorted_by_edge_id() {
        let g = Graph::new(6, &[(4, 5), (0, 1), (2, 3), (1, 2), (3, 5)]).unwrap();
        let ids: Vec<u64> = g.edges().iter().map(|&(a, b)| edge_id(6, a, b)).collect();
        let mut want = ids.clone();
        want.sort_unstable();
        assert_eq!(ids, want);
        assert!(g.edges().iter().all(|&(a, b)| a > b), "not canonical");
    }

    #[test]
    fn incident_and_degree_agree_with_the_edge_list() {
        let g = two_triangles();
        for v in 0..g.vertex_count() {
            let want: Vec<usize> = g
                .edges()
                .iter()
                .enumerate()
                .filter(|(_, &(a, b))| a == v || b == v)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(g.incident(v), want.as_slice(), "vertex {v}");
            assert_eq!(g.degree(v), want.len(), "vertex {v}");
        }
    }

    #[test]
    fn degrees_sum_to_twice_the_edge_count() {
        let g = two_triangles();
        let total: usize = (0..g.vertex_count()).map(|v| g.degree(v)).sum();
        assert_eq!(total, 2 * g.edge_count());
    }

    #[test]
    fn a_graph_with_no_edges_is_valid() {
        let g = Graph::new(3, &[]).unwrap();
        assert_eq!(g.edge_count(), 0);
        assert_eq!(g.degree(2), 0);
        assert!(g.incident(2).is_empty());
        assert_eq!(g.partition(&Chromosome::zeros(0)).community_count(), 3);
    }

    #[test]
    fn a_graph_with_no_vertices_is_valid() {
        let g = Graph::new(0, &[]).unwrap();
        assert_eq!(g.vertex_count(), 0);
        let p = g.partition(&Chromosome::zeros(0));
        assert_eq!(p.community_count(), 0);
        assert!(p.labels().is_empty());
    }

    #[test]
    fn nothing_removed_leaves_one_community() {
        let g = two_triangles();
        let p = g.partition(&Chromosome::zeros(g.edge_count()));
        assert_eq!(p.community_count(), 1);
        assert_eq!(p.labels(), &[0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn everything_removed_leaves_singletons() {
        let g = two_triangles();
        let p = g.partition(&all_removed(&g));
        assert_eq!(p.community_count(), g.vertex_count());
        // The ascending scan makes vertex v community v when all are alone.
        assert_eq!(p.labels(), &[0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn removing_the_bridge_leaves_exactly_two_communities() {
        let g = two_triangles();
        let bridge = g
            .edges()
            .iter()
            .position(|&(a, b)| (a, b) == (3, 2))
            .expect("the bridge is in the edge list");
        let mut c = Chromosome::zeros(g.edge_count());
        c.set(bridge, true);
        let p = g.partition(&c);
        assert_eq!(p.community_count(), 2);
        assert_eq!(p.labels(), &[0, 0, 0, 1, 1, 1]);
    }

    /// The source claims the removed-edge encoding represents each
    /// clustering "exactly once". It does not, and this is the correction.
    /// On a triangle the empty removal set and every single-edge removal
    /// set induce the same one community partition, because the endpoints
    /// of the cut edge stay joined through the third vertex. The claim
    /// holds for label permutations, which is the real contribution, but
    /// the encoding is still many-to-one onto partitions, and `partition`
    /// is what canonicalizes.
    #[test]
    fn distinct_chromosomes_can_induce_the_same_partition() {
        let g = Graph::new(3, &[(0, 1), (1, 2), (0, 2)]).unwrap();
        let none = g.partition(&Chromosome::zeros(3));
        assert_eq!(none.community_count(), 1);
        for i in 0..g.edge_count() {
            let mut c = Chromosome::zeros(g.edge_count());
            c.set(i, true);
            let p = g.partition(&c);
            assert_eq!(
                p.community_count(),
                1,
                "cutting edge {i} of a triangle should not disconnect it"
            );
            assert_eq!(p.labels(), none.labels());
        }
    }

    /// Labels have to be a function of the partition alone, never of the
    /// order components happen to be discovered in, or two runs that find
    /// the same clustering would look different to a caller.
    #[test]
    fn labels_are_numbered_by_an_ascending_vertex_scan() {
        let g = Graph::new(5, &[(3, 1)]).unwrap();
        let p = g.partition(&Chromosome::zeros(1));
        assert_eq!(p.labels(), &[0, 1, 2, 1, 3]);
        assert_eq!(p.community_count(), 4);
        assert_eq!(p.label(3), p.label(1));
        assert_ne!(p.label(0), p.label(2));
    }

    #[test]
    fn isolated_vertices_are_their_own_community() {
        let g = Graph::new(4, &[(0, 1)]).unwrap();
        let p = g.partition(&Chromosome::zeros(1));
        assert_eq!(p.community_count(), 3);
        assert_eq!(p.label(0), p.label(1));
        assert_ne!(p.label(2), p.label(3));
    }

    /// A recursive traversal blows the stack well before this size, and
    /// the source's target graphs are this size and larger.
    #[test]
    fn a_long_path_does_not_overflow_the_stack() {
        let n = 100_000;
        let edges: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).collect();
        let g = Graph::new(n, &edges).unwrap();
        assert_eq!(g.edge_count(), n - 1);
        let p = g.partition(&Chromosome::zeros(g.edge_count()));
        assert_eq!(p.community_count(), 1);

        let p = g.partition(&all_removed(&g));
        assert_eq!(p.community_count(), n);
    }

    /// A chromosome of the wrong length means gene `i` is not edge `i`,
    /// so the partition would be silently wrong rather than loudly.
    #[test]
    #[should_panic(expected = "chromosome length")]
    fn partition_rejects_a_chromosome_of_the_wrong_length() {
        let g = two_triangles();
        g.partition(&Chromosome::zeros(g.edge_count() - 1));
    }

    #[test]
    fn errors_describe_themselves() {
        let e = GraphError::VertexOutOfRange {
            vertex: 9,
            n_vertices: 4,
        };
        let text = e.to_string();
        assert!(text.contains('9') && text.contains('4'), "{text}");
        let text = GraphError::SelfLoop { vertex: 2 }.to_string();
        assert!(text.contains('2'), "{text}");
    }
}
