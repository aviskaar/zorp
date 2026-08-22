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
//! **Every sampled directory prints one line and the totals add up.**
//! Each attempt is either scored or discarded into one named category,
//! `Tally` counts and prints in the same call so neither can happen
//! without the other, and the run asserts that the categories sum to
//! the sample before it prints a calibration report. The 60-directory
//! registry run reported 25 discards and only 19 of them said the word
//! the summary used, so six looked lost when they were not. A number
//! that cannot be checked against the lines above it is a number nobody
//! can act on.
//!
//! Discarding is never a way to improve the pass rate. An attempt that
//! could not be read contributes nothing to `n`, nothing here invents
//! an interval, and `a_discarded_attempt_never_becomes_a_scored_one`
//! says so.
//!
//! Compiles to nothing outside the `research` feature, like every
//! other test here that reaches for `investigate` or `zorp-track`.
#![cfg(feature = "research")]

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use zorp_agent::investigate::forecast::{parse_forecast, Forecast, ForecastError};
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

/// The first word of every per attempt line, scored or discarded, so
/// one grep finds the whole sample.
const ATTEMPT_PREFIX: &str = "attempt ";

/// How much of a model's answer goes next to a discard reason.
const RAW_EXCERPT_CHARS: usize = 400;

/// Why one sampled directory did not produce a scored forecast.
///
/// Every discard lands in exactly one of these and every one of them is
/// printed in the summary, zero included. Add a variant and `index` and
/// `label` stop compiling until you handle it; add it to `ALL` too, and
/// `discard_categories_are_distinct_and_all_of_them_are_listed` says so
/// if you forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Discard {
    /// The agent used its whole step budget without answering.
    StepLimit,
    /// The run ended in an error. A dropped connection to the model
    /// endpoint is the common one.
    AgentError,
    /// The run stopped early for some other reason: cancelled, blocked
    /// on approvals, looping, or verification giving up.
    AgentStopped,
    /// The answer carried no fenced block. Fixed by PR #86, kept so the
    /// count is visible when it is zero.
    NoFencedBlock,
    /// The block was not JSON, or not the shape asked for. Also fixed
    /// by PR #86, also kept.
    NotTheShapeAsked,
    /// A number was missing, not a number, or not finite.
    MissingNumber,
    /// The interval does not contain its own expected value, or the
    /// confidence is not a probability.
    IncoherentForecast,
    /// The store would not take the expectation.
    StoreRefused,
}

impl Discard {
    const COUNT: usize = 8;

    const ALL: [Discard; Self::COUNT] = [
        Discard::StepLimit,
        Discard::AgentError,
        Discard::AgentStopped,
        Discard::NoFencedBlock,
        Discard::NotTheShapeAsked,
        Discard::MissingNumber,
        Discard::IncoherentForecast,
        Discard::StoreRefused,
    ];

    fn index(self) -> usize {
        match self {
            Discard::StepLimit => 0,
            Discard::AgentError => 1,
            Discard::AgentStopped => 2,
            Discard::NoFencedBlock => 3,
            Discard::NotTheShapeAsked => 4,
            Discard::MissingNumber => 5,
            Discard::IncoherentForecast => 6,
            Discard::StoreRefused => 7,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Discard::StepLimit => "step limit reached",
            Discard::AgentError => "agent error",
            Discard::AgentStopped => "agent stopped early",
            Discard::NoFencedBlock => "no fenced json block",
            Discard::NotTheShapeAsked => "not the shape asked for",
            Discard::MissingNumber => "missing or non finite number",
            Discard::IncoherentForecast => "incoherent forecast",
            Discard::StoreRefused => "store refused the expectation",
        }
    }
}

/// What happened to every sampled directory.
///
/// Counting and printing are the same call, which is the whole design.
/// The old harness had three discard paths that each incremented one
/// counter and printed one of three different phrasings, and a summary
/// that used the word from only one of them. Six attempts out of sixty
/// were reported but unfindable, because the line that reported them
/// did not contain the word the summary used.
struct Tally {
    sampled: usize,
    scored: usize,
    discarded: [usize; Discard::COUNT],
    lines: Vec<String>,
}

