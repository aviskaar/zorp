//! Calibration when the forecaster is allowed to look.
//!
//! The blind version of this measures whether a model's prior is any
//! good. That is the FermiEval question and it is not the question zorp
//! runs in: a zorp agent has tools, and the interesting quantity is
//! whether a forecaster that *can* gather evidence knows whether it
//! did.
//!
//! A well calibrated agent with read-only tools should have close to
//! perfect coverage, because it can simply go and count, and its
//! intervals should be narrow when it counted and wide when it did not
//! bother. A badly calibrated one states a narrow interval either way.
//! Coverage against interval width is what separates those, and both
//! are in the report.
//!
//! **Why this does not contradict `forecast::ask` having no tools.**
//! There, the forecast is about the outcome of an experiment that has
//! not run yet, and a tool-using forecaster can go and read the
//! previous run's number, which is measurement wearing a prediction's
//! clothes. Here the quantity is a static property of the repository
//! that exists before and after the forecast, so looking it up *is* the
//! evidence gathering under test rather than a way around it. The
//! difference is whether the answer exists yet.
//!
//! The task is aggregate rather than single file on purpose. One file's
//! line count is one `read_file` away, so a forecast about it measures
//! tool use and nothing else. A directory total needs several reads and
//! an addition, so a lazy forecaster and a diligent one give visibly
//! different answers and the report can tell them apart.
//!
//! Ground truth is computed here and never shown to the model.
//!
//!   ZORP_BASE_URL=http://localhost:11434/v1 ZORP_MODEL=qwen3.8:27b \
//!     cargo test -p zorp-agent --release --features research \
//!       --test evidence_calibration -- --ignored --nocapture
//!
//! `ZORP_CAL_N` caps how many directories are asked about.
//! `ZORP_CAL_STEPS` caps agent steps per forecast, default 12.
//!
//! Compiles to nothing outside the `research` feature, like every
//! other test here that reaches for `investigate` or `zorp-track`.
#![cfg(feature = "research")]

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use zorp_agent::investigate::forecast::parse_forecast;
use zorp_agent::{reviewer_tools, Agent, ApprovalMode, HttpModel, Outcome};
use zorp_track::calibration::Tolerance;
use zorp_track::experiment::MetricValue;
use zorp_track::track::Store;

const SYSTEM_PROMPT: &str = "\
You estimate a quantity and state how sure you are. You have read-only \
tools and you are expected to use them: looking is cheaper than \
guessing, and you are being scored on whether your interval contains \
the truth.\n\n\
Your interval is a claim you will be held to. An interval so wide it \
cannot be wrong scores as badly as one so narrow it always is, because \
width is recorded next to coverage. If you counted, say so with a \
narrow interval. If you ran out of steps and had to guess, say that \
with a wide one.";

fn prompt(dir: &str) -> String {
    format!(
        "How many lines of Rust are there in total across every `.rs` file \
         directly inside the directory `{dir}` of this repository, not \
         counting subdirectories?\n\n\
         Use your tools to find out. When you are done, end your answer \
         with a single fenced JSON block, exactly this shape:\n\
         ```json\n\
         {{\"expected_value\": <number>, \"interval_low\": <number>, \
         \"interval_high\": <number>, \"confidence\": <number between 0 and 1>}}\n\
         ```\n\n\
         `confidence` is the probability you assign to the true total \
         landing inside your interval. Do not write anything after the \
         block."
    )
}

/// Directories holding at least two `.rs` files, with the true total
/// for each. Two, because a single file total is one `read_file` away
/// and would measure tool use rather than judgement.
fn subjects(root: &std::path::Path, want: usize) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();

        let mut total = 0usize;
        let mut files = 0usize;
        for path in &paths {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                dirs.push(path.clone());
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(path) {
                    total += text.lines().count();
                    files += 1;
                }
            }
        }
        if files >= 2 {
            let rel = dir
                .strip_prefix(root)
                .unwrap_or(&dir)
                .to_string_lossy()
                .to_string();
            if !rel.is_empty() {
                out.push((rel, total));
            }
        }
    }
    out.sort();
    if out.len() <= want {
        return out;
    }
    // Every nth, not the first n. Sorted paths mean truncation would take
    // an alphabetical prefix, which on a registry checkout is one letter's
    // worth of crates and on any tree is whatever sorts early. Striding
    // spans the corpus and is still deterministic, so the sample is the
    // same on every machine and the run reproduces.
    let stride = out.len() / want;
    let sampled: Vec<(String, usize)> = out.iter().step_by(stride).take(want).cloned().collect();
    println!(
        "corpus: {} eligible directories, sampling every {}th for {}",
        out.len(),
        stride,
        sampled.len()
    );
    sampled
}

