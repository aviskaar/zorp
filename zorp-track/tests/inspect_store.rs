//! Read back what a real run recorded.
//!
//! A read-only dump of an existing store, for answering "did that
//! actually record anything?" after running the binary by hand. It
//! opens the path in `ZORP_INSPECT_STORE` and prints; it writes
//! nothing, which is the same rule the calibration report follows and
//! for the same reason.
//!
//!   ZORP_INSPECT_STORE=/path/to/.zorp/zorp.duckdb \
//!     cargo test -p zorp-track --test inspect_store -- --ignored --nocapture

use zorp_track::calibration::Tolerance;
use zorp_track::track::Store;

#[test]
#[ignore = "needs ZORP_INSPECT_STORE pointing at a real store"]
fn inspect() {
    let path = std::env::var("ZORP_INSPECT_STORE")
        .expect("set ZORP_INSPECT_STORE to a .zorp/zorp.duckdb path");
    let store = Store::open(std::path::Path::new(&path)).unwrap();

    let tracks = store.list_tracks().unwrap();
    println!("tracks: {}", tracks.len());
    for t in &tracks {
        println!("  {} [{:?}]", t.id, t.status);

        let experiments = store.experiments_for(&t.id).unwrap();
        println!("  experiments: {}", experiments.len());
        for e in &experiments {
            let conditions = store.conditions_for(&e.id).unwrap();
            let expectations = store.expectations_for(&e.id).unwrap();
            let metrics = store.metrics_for(&e.id).unwrap();
            println!(
                "    {} [{:?}] conditions={} expectations={} metrics={}",
                e.id,
                e.status,
                conditions.len(),
                expectations.len(),
                metrics.len()
            );
            for c in &conditions {
                println!("      condition {} = {:?}", c.condition_key, c.value);
            }
            for x in &expectations {
                println!(
                    "      forecast {} = {} in [{}, {}] at {}",
                    x.metric_key, x.expected_value, x.interval_low, x.interval_high, x.confidence
                );
            }
            for (k, v) in &metrics {
                println!("      outcome {k} = {v:?}");
            }
        }
    }

    let report = store.calibration_report().unwrap();
    println!(
        "\ncalibration: n={} covered={} coverage={:?} mean_width={:?}",
        report.n,
        report.covered,
        report.observed_coverage(),
        report.mean_interval_width
    );
    for band in &report.bands {
        println!(
            "  mean stated {:.4}: n={} covered={} observed={:.4} width={:.4}",
            band.confidence, band.n, band.covered, band.observed_coverage, band.mean_interval_width
        );
        // What got pooled into that mean. Printed because pooling
        // averages, and an average of two opposite errors reads as no
        // error at all unless both halves are on the page.
        for part in &band.parts {
            println!(
                "      stated {:.4}: n={} covered={}",
                part.confidence, part.n, part.covered
            );
        }
    }
    let verdict = report.verdict(Tolerance::new(0.10).unwrap());
    println!("verdict at tolerance 0.10 -> go: {}", verdict.is_go());
    for r in verdict.reasons() {
        println!("  {r:?}");
    }

    println!("\nboredom findings:");
    for f in store.boredom_findings().unwrap() {
        println!(
            "  {} on {}: {:?} (support {})",
            f.detector, f.subject, f.invariant_value, f.support
        );
    }
}