impl Tally {
    fn new(sampled: usize) -> Self {
        Self {
            sampled,
            scored: 0,
            discarded: [0; Discard::COUNT],
            lines: Vec::new(),
        }
    }

    fn sampled(&self) -> usize {
        self.sampled
    }

    fn scored(&self) -> usize {
        self.scored
    }

    fn count(&self, why: Discard) -> usize {
        self.discarded[why.index()]
    }

    fn total_discarded(&self) -> usize {
        self.discarded.iter().sum()
    }

    fn recorded(&self) -> usize {
        self.scored + self.total_discarded()
    }

    fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Every sampled directory accounted for, exactly once.
    fn reconciles(&self) -> bool {
        self.recorded() == self.sampled && self.lines.len() == self.sampled
    }

    fn record_scored(&mut self, subject: &str, detail: &str) {
        let line = format!("{} scored    {detail}", self.prefix(subject));
        self.scored += 1;
        self.emit(line);
    }

    fn record_discard(&mut self, subject: &str, why: Discard, detail: &str) {
        let line = format!(
            "{} discarded [{}] {detail}",
            self.prefix(subject),
            why.label()
        );
        self.discarded[why.index()] += 1;
        self.emit(line);
    }

    fn prefix(&self, subject: &str) -> String {
        format!(
            "{ATTEMPT_PREFIX}{:>3}/{:<3} {subject:<44}",
            self.recorded() + 1,
            self.sampled
        )
    }

    fn emit(&mut self, line: String) {
        println!("{line}");
        self.lines.push(line);
    }

    fn summary(&self) -> Vec<String> {
        let mut out = vec![
            "=== attempts ===".to_string(),
            format!("sampled   {:>4}", self.sampled),
            format!("scored    {:>4}", self.scored),
            format!("discarded {:>4}", self.total_discarded()),
        ];
        for why in Discard::ALL {
            out.push(format!("  {:<32}{:>4}", why.label(), self.count(why)));
        }
        out
    }
}

/// The agent's answer, or the category its stop reason belongs to.
///
/// Exhaustive on purpose. A new `Outcome` variant should be a compile
/// error here, not a seventh way to leave the loop unreported.
fn classify_answer(outcome: Outcome) -> Result<String, (Discard, String)> {
    let detail = outcome.describe();
    match outcome {
        Outcome::Complete(text) => Ok(text),
        Outcome::StepLimit => Err((Discard::StepLimit, detail)),
        Outcome::Error(_) => Err((Discard::AgentError, detail)),
        Outcome::Cancelled
        | Outcome::RepeatedAction
        | Outcome::Blocked
        | Outcome::VerificationFailed { .. } => Err((Discard::AgentStopped, detail)),
    }
}

fn classify_forecast_error(e: &ForecastError) -> Discard {
    match e {
        ForecastError::NoFencedBlock => Discard::NoFencedBlock,
        ForecastError::InvalidJson(_) => Discard::NotTheShapeAsked,
        ForecastError::BadNumber(_) => Discard::MissingNumber,
        ForecastError::Incoherent(_) => Discard::IncoherentForecast,
    }
}

/// A one line, length capped view of a model's answer, for the log.
///
/// The two parser bugs PR #86 fixed could not be replayed, because the
/// harness recorded the error and threw the text away. This puts the
/// offending text next to the reason so the next one is diagnosable.
///
/// The tail rather than the head: the forecast is asked for last, so
/// that is where the failure is.
///
/// This is model authored text. It goes to stdout and nowhere else. It
/// is never written to the store, so no detector and nothing in the
/// search layer can read it back, which is the rule in CLAUDE.md. It
/// returns a `String` to print, not something a caller can record.
fn excerpt(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let total = flat.chars().count();
    if total <= max_chars {
        return flat;
    }
    let tail: String = flat.chars().skip(total - max_chars).collect();
    format!("[{total} chars, last {max_chars}] ...{tail}")
}

