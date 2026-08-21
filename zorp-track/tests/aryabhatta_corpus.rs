//! aryabhatta over a real corpus.
//!
//! Every number this produces comes from actually running something.
//! The graphs are the four networks the ERBGA thesis reports on, the
//! outcomes are modularity scores from real searches over them, and the
//! forecasts are made before each search runs, from the runs that came
//! before it.
//!
//! **What this does and does not measure.** It measures whether
//! intervals derived from a stochastic algorithm's own past runs cover
//! its future runs, and it exercises every reader in the discovery
//! layer against a record that was genuinely produced rather than
//! fixtured. It does *not* measure what the design cares most about,
//! which is whether an *agent's* stated intervals have coverage. That
//! is the endogenous case, it needs a model endpoint, and no forecast
//! here comes from a model. Read the numbers as a first observed
//! calibration curve for a documented statistical forecaster, and as
//! evidence the engine works end to end. Not as evidence about zorp's
//! own calibration.
//!
//! The forecaster is deliberately simple and stated in full: for run
//! `k` on a graph, take the mean and sample standard deviation of the
//! previous runs on that same graph and forecast
//! `mean +/- z(confidence) * sd`. Nothing is tuned and no interval is
//! widened by hand. A forecaster whose intervals were adjusted after
//! seeing the coverage would be measuring itself.
//!
//! Ignored by default because it runs hundreds of genetic algorithm
//! searches. Run it with:
//!   cargo test -p zorp-track --release --test aryabhatta_corpus -- --ignored --nocapture

use erbga::{best_of, run_islands, GaParams, Graph, Modularity};
use zorp_track::calibration::{z_for_confidence, Tolerance};
use zorp_track::experiment::MetricValue;
use zorp_track::track::Store;

/// The four networks from the thesis, with the vertex and edge counts
/// it reports. The counts are asserted against the fixtures so a
/// corrupt fixture cannot quietly change what is being measured.
const GRAPHS: [(&str, usize, usize); 4] = [
    ("karate", 34, 78),
    ("dolphins", 62, 159),
    ("polbooks", 105, 441),
    ("football", 115, 613),
];

/// Runs per graph. The first `WARMUP` of each have no history to
/// forecast from and are recorded as outcomes without an expectation,
/// which is exactly what the calibration report is built to skip.
const RUNS_PER_GRAPH: usize = 20;
const WARMUP: usize = 3;

/// Two stated confidences, so the calibration curve has more than one
/// point. A curve with a single point cannot show a slope, and the
/// slope is the thing worth looking at.
const CONFIDENCES: [f64; 2] = [0.80, 0.95];

fn load(name: &str) -> Graph {
    let path = format!(
        "{}/../erbga/tests/data/{name}.edges",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let mut lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty());
    let mut header = lines
        .next()
        .expect("fixture has no header")
        .split_whitespace();
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
        "{name}: fixture edge count disagrees with its header"
    );
    Graph::new(n, &edges).unwrap_or_else(|e| panic!("{name}: building graph: {e:?}"))
}

/// One real search. Returns the modularity it reached.
fn measure(graph: &Graph, seed: u64) -> f64 {
    let params = GaParams {
        population_size: 60,
        generations: 300,
        ..GaParams::thesis()
    };
    let results = run_islands(graph, &Modularity, &params, 3, seed);
    best_of(&results).fitness
}

/// Mean and sample standard deviation. `None` when there is not enough
/// history, or when every prior run landed on the same number: a zero
/// width interval is a claim of certainty, and widening it by hand to
/// avoid that would be inventing the forecast this is supposed to be
/// measuring.
fn mean_and_sd(history: &[f64]) -> Option<(f64, f64)> {
    if history.len() < 2 {
        return None;
    }
    let n = history.len() as f64;
    let mean = history.iter().sum::<f64>() / n;
    let var = history.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let sd = var.sqrt();
    if sd <= 0.0 || !sd.is_finite() {
        return None;
    }
    Some((mean, sd))
}

