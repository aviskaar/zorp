//! Is a local model's stated interval worth anything?
//!
//! This is the endogenous case the design cares about, run for real
//! against a model on this machine. The model states an interval before
//! the answer is known, the answer is then measured **in code**, and
//! the two are scored by the calibration report.
//!
//! The measurement is the important part. `investigate` takes the
//! metric value from the model's own reported JSON, which means the
//! same model both forecasts the number and reports it. That is fine
//! for running an investigation and useless for measuring calibration,
//! because nothing stops the two agreeing. Here the ground truth is
//! computed by this file, from the filesystem, and the model never sees
//! it.
//!
//! The task is deliberately a Fermi estimate: the model is given a file
//! path and asked how many lines are in it, with no tools and no way to
//! read the file. Guessing is the point. A forecaster that knows the
//! answer tells you nothing about its uncertainty, and honest
//! uncertainty is the whole quantity under test.
//!
//! Needs Ollama, or anything else OpenAI compatible, on
//! `ZORP_BASE_URL`. Run with:
//!   ZORP_BASE_URL=http://localhost:11434/v1 ZORP_MODEL=qwen3:4b \
//!     cargo test -p zorp-agent --release --features research \
//!       --test ollama_calibration -- --ignored --nocapture

use zorp_agent::investigate::forecast;
use zorp_agent::HttpModel;
use zorp_track::calibration::Tolerance;
use zorp_track::experiment::MetricValue;
use zorp_track::track::Store;

/// Files to ask about. Chosen by walking the workspace rather than
/// hand-picked, so the set is not curated toward files the model would
/// find easy.
fn subjects(root: &std::path::Path, want: usize) -> Vec<(String, usize)> {
    let mut found: Vec<(String, usize)> = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        // Sorted so the corpus is the same set on every machine.
        names.sort();
        for path in names {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                found.push((rel, text.lines().count()));
            }
        }
    }
    found.sort();
    found.truncate(want);
    found
}

#[test]
#[ignore = "needs a local model on ZORP_BASE_URL; run with --ignored --release"]
fn a_local_models_intervals_are_scored_against_measured_truth() {
    let base = std::env::var("ZORP_BASE_URL").expect("set ZORP_BASE_URL");
    let model_name = std::env::var("ZORP_MODEL").expect("set ZORP_MODEL");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let model = HttpModel {
        url: format!("{}/chat/completions", base.trim_end_matches('/')),
        api_key: std::env::var("ZORP_API_KEY").ok(),
        model: model_name.clone(),
        provider: zorp_agent::Provider::OpenAiCompatible,
        max_tokens: None,
    };

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
    store
        .create_track(
            "fermi",
            "a local model's stated intervals have real coverage",
        )
        .unwrap();

    // Size the corpus from the environment, so a paid endpoint can be
    // smoke tested for a few cents before committing to the full run.
    // Below 50 the verdict is NotEnoughEvidence by design, which is the
    // correct answer and not a reason to raise it.
    let want: usize = std::env::var("ZORP_CAL_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let files = subjects(&root, want);
    println!("asking about {} files, model {model_name}\n", files.len());

    let mut asked = 0usize;
    let mut unusable = 0usize;

    for (rel, truth) in &files {
        let experiment = store.create_experiment("fermi", "prereg").unwrap();
        store
            .record_condition(
                &experiment.id,
                "model",
                &MetricValue::Text(model_name.clone()),
            )
            .unwrap();
        store
            .record_condition(
                &experiment.id,
                "task",
                &MetricValue::Text("line_count".into()),
            )
            .unwrap();

        let hypothesis = format!(
            "The Rust source file at path '{rel}' in the zorp workspace has some number of lines. \
             You cannot open it. Estimate from the path alone."
        );

        asked += 1;
        let forecast = match forecast::ask(&model, &hypothesis, "line_count", root.clone()) {
            Ok(Some(f)) => f,
            Ok(None) => {
                unusable += 1;
                println!("  {rel}: no answer");
                continue;
            }
            Err(e) => {
                unusable += 1;
                println!("  {rel}: unusable ({e})");
                continue;
            }
        };

        // Recorded before the outcome, which is what makes the guard in
        // `record_expectation` meaningful rather than decorative.
        if let Err(e) = store.record_expectation(
            &experiment.id,
            "line_count",
            forecast.expected_value,
            forecast.interval_low,
            forecast.interval_high,
            forecast.confidence,
            &[],
        ) {
            unusable += 1;
            println!("  {rel}: refused ({e})");
            continue;
        }

        // Ground truth, measured here, never shown to the model.
        store
            .record_metric(
                &experiment.id,
                "line_count",
                MetricValue::Number(*truth as f64),
            )
            .unwrap();

        let hit = *truth as f64 >= forecast.interval_low && *truth as f64 <= forecast.interval_high;
        println!(
            "  {:<52} said {:>7.0} in [{:>7.0},{:>7.0}] @{:.2}  truth {:>5}  {}",
            rel,
            forecast.expected_value,
            forecast.interval_low,
            forecast.interval_high,
            forecast.confidence,
            truth,
            if hit { "hit" } else { "MISS" }
        );
    }

    println!("\nasked {asked}, unusable {unusable}");

    let report = store.calibration_report().unwrap();
    println!("\n=== calibration, local model, measured truth ===");
    println!(
        "n={} covered={} coverage={:?}",
        report.n,
        report.covered,
        report.observed_coverage()
    );
    println!("mean interval width = {:?}", report.mean_interval_width);
    for band in &report.bands {
        println!(
            "  stated {:.2}: n={:>3} covered={:>3} observed={:.4} mean width={:.1}",
            band.confidence, band.n, band.covered, band.observed_coverage, band.mean_interval_width
        );
    }
    println!("curve: {:?}", report.curve());

    for t in [0.05, 0.10, 0.20] {
        let verdict = report.verdict(Tolerance::new(t).unwrap());
        println!("\ntolerance {t:.2} -> go: {}", verdict.is_go());
        for r in verdict.reasons() {
            println!("    {r:?}");
        }
    }
}
