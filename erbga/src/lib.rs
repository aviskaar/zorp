//! ERBGA: a genetic algorithm for graph community detection.
//!
//! An implementation of Rao, Janikow, Bhatia, and Climer, "Efficient
//! Reduced-Bias Genetic Algorithm (ERBGA) for Generic Community Detection
//! Objectives", MWAIS 2018 Proceedings 32, and the thesis of the same
//! name.
//!
//! The idea it is built on: encoding a clustering by labelling vertices
//! means `k!` distinct chromosomes describe the same clustering, which
//! inflates the search space by a factor of `k!`. Encoding it as the set
//! of *removed edges* removes that redundancy, and needs no advance
//! knowledge of how many communities there are.
//!
//! One correction to the source. It claims the encoding represents each
//! clustering "exactly once". That holds for label permutations, which is
//! the contribution, but not in general: on a triangle, removing nothing
//! and removing a single edge both leave one component, because the
//! endpoints stay connected through the third vertex. The collapse factor
//! is at least `2^(|E| - |V| + c)` for `c` components, so it is largest on
//! dense graphs. The redundancy is neutral for search (it forms neutral
//! networks) but it means the encoding is many-to-one onto partitions, and
//! `graph::Graph::partition` is what canonicalizes.
//!
//! This crate knows nothing about zorp. It takes a graph and an objective
//! and searches.

pub mod chromosome;
pub mod ga;
pub mod graph;
pub mod objective;
pub mod operators;
pub mod rng;
pub mod selection;

pub use chromosome::Chromosome;
pub use ga::{
    best_of, run_island, run_island_on, run_islands, run_islands_on, Best, GaParams, Problem,
};
pub use graph::{Graph, GraphError, Partition};
pub use objective::{Modularity, Objective};
pub use rng::Rng;