/// One attempt, from the agent's outcome to exactly one tally line.
///
/// The store write is injected so the accounting can be driven end to
/// end by a test with no model and no database. Returns whether the
/// interval covered the truth and how wide it was, when the attempt
/// scored. `None` means it was discarded, and the discard is already
/// counted and printed by the time this returns.
fn record_attempt(
    tally: &mut Tally,
    subject: &str,
    truth: f64,
    outcome: Outcome,
    store_write: impl FnOnce(&Forecast) -> Result<(), String>,
) -> Option<(bool, f64)> {
    let answer = match classify_answer(outcome) {
        Ok(text) => text,
        Err((why, detail)) => {
            tally.record_discard(subject, why, &detail);
            return None;
        }
    };

    let forecast = match parse_forecast(&answer) {
        Ok(f) => f,
        Err(e) => {
            // The raw answer goes next to the reason. Without it a
            // parser bug can only be chased by running the whole thing
            // again, which is what PR #86 had to do.
            tally.record_discard(
                subject,
                classify_forecast_error(&e),
                &format!("{e}; raw {:?}", excerpt(&answer, RAW_EXCERPT_CHARS)),
            );
            return None;
        }
    };

    if let Err(e) = store_write(&forecast) {
        tally.record_discard(
            subject,
            Discard::StoreRefused,
            &format!(
                "{e}; said {} in [{}, {}] @{}",
                forecast.expected_value,
                forecast.interval_low,
                forecast.interval_high,
                forecast.confidence
            ),
        );
        return None;
    }

    let hit = truth >= forecast.interval_low && truth <= forecast.interval_high;
    let width = forecast.interval_high - forecast.interval_low;
    tally.record_scored(
        subject,
        &format!(
            "said {:>8.0} in [{:>8.0},{:>8.0}] @{:.2} width {:>7.0}  truth {:>6.0}  {}",
            forecast.expected_value,
            forecast.interval_low,
            forecast.interval_high,
            forecast.confidence,
            width,
            truth,
            if hit { "hit" } else { "MISS" }
        ),
    );
    Some((hit, width))
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

    let mut tally = Tally::new(targets.len());
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

        let t = *truth as f64;
        let scored = record_attempt(&mut tally, rel, t, agent.run(&prompt(rel)), |f| {
            // The expectation before the outcome, always. A forecast
            // recorded after the metric is a postdiction, and the store
            // refuses it, which is the whole point of the guard.
            store
                .record_expectation(
                    &experiment.id,
                    "line_total",
                    f.expected_value,
                    f.interval_low,
                    f.interval_high,
                    f.confidence,
                    &[],
                )
                .map_err(|e| e.to_string())?;
            store
                .record_metric(&experiment.id, "line_total", MetricValue::Number(t))
                .unwrap();
            Ok(())
        });

        if let Some((hit, width)) = scored {
            if hit {
                widths_when_hit.push(width);
            } else {
                widths_when_missed.push(width);
            }
        }
    }

    println!();
    for line in tally.summary() {
        println!("{line}");
    }

    // The point of the change. A summary that does not add up is a
    // summary nobody can act on, and the run below it inherits the
    // doubt. Fail here rather than print a calibration report built on
    // a sample of unknown size.
    assert!(
        tally.reconciles(),
        "attempts do not reconcile: sampled {}, scored {}, discarded {}, lines {}",
        targets.len(),
        tally.scored(),
        tally.total_discarded(),
        tally.lines().len()
    );

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
            "  mean stated {:.4}: n={:>3} covered={:>3} observed={:.4} mean width={:.0}",
            band.confidence, band.n, band.covered, band.observed_coverage, band.mean_interval_width
        );
        // What got pooled into that mean. Printed because pooling
        // averages, and an average of two opposite errors reads as no
        // error at all unless both halves are on the page.
        for part in &band.parts {
            println!(
                "      stated {:.4}: n={:>3} covered={:>3}",
                part.confidence, part.n, part.covered
            );
        }
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