#[test]
#[ignore = "runs hundreds of real GA searches; run with --ignored --release"]
fn aryabhatta_over_the_erbga_benchmarks() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
    store
        .create_track(
            "erbga",
            "intervals from a stochastic search's own past runs cover its future runs",
        )
        .unwrap();

    let mut skipped_no_history = 0usize;
    let mut skipped_zero_sd = 0usize;
    let mut recorded = 0usize;
    // Experiments whose outcome landed outside its own interval.
    let mut misses: Vec<(String, &str, usize, usize)> = Vec::new();

    for (gi, (name, vertices, edges)) in GRAPHS.iter().enumerate() {
        let graph = load(name);
        assert_eq!(graph.vertex_count(), *vertices, "{name}: vertex count");
        assert_eq!(graph.edge_count(), *edges, "{name}: edge count");

        let mut history: Vec<f64> = Vec::new();

        for run in 0..RUNS_PER_GRAPH {
            // A distinct seed per (graph, run), fixed rather than
            // random so the whole corpus is reproducible.
            let seed = 20_260_821 + (gi as u64) * 1_000 + run as u64;
            let experiment = store.create_experiment("erbga", "prereg").unwrap();

            // Conditions: what this run was performed under. All
            // observed, none of them prose.
            store
                .record_condition(
                    &experiment.id,
                    "graph",
                    &MetricValue::Text(name.to_string()),
                )
                .unwrap();
            store
                .record_condition(
                    &experiment.id,
                    "vertices",
                    &MetricValue::Number(*vertices as f64),
                )
                .unwrap();
            store
                .record_condition(&experiment.id, "edges", &MetricValue::Number(*edges as f64))
                .unwrap();
            store
                .record_condition(&experiment.id, "islands", &MetricValue::Number(3.0))
                .unwrap();
            store
                .record_condition(&experiment.id, "generations", &MetricValue::Number(300.0))
                .unwrap();
            store
                .record_condition(
                    &experiment.id,
                    "population_size",
                    &MetricValue::Number(60.0),
                )
                .unwrap();

            // The forecast, before the search runs. `run % 2` alternates
            // the stated confidence so both bands fill up evenly.
            let confidence = CONFIDENCES[run % CONFIDENCES.len()];
            let forecast = if run < WARMUP {
                skipped_no_history += 1;
                None
            } else {
                match mean_and_sd(&history) {
                    None => {
                        skipped_zero_sd += 1;
                        None
                    }
                    Some((mean, sd)) => {
                        let z = z_for_confidence(confidence).expect("stated confidence is usable");
                        Some((mean, mean - z * sd, mean + z * sd, confidence))
                    }
                }
            };

            if let Some((expected, low, high, conf)) = forecast {
                store
                    .record_expectation(
                        &experiment.id,
                        "modularity",
                        expected,
                        low,
                        high,
                        conf,
                        &[],
                    )
                    .unwrap_or_else(|e| panic!("{name} run {run}: recording forecast failed: {e}"));
                recorded += 1;
            }

            // Only now does the experiment run. Everything above had to
            // happen first or the forecast would be a postdiction.
            let q = measure(&graph, seed);
            store
                .record_metric(&experiment.id, "modularity", MetricValue::Number(q))
                .unwrap();
            history.push(q);

            // A miss is a candidate for the gate. Surprise alone does
            // not admit an anomaly: the run has to be repeated under the
            // same conditions first, which is phase two.
            if let Some((_, low, high, _)) = forecast {
                if q < low || q > high {
                    misses.push((experiment.id.clone(), *name, gi, run));
                }
            }
        }

        let mean = history.iter().sum::<f64>() / history.len() as f64;
        let lo = history.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = history.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        println!(
            "{name:>9}: n={} mean Q={mean:.4} min={lo:.4} max={hi:.4}",
            history.len()
        );
    }

    println!(
        "\nforecasts recorded: {recorded}  skipped (no history): {skipped_no_history}  \
         skipped (zero spread): {skipped_zero_sd}"
    );

    // ---- the calibration report, over a record that was really produced ----
    let report = store.calibration_report().unwrap();
    println!("\n=== calibration report ===");
    println!(
        "n = {}  covered = {}  observed coverage = {:?}",
        report.n,
        report.covered,
        report.observed_coverage()
    );
    println!("mean interval width = {:?}", report.mean_interval_width);
    for band in &report.bands {
        println!(
            "  stated {:.2}: n={:>3} covered={:>3} observed={:.4} mean width={:.4}",
            band.confidence, band.n, band.covered, band.observed_coverage, band.mean_interval_width
        );
    }
    println!("curve: {:?}", report.curve());

    // ---- the go/no-go, against a tolerance stated here ----
    // The design refuses to fix this number and says it should be set
    // from the first observed curve. This IS the first observed curve,
    // so the number below is a starting proposal and nothing more. It
    // is printed next to the verdict so a reader can disagree with it
    // without re-running anything.
    for t in [0.05, 0.10, 0.20] {
        let verdict = report.verdict(Tolerance::new(t).unwrap());
        println!("\ntolerance {t:.2} -> go: {}", verdict.is_go());
        for reason in verdict.reasons() {
            println!("    {reason:?}");
        }
    }

    // ---- phase two: the re-run gate, over the real misses ----
    //
    // Every miss above is a deviation. The gate is what decides whether
    // it was a phenomenon or noise, by repeating the run under the same
    // recorded conditions. On a stochastic search most misses should
    // come back Transient or Volatile, and a Reproduced one would be
    // the interesting result.
    println!("\n=== re-run gate over {} misses ===", misses.len());
    let mut outcomes: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for (original_id, name, gi, run) in &misses {
        let graph = load(name);
        let mut repeat_ids = Vec::new();
        for r in 0..3u64 {
            let repeat = store.create_experiment("erbga", "prereg").unwrap();
            // The same conditions, or the gate refuses: a repeat under
            // different conditions is a different experiment.
            for (key, value) in [
                ("graph", MetricValue::Text(name.to_string())),
                ("vertices", MetricValue::Number(GRAPHS[*gi].1 as f64)),
                ("edges", MetricValue::Number(GRAPHS[*gi].2 as f64)),
                ("islands", MetricValue::Number(3.0)),
                ("generations", MetricValue::Number(300.0)),
                ("population_size", MetricValue::Number(60.0)),
            ] {
                store.record_condition(&repeat.id, key, &value).unwrap();
            }
            // Seeds disjoint from phase one, so a repeat is a genuinely
            // fresh run and not a replay of the same search.
            let seed = 900_000_000 + (*gi as u64) * 10_000 + (*run as u64) * 10 + r;
            let q = measure(&graph, seed);
            store
                .record_metric(&repeat.id, "modularity", MetricValue::Number(q))
                .unwrap();
            repeat_ids.push(repeat.id);
        }

        let refs: Vec<&str> = repeat_ids.iter().map(|s| s.as_str()).collect();
        let verdict = store.rerun_gate(original_id, "modularity", &refs).unwrap();
        *outcomes
            .entry(verdict.outcome.as_str().to_string())
            .or_default() += 1;
        store.record_gate_verdict(&verdict).unwrap();
    }

    println!("gate outcomes: {outcomes:?}");
    let ledger = store.anomalies_for_track("erbga").unwrap();
    println!("anomalies admitted to the ledger: {}", ledger.len());
    let noise = store.noise_report().unwrap();
    println!(
        "noise report: total={} admitted={} noise_rate={:?}",
        noise.total(),
        noise.admitted(),
        noise.noise_rate()
    );
    for a in ledger.iter().take(5) {
        println!(
            "  {} {} observed={:.4} expected={:.4} surprise={:.2} sigma [{}]",
            a.metric_key,
            a.gate_outcome.as_str(),
            a.observed_value,
            a.expected_value,
            a.surprise_sigma,
            a.status.as_str()
        );
    }

    // ---- anomaly families, over whatever the gate admitted ----
    if !ledger.is_empty() {
        let families = store.anomaly_families("erbga", 2).unwrap();
        println!(
            "\n=== families === considered={} discarded_as_unstable={} families={}",
            families.anomalies_considered,
            families.discarded_as_unstable,
            families.families.len()
        );
        for f in &families.families {
            println!(
                "  metric={} direction={:?} members={} band={:.2}..{:.2}",
                f.metric_key,
                f.direction,
                f.members.len(),
                f.band.low,
                f.band.high
            );
        }
    }

    // ---- the boredom detectors, over the same real record ----
    println!("\n=== boredom findings ===");
    let findings = store.boredom_findings().unwrap();
    if findings.is_empty() {
        println!("  (none)");
    }
    for f in &findings {
        println!(
            "  {} on {}: {:?} (support {})",
            f.detector, f.subject, f.invariant_value, f.support
        );
    }

    // ---- the candidates a reader would actually be handed ----
    println!("\n=== inquiry candidates ===");
    for c in store.boredom_candidates().unwrap() {
        println!("---\n{}", c.brief());
    }

    // The corpus has to be big enough for the question to be
    // answerable, or this test proves nothing about the engine.
    assert!(
        report.n >= 50,
        "corpus too small to answer the calibration question: n={}",
        report.n
    );
}