#[test]
#[ignore = "needs a model on ZORP_BASE_URL; run with --ignored --release"]
fn a_forecaster_with_tools_is_scored_on_what_it_gathered() {
    let base = std::env::var("ZORP_BASE_URL").expect("set ZORP_BASE_URL");
    let model_name = std::env::var("ZORP_MODEL").expect("set ZORP_MODEL");
    let want: usize = std::env::var("ZORP_CAL_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let max_steps: usize = std::env::var("ZORP_CAL_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);

    // The corpus this repo can offer is 22 directories, and the report
    // needs 50 before it will return anything but NotEnoughEvidence, so
    // the interesting runs point somewhere larger. The cargo registry is
    // the obvious one: thousands of directories of real third-party Rust
    // that nobody here wrote, which is a better corpus than our own code
    // for the same reason a held-out set is better than a training one.
    let root = match std::env::var("ZORP_CAL_ROOT") {
        Ok(dir) if !dir.trim().is_empty() => std::path::PathBuf::from(dir),
        _ => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf(),
    };
    assert!(
        root.is_dir(),
        "ZORP_CAL_ROOT is not a directory: {}",
        root.display()
    );

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
            "evidence",
            "a forecaster that can gather evidence states intervals that match what it gathered",
        )
        .unwrap();

    let targets = subjects(&root, want);
    println!(
        "asking about {} directories, model {model_name}, max_steps {max_steps}\n",
        targets.len()
    );

    let mut unusable = 0usize;
    let mut widths_when_hit: Vec<f64> = Vec::new();
    let mut widths_when_missed: Vec<f64> = Vec::new();

    for (rel, truth) in &targets {
        let experiment = store.create_experiment("evidence", "prereg").unwrap();
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
                "tools",
                &MetricValue::Text("read-only".into()),
            )
            .unwrap();
        store
            .record_condition(
                &experiment.id,
                "max_steps",
                &MetricValue::Number(max_steps as f64),
            )
            .unwrap();

        let mut agent = Agent::new(
            zorp_agent::Model::clone_box(&model),
            SYSTEM_PROMPT,
            max_steps,
            root.clone(),
            Arc::new(AtomicBool::new(false)) as zorp_agent::CancelToken,
            ApprovalMode::AutoApprove,
        )
        .register_builtins_filtered(Some(&reviewer_tools()));

        let answer = match agent.run(&prompt(rel)) {
            Outcome::Complete(text) => text,
            other => {
                unusable += 1;
                println!("  {rel}: no answer ({})", other.describe());
                continue;
            }
        };

        let forecast = match parse_forecast(&answer) {
            Ok(f) => f,
            Err(e) => {
                unusable += 1;
                println!("  {rel}: unusable ({e})");
                continue;
            }
        };

        if let Err(e) = store.record_expectation(
            &experiment.id,
            "line_total",
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

        store
            .record_metric(
                &experiment.id,
                "line_total",
                MetricValue::Number(*truth as f64),
            )
            .unwrap();

        let t = *truth as f64;
        let hit = t >= forecast.interval_low && t <= forecast.interval_high;
        let width = forecast.interval_high - forecast.interval_low;
        if hit {
            widths_when_hit.push(width);
        } else {
            widths_when_missed.push(width);
        }

        println!(
            "  {:<44} said {:>8.0} in [{:>8.0},{:>8.0}] @{:.2} width {:>7.0}  truth {:>6}  {}",
            rel,
            forecast.expected_value,
            forecast.interval_low,
            forecast.interval_high,
            forecast.confidence,
            width,
            truth,
            if hit { "hit" } else { "MISS" }
        );
    }

    println!("\nunusable {unusable}");

    let mean = |v: &[f64]| {
        if v.is_empty() {
            f64::NAN
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    // The question this harness exists for. A forecaster that knows
    // when it looked states a narrower interval on the ones it got
    // right; one that does not shows the same width either way.
    println!(
        "mean interval width when it hit:    {:.0}  (n={})",
        mean(&widths_when_hit),
        widths_when_hit.len()
    );
    println!(
        "mean interval width when it missed: {:.0}  (n={})",
        mean(&widths_when_missed),
        widths_when_missed.len()
    );

    let report = store.calibration_report().unwrap();
    println!("\n=== calibration, forecaster with read-only tools ===");
    println!(
        "n={} covered={} coverage={:?}",
        report.n,
        report.covered,
        report.observed_coverage()
    );
    println!("mean interval width = {:?}", report.mean_interval_width);
    for band in &report.bands {
        println!(
            "  stated {:.2}: n={:>3} covered={:>3} observed={:.4} mean width={:.0}",
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