// ---------------------------------------------------------------------
// The accounting, checked without a model.
//
// The harness above is `#[ignore]`d because it calls one, so nothing it
// asserts runs in CI. These do run, and the first is the point of the
// file: every sampled directory lands in exactly one bucket, and the
// buckets add up to the sample.
// ---------------------------------------------------------------------

#[test]
fn every_sampled_attempt_produces_exactly_one_line_and_the_totals_reconcile() {
    let sampled = 1 + Discard::ALL.len();
    let mut tally = Tally::new(sampled);

    tally.record_scored("a/scored", "said 10 truth 10 hit");
    assert_eq!(tally.lines().len(), 1);

    for (i, why) in Discard::ALL.iter().enumerate() {
        tally.record_discard(&format!("a/discard{i}"), *why, "because");
        assert_eq!(
            tally.lines().len(),
            i + 2,
            "{} was counted without printing a line",
            why.label()
        );
    }

    assert_eq!(tally.scored(), 1);
    assert_eq!(tally.total_discarded(), Discard::ALL.len());
    assert_eq!(tally.scored() + tally.total_discarded(), sampled);
    assert_eq!(tally.lines().len(), sampled);
    assert!(tally.reconciles());
}

/// The defect this change exists to stop. Two attempts accounted for
/// out of three is not a summary, it is a guess.
#[test]
fn a_missing_attempt_does_not_reconcile() {
    let mut tally = Tally::new(3);
    tally.record_scored("a", "said 10 truth 10 hit");
    tally.record_discard("b", Discard::StepLimit, "ran out");

    assert!(
        !tally.reconciles(),
        "two attempts out of three sampled must not reconcile"
    );
}

#[test]
fn the_summary_reports_every_category_including_the_ones_at_zero() {
    let mut tally = Tally::new(1);
    tally.record_discard("a", Discard::StepLimit, "ran out");
    let summary = tally.summary().join("\n");

    for why in Discard::ALL {
        assert!(
            summary.contains(why.label()),
            "category {} is missing from the summary:\n{summary}",
            why.label()
        );
    }
    assert!(summary.contains("sampled"));
    assert!(summary.contains("scored"));
    assert!(summary.contains("discarded"));
}

/// PR #86 fixed both parser reasons, so their counts should fall. A
/// category that vanishes when it stops firing is one nobody can check
/// the arithmetic against, so they stay and report zero.
#[test]
fn the_two_reasons_fixed_by_86_report_zero_rather_than_disappearing() {
    let tally = Tally::new(0);
    let summary = tally.summary().join("\n");

    assert!(summary.contains(Discard::NoFencedBlock.label()));
    assert!(summary.contains(Discard::NotTheShapeAsked.label()));
    assert_eq!(tally.count(Discard::NoFencedBlock), 0);
    assert_eq!(tally.count(Discard::NotTheShapeAsked), 0);
}

#[test]
fn every_agent_outcome_that_is_not_an_answer_lands_in_a_category() {
    assert_eq!(
        classify_answer(Outcome::Complete("hi".into())).unwrap(),
        "hi"
    );

    let cases: Vec<(Outcome, Discard)> = vec![
        (Outcome::StepLimit, Discard::StepLimit),
        (Outcome::Error("boom".into()), Discard::AgentError),
        (Outcome::Cancelled, Discard::AgentStopped),
        (Outcome::RepeatedAction, Discard::AgentStopped),
        (Outcome::Blocked, Discard::AgentStopped),
        (
            Outcome::VerificationFailed { attempts: 3 },
            Discard::AgentStopped,
        ),
    ];

    for (outcome, want) in cases {
        let described = outcome.describe();
        let (got, detail) = classify_answer(outcome).unwrap_err();
        assert_eq!(got, want, "{described} landed in the wrong category");
        assert!(
            !detail.is_empty(),
            "{described} was discarded with no reason"
        );
    }
}

