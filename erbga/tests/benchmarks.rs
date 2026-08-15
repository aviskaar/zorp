//! Reproduction of the ERBGA benchmark results.
//!
//! Targets are the values the thesis reports in Table 3, including the two
//! networks the method did badly on. Gating only on the successes would be
//! choosing the benchmark after seeing which ones the method passes.
//! Reproducing 0.073 on Football is a stronger correctness signal than
//! reproducing 0.420 on Karate, because far fewer wrong implementations
//! produce it.
//!
//! The full benchmark is `#[ignore]` because it is stochastic and slow.
//! The thesis reports that 4 of 25 islands reached 0.420 on Karate and the
//! rest landed near 0.397, so a small-island run of a correct
//! implementation fails a tight assertion a large fraction of the time.
//! The published numbers also come from runs capped at 48 hours, which is
//! not a CI budget. What runs unattended is the seeded smoke test below,
//! which is deterministic.
//!
//! Run the full set with:
//!   cargo test -p erbga --release --test benchmarks -- --ignored --nocapture

use erbga::{best_of, run_islands, GaParams, Graph, Modularity};

/// Thesis Table 3, plus the E/V density from Table 2.
struct Benchmark {
    name: &'static str,
    vertices: usize,
    edges: usize,
    /// Best-known result across published modularity optimizers.
    bkr: f64,
    /// What ERBGA itself reported.
    erbga: f64,
}

const BENCHMARKS: &[Benchmark] = &[
    Benchmark {
        name: "karate",
        vertices: 34,
        edges: 78,
        bkr: 0.420,
        erbga: 0.420,
    },
    Benchmark {
        name: "dolphins",
        vertices: 62,
        edges: 159,
        bkr: 0.529,
        erbga: 0.445,
    },
    Benchmark {
        name: "polbooks",
        vertices: 105,
        edges: 441,
        bkr: 0.527,
        erbga: 0.256,
    },
    Benchmark {
        name: "football",
        vertices: 115,
        edges: 613,
        bkr: 0.605,
        erbga: 0.073,
    },
];

/// Load a fixture written by `tests/data/fetch.py`: a comment line, then
/// `n_vertices n_edges`, then one `u v` per line, 0-indexed.
fn load(name: &str) -> Graph {
    let path = format!("{}/tests/data/{name}.edges", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));

    let mut lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty());
    let header = lines.next().expect("fixture has no header line");
    let mut header = header.split_whitespace();
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
    assert_eq!(
        edges.len(),
        m,
        "{name}: header claims {m} edges, found {}",
        edges.len()
    );

    let g = Graph::new(n, &edges).expect("fixture is a valid graph");
    assert_eq!(g.vertex_count(), n);
    assert_eq!(g.edge_count(), m);
    g
}

/// The fixtures must match the counts in thesis Table 2, or the benchmark
/// is measuring a different network than the one being reproduced.
#[test]
fn fixtures_match_the_published_counts() {
    for b in BENCHMARKS {
        let g = load(b.name);
        assert_eq!(g.vertex_count(), b.vertices, "{}: vertex count", b.name);
        assert_eq!(g.edge_count(), b.edges, "{}: edge count", b.name);
    }
}

/// Karate's exact modularity optimum, established independently by a
/// clique-partitioning ILP solved to proven optimality and cross-checked
/// against brute force. The optimal partition has four communities
/// holding {6, 7, 21, 23} internal edges and {16, 24, 56, 60} total
/// degree.
const KARATE_EXACT_OPTIMUM: f64 = 0.419_790;

/// A one-sided correctness check on the objective rather than on the
/// search. No partition of Karate can score above the exact optimum, so
/// a higher number means modularity is over-counting somewhere, which a
/// benchmark comparing against a lower published value would happily
/// report as success.
#[test]
fn no_search_result_can_exceed_karate_exact_optimum() {
    let g = load("karate");
    let params = GaParams {
        population_size: 80,
        generations: 400,
        ..GaParams::thesis()
    };
    for r in run_islands(&g, &Modularity, &params, 5, 20_260_814) {
        assert!(
            r.fitness <= KARATE_EXACT_OPTIMUM + 1e-6,
            "scored Q={:.6}, above Karate's proven optimum {KARATE_EXACT_OPTIMUM:.6}; \
             the objective is over-counting",
            r.fitness
        );
    }
}

/// Deterministic and quick. Catches a broken engine (which scores near
/// zero or negative) without depending on reaching the published optimum.
#[test]
fn karate_smoke() {
    let g = load("karate");
    let params = GaParams {
        population_size: 60,
        generations: 300,
        ..GaParams::thesis()
    };
    let results = run_islands(&g, &Modularity, &params, 3, 20_260_814);
    let best = best_of(&results);
    assert!(
        best.fitness > 0.30,
        "karate smoke reached only Q={:.4}, engine is likely broken",
        best.fitness
    );
}

/// The full reproduction. Reports every network against both the thesis's
/// own result and the best-known result, under both disputed parameter
/// sets, rather than asserting one number and hiding the rest.
#[test]
#[ignore = "stochastic and slow; run explicitly with --ignored --release"]
fn reproduces_thesis_table_3() {
    let islands = 25;
    let params_sets = [("thesis", GaParams::thesis()), ("paper", GaParams::paper())];

    // Report the median as well as the best. The published values come
    // from a best-of-islands protocol, and a max over 25 stochastic runs
    // says little about whether a typical run is healthy. The median is
    // the more powerful signal per unit of compute and is not flaky.
    println!(
        "\n{:<10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>10}",
        "network", "params", "best", "median", "erbga", "bkr", "vs bkr"
    );
    let mut failures = Vec::new();

    for b in BENCHMARKS {
        let g = load(b.name);
        for (label, base) in &params_sets {
            let params = GaParams {
                generations: 1000,
                ..base.clone()
            };
            let results = run_islands(&g, &Modularity, &params, islands, 20_260_814);
            let best = best_of(&results).fitness;

            let mut scores: Vec<f64> = results.iter().map(|r| r.fitness).collect();
            scores.sort_by(f64::total_cmp);
            let median = scores[scores.len() / 2];

            println!(
                "{:<10} {:>8} {:>8.4} {:>8.4} {:>8.3} {:>8.3} {:>9.1}%",
                b.name,
                label,
                best,
                median,
                b.erbga,
                b.bkr,
                100.0 * best / b.bkr
            );

            // One-sided against the published value: this catches a gross
            // regression, not a subtly wrong implementation. The real
            // correctness checks are the Karate exact-optimum bound above
            // and the unit suite, not this.
            if *label == "thesis" && best < b.erbga - 0.05 {
                failures.push(format!(
                    "{}: got {best:.4}, thesis reports {:.3}",
                    b.name, b.erbga
                ));
            }
            // No result may exceed the best known value. If one does,
            // modularity is over-counting.
            if best > b.bkr + 1e-6 {
                failures.push(format!(
                    "{}: got {best:.4}, above the best known {:.3}; objective is wrong",
                    b.name, b.bkr
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "benchmark failures:\n  {}",
        failures.join("\n  ")
    );
}