/// Five of the six attempts nobody could find in the 60-directory
/// registry run were this: the model endpoint dropped the connection
/// mid-run and the agent returned `Outcome::Error`.
#[test]
fn a_dropped_connection_is_reported_as_an_agent_error() {
    let msg = "https://openrouter.ai/api/v1/chat/completions: Network Error: \
               Connection reset by peer (os error 54)";
    let (why, detail) = classify_answer(Outcome::Error(msg.into())).unwrap_err();

    assert_eq!(why, Discard::AgentError);
    assert!(detail.contains("Connection reset by peer"), "{detail}");
}

#[test]
fn every_forecast_error_lands_in_a_category() {
    let cases = [
        (ForecastError::NoFencedBlock, Discard::NoFencedBlock),
        (
            ForecastError::InvalidJson("EOF while parsing a value at line 1 column 0".into()),
            Discard::NotTheShapeAsked,
        ),
        (
            ForecastError::BadNumber("confidence"),
            Discard::MissingNumber,
        ),
        (
            ForecastError::Incoherent("interval_low 9 is above interval_high 7".into()),
            Discard::IncoherentForecast,
        ),
    ];

    for (e, want) in cases {
        assert_eq!(classify_forecast_error(&e), want, "{e}");
    }
}

#[test]
fn discard_categories_are_distinct_and_all_of_them_are_listed() {
    assert_eq!(Discard::ALL.len(), Discard::COUNT);
    for (i, why) in Discard::ALL.iter().enumerate() {
        assert_eq!(
            why.index(),
            i,
            "Discard::ALL is out of order or missing an entry"
        );
    }

    let mut labels: Vec<&str> = Discard::ALL.iter().map(|d| d.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), Discard::COUNT, "two categories share a label");
}

#[test]
fn a_discard_line_names_the_subject_the_category_and_the_reason() {
    let mut tally = Tally::new(1);
    tally.record_discard(
        "crates/foo/src",
        Discard::NoFencedBlock,
        "raw \"I think about 400 lines\"",
    );
    let line = &tally.lines()[0];

    assert!(line.contains("discarded"), "{line}");
    assert!(line.contains("crates/foo/src"), "{line}");
    assert!(line.contains(Discard::NoFencedBlock.label()), "{line}");
    assert!(line.contains("I think about 400 lines"), "{line}");
}

/// One grep has to find all of them. The 60-directory run printed three
/// different words for three discard paths and a summary that used only
/// one of them, so a grep for the summary's word found 19 of 25.
#[test]
fn one_grep_finds_every_attempt_line() {
    let mut tally = Tally::new(2);
    tally.record_scored("a", "said 1 truth 1 hit");
    tally.record_discard("b", Discard::StepLimit, "ran out");

    assert_eq!(
        tally
            .lines()
            .iter()
            .filter(|l| l.starts_with(ATTEMPT_PREFIX))
            .count(),
        2
    );
}

fn a_good_answer() -> String {
    "I counted them.\n\n```json\n{\"expected_value\": 10, \"interval_low\": 5, \
     \"interval_high\": 15, \"confidence\": 0.8}\n```\n"
        .to_string()
}

/// The 60-directory registry run, replayed through the same code the
/// harness runs, with no model and no store.
///
/// 35 scored and 25 discarded were the real numbers. Nineteen of the
/// discards printed the word the summary used and six did not, so a
/// grep for that word found 19 of 25 and the other six looked like
/// silent losses. They were one step limit and five dropped
/// connections to the model endpoint.
#[test]
fn the_registry_run_that_seemed_to_lose_six_attempts_reconciles() {
    let mut outcomes: Vec<Outcome> = Vec::new();
    for _ in 0..35 {
        outcomes.push(Outcome::Complete(a_good_answer()));
    }
    // Eleven answers whose last fence was empty. Before PR #86 that
    // threw the real forecast away and serde reported "EOF while
    // parsing a value at line 1 column 0".
    for _ in 0..11 {
        outcomes.push(Outcome::Complete("about 400 lines\n\n```\n```\n".into()));
    }
    // Eight answers with no fenced block at all.
    for _ in 0..8 {
        outcomes.push(Outcome::Complete("I could not count them.".into()));
    }
    // And the six.
    outcomes.push(Outcome::StepLimit);
    for _ in 0..5 {
        outcomes.push(Outcome::Error(
            "https://openrouter.ai/api/v1/chat/completions: Network Error: \
             Connection reset by peer (os error 54)"
                .into(),
        ));
    }

    let mut tally = Tally::new(outcomes.len());
    for (i, outcome) in outcomes.into_iter().enumerate() {
        let _ = record_attempt(&mut tally, &format!("dir{i}"), 10.0, outcome, |_| Ok(()));
    }

    assert_eq!(tally.sampled(), 60);
    assert_eq!(tally.scored(), 35);
    assert_eq!(tally.total_discarded(), 25);
    assert_eq!(tally.count(Discard::NotTheShapeAsked), 11);
    assert_eq!(tally.count(Discard::NoFencedBlock), 8);
    assert_eq!(tally.count(Discard::StepLimit), 1);
    assert_eq!(tally.count(Discard::AgentError), 5);
    assert_eq!(tally.lines().len(), 60);
    assert!(tally.reconciles());
}

#[test]
fn a_store_refusal_is_counted_and_printed_like_any_other_discard() {
    let mut tally = Tally::new(1);
    let scored = record_attempt(
        &mut tally,
        "dir",
        10.0,
        Outcome::Complete(a_good_answer()),
        |_| Err("outcome already exists for line_total".into()),
    );

    assert!(scored.is_none(), "a refused expectation must not score");
    assert_eq!(tally.scored(), 0);
    assert_eq!(tally.count(Discard::StoreRefused), 1);
    assert!(tally.reconciles());
    assert!(
        tally.lines()[0].contains("outcome already exists"),
        "{}",
        tally.lines()[0]
    );
}

/// The cheap half of this change. A discard that only records its
/// reason cannot be replayed, which is why the two bugs PR #86 fixed
/// needed a whole rerun to find.
#[test]
fn a_parse_discard_carries_the_text_that_could_not_be_parsed() {
    let mut tally = Tally::new(1);
    let _ = record_attempt(
        &mut tally,
        "dir",
        10.0,
        Outcome::Complete("I reckon 412 lines,\ngive or take.".into()),
        |_| Ok(()),
    );
    let line = &tally.lines()[0];

    assert!(line.contains("I reckon 412 lines, give or take."), "{line}");
    assert!(line.contains(Discard::NoFencedBlock.label()), "{line}");
}

/// The constraint that outranks the reporting. A discard must never
/// become a score, so nothing here substitutes a default interval and
/// an unreadable answer contributes nothing to `n`.
#[test]
fn a_discarded_attempt_never_becomes_a_scored_one() {
    let mut tally = Tally::new(4);
    for outcome in [
        Outcome::StepLimit,
        Outcome::Error("boom".into()),
        Outcome::Complete("no block here".into()),
        Outcome::Complete("```json\n{\"expected_value\": 1.0}\n```".into()),
    ] {
        let scored = record_attempt(&mut tally, "dir", 10.0, outcome, |_| Ok(()));
        assert!(scored.is_none());
    }

    assert_eq!(tally.scored(), 0);
    assert_eq!(tally.total_discarded(), 4);
    assert!(tally.reconciles());
}

#[test]
fn a_raw_excerpt_is_one_line() {
    let got = excerpt("first\nsecond\tthird\r\n", 100);

    assert_eq!(got, "first second third");
    assert!(!got.contains('\n'));
}

#[test]
fn a_raw_excerpt_keeps_the_tail_and_says_how_much_it_dropped() {
    let text = format!("{}TAILEND", "x".repeat(500));
    let got = excerpt(&text, 10);

    assert!(got.ends_with("TAILEND"), "{got}");
    assert!(
        got.contains("507"),
        "the excerpt must state the true length: {got}"
    );
    assert!(!got.contains('\n'));
}
