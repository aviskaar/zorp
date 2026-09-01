//! The caller the anomaly ledger did not have.
//!
//! aryabhatta's re-run gate has existed since 2026-08-19 and until now
//! nothing outside a test ever ran it. `investigate` recorded conditions,
//! a forecast and an outcome, and stopped, so the `anomalies` table had no
//! producer and no number of attempts would put a row in it
//! (`docs/DECISIONS.md`, 2026-09-01). This is that producer.
//!
//! What it does is repeat the attempt and hand the repeats to
//! `Store::rerun_gate`, which classifies them. Surprise alone admits
//! nothing: most prediction error in a software environment is a changed
//! default or a mis-parsed file, and the ones that reproduce on demand
//! forever are flaky tests. `Transient` and `Volatile` are how those get
//! thrown away, and they are counted rather than discarded so the noise
//! rate is readable afterwards.
//!
//! Three things about this are not negotiable.
//!
//! **It is off by default.** A gated attempt costs one extra full agent
//! run per repeat, so the default of two repeats triples the price of any
//! attempt that surprises its forecast. `ZORP_FORECAST` is off for the
//! same reason and this is the more expensive of the two. A ledger nobody
//! paid for is empty, and empty is the honest state for a record nobody
//! has fed.
//!
//! **No model takes part in the decision.** `classify` is arithmetic and
//! equality, `gate_candidate` is two reads and a comparison, and nothing
//! here asks a model whether something was an anomaly. The model's only
//! job is running the repeat, exactly as it ran the original. This is the
//! same split `critique` uses and the same one integrity rule 5 protects:
//! the agent's own speculation must never become tomorrow's observation.
//!
//! **A repeat starts from the same place the original did.** The
//! transcript is truncated back before each one, so a repeat cannot read
//! what the first attempt concluded. Without that, `Reproduced` would stop
//! meaning "it happened again" and start meaning "it was shown its own
//! previous answer", which is the failure that would make every row in the
//! ledger worthless.

use crate::investigate::{parse_attempt_result, InvestigateError};
use crate::Agent;
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::Preregistration;
use zorp_track::rerun::GateOutcome;
use zorp_track::Project;

/// Turns the gate on. Off unless set, and deliberately so.
pub const GATE_ENV: &str = "ZORP_RERUN_GATE";

/// How many repeats to run. Two by default.
pub const REPEATS_ENV: &str = "ZORP_RERUN_REPEATS";

/// Two, not one, and the reason is in `classify`: the agreement check
/// measures spread across the repeats alone, so a single repeat has zero
/// spread and can never fail it. One repeat can still reach `Reproduced`,
/// it just cannot be contradicted, and the module's own comment says the
/// honest fix is more repeats.
pub const DEFAULT_REPEATS: usize = 2;

/// The ceiling, because each repeat is a whole agent run and the cost is
/// linear in this number. Somebody who wants more than this wants a batch
/// harness, not a browser click.
pub const MAX_REPEATS: usize = 5;

/// Whether the re-run gate runs after an attempt.
pub fn enabled() -> bool {
    matches!(
        std::env::var(GATE_ENV).ok().as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// How many repeats to run, clamped to something a person can afford.
///
/// A zero or an unparseable value falls back to the default rather than
/// silently disabling the gate: `ZORP_RERUN_GATE` is the switch, and a
/// typo in the count must not turn the feature off while looking like it
/// is on.
pub fn repeats() -> usize {
    std::env::var(REPEATS_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_REPEATS)
        .min(MAX_REPEATS)
}

/// What the gate did, for the caller to report.
///
/// Every skip names its reason. A gate that ran and admitted nothing and
/// a gate that never ran look identical in the ledger, and telling them
/// apart is the difference between "the environment is quiet" and "nobody
/// has looked".
#[derive(Debug, Clone, PartialEq)]
pub enum GateReport {
    /// The gate is off.
    Disabled,
    /// Nothing to gate: no forecast for the metric, no numeric outcome,
    /// or an outcome that fell inside its own stated interval.
    NotACandidate,
    /// The gate ran. `admitted` is whether a row went into the ledger,
    /// which two of the four outcomes do.
    Ran {
        outcome: GateOutcome,
        admitted: bool,
    },
    /// The repeats could not be run. Carried rather than raised because a
    /// failed gate must not fail the attempt that was already completed
    /// and recorded.
    Failed(String),
}

impl GateReport {
    /// A line for stderr, or nothing when there is nothing worth saying.
    pub fn describe(&self) -> Option<String> {
        match self {
            // Silence for the two ordinary cases. A person who has not
            // turned the gate on does not need telling on every attempt,
            // and an unsurprising outcome is the common case.
            GateReport::Disabled | GateReport::NotACandidate => None,
            GateReport::Ran { outcome, admitted } => Some(format!(
                "re-run gate: {} ({})",
                outcome.as_str(),
                if *admitted {
                    "admitted to the anomaly ledger"
                } else {
                    "rejected as noise, and counted"
                }
            )),
            GateReport::Failed(why) => Some(format!("re-run gate could not run: {why}")),
        }
    }
}

/// What a repeat needs to be the same run the original was.
///
/// One struct rather than eight parameters, and the grouping is not
/// cosmetic: every field here exists to make the repeat match the
/// original, and a repeat that differs on any of them is a different
/// experiment wearing the same name.
pub struct Replay<'a> {
    pub project: &'a Project,
    pub track_id: &'a str,
    pub prereg: &'a Preregistration,
    pub original_experiment_id: &'a str,
    pub task: &'a str,
    /// Where the transcript stood before the original attempt ran.
    pub seed_len: usize,
    pub checkpoint_mode: &'a CheckpointMode,
}

/// Repeat the attempt and gate it.
///
/// Called after the outcome is recorded and before the kill threshold is
/// enforced, which is deliberate and is the one ordering decision here
/// that matters. A breach kills the track, and `investigate::run` refuses
/// to start on a killed track, so repeats after the kill could not run at
/// all. It is also the right order on its own terms: a breach is the
/// largest deviation an attempt can produce, and a ledger that excluded
/// exactly those would hold only the anomalies that were not bad enough
/// to matter.
///
/// Never returns an error. The attempt this gates has already completed
/// and been recorded, and failing it now over a replay would throw away a
/// result that was already earned.
pub fn run_gate(agent: &mut Agent, replay: &Replay<'_>) -> GateReport {
    if !enabled() {
        return GateReport::Disabled;
    }
    run_gate_with(agent, replay, repeats())
}

/// The gate with the repeat count handed in.
///
/// Split out from `run_gate` so a test can drive the whole path without
/// setting a process-wide environment variable, which the rest of this
/// suite runs in parallel with.
pub fn run_gate_with(agent: &mut Agent, replay: &Replay<'_>, wanted: usize) -> GateReport {
    match replay
        .project
        .store
        .gate_candidate(replay.original_experiment_id, &replay.prereg.metric_name)
    {
        Ok(None) => return GateReport::NotACandidate,
        Ok(Some(_)) => {}
        Err(e) => return GateReport::Failed(e.to_string()),
    }

    match gate_inner(agent, replay, wanted) {
        Ok(report) => report,
        Err(e) => GateReport::Failed(e.to_string()),
    }
}

fn gate_inner(
    agent: &mut Agent,
    replay: &Replay<'_>,
    wanted: usize,
) -> Result<GateReport, InvestigateError> {
    let Replay {
        project,
        track_id,
        prereg,
        original_experiment_id,
        task,
        seed_len,
        checkpoint_mode,
    } = *replay;
    eprintln!(
        "zorp-agent: outcome fell outside its forecast; running {wanted} repeat(s) for the re-run gate"
    );

    let mut repeat_ids = Vec::with_capacity(wanted);
    for n in 1..=wanted {
        let experiment = project.store.create_experiment(track_id, &prereg.id)?;
        // The same conditions the original recorded, from the same
        // function, because the gate compares the two fingerprints and
        // any difference makes the whole thing `Unverifiable`. Recorded
        // before the run for the same reason it is on the original: a
        // condition written afterwards describes a different run.
        super::record_conditions(agent, project, &experiment.id, checkpoint_mode)?;
        // No forecast on a repeat, on purpose. A repeat is not being
        // predicted, it is being used to check a prediction that already
        // exists, and a second expectation on the same metric would give
        // the calibration report a forecast nobody made.
        project
            .store
            .set_experiment_status(&experiment.id, ExperimentStatus::Running)?;

        // Back to where the transcript stood before the original ran, so
        // this is a repeat and not a continuation. Without it the model
        // would be answering with its own previous answer in view, and
        // `Reproduced` would mean "it was shown what it said last time".
        agent.truncate_transcript(seed_len);
        let outcome = agent.run(task);

        let value = match outcome {
            crate::Outcome::Complete(text) => match parse_attempt_result(&text) {
                Ok(attempt) => Some(attempt.metric_value),
                Err(_) => None,
            },
            _ => None,
        };

        match value {
            Some(value) => {
                project.store.record_metric(
                    &experiment.id,
                    &prereg.metric_name,
                    MetricValue::Number(value),
                )?;
                project
                    .store
                    .set_experiment_status(&experiment.id, ExperimentStatus::Completed)?;
            }
            // A repeat that produced no number is recorded as failed and
            // still handed to the gate. `rerun_gate` treats a missing
            // outcome as a divergence, which makes the verdict
            // `Unverifiable`, which is admitted and flagged. That is the
            // designed answer: failing to look is not evidence that there
            // was nothing to see, and quietly dropping the repeat would
            // turn a broken replay into a clean `Reproduced`.
            None => {
                eprintln!(
                    "zorp-agent: repeat {n} of {wanted} produced no numeric outcome; \
                     the gate will report this replay as unverifiable"
                );
                project
                    .store
                    .set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            }
        }
        repeat_ids.push(experiment.id);
    }

    let borrowed: Vec<&str> = repeat_ids.iter().map(String::as_str).collect();
    let verdict =
        project
            .store
            .rerun_gate(original_experiment_id, &prereg.metric_name, &borrowed)?;
    let outcome = verdict.outcome;
    let admitted = project.store.record_gate_verdict(&verdict)?.is_some();
    Ok(GateReport::Ran { outcome, admitted })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switch is opt-in and spelled the way the other opt-in switches
    /// in this codebase are.
    #[test]
    fn the_gate_is_off_unless_asked_for() {
        let on = |v: Option<&str>| matches!(v, Some("1") | Some("true") | Some("yes") | Some("on"));
        assert!(!on(None));
        assert!(!on(Some("0")));
        assert!(!on(Some("")));
        assert!(!on(Some("no")));
        assert!(on(Some("1")));
        assert!(on(Some("true")));
    }

    /// A typo in the count must not turn the gate off while it looks on,
    /// and nobody gets to ask for fifty agent runs from a browser click.
    #[test]
    fn the_repeat_count_falls_back_rather_than_disabling_and_is_capped() {
        let parse = |v: Option<&str>| {
            v.and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_REPEATS)
                .min(MAX_REPEATS)
        };
        assert_eq!(parse(None), DEFAULT_REPEATS);
        assert_eq!(parse(Some("banana")), DEFAULT_REPEATS);
        assert_eq!(parse(Some("0")), DEFAULT_REPEATS);
        assert_eq!(parse(Some("1")), 1);
        assert_eq!(parse(Some("3")), 3);
        assert_eq!(parse(Some("500")), MAX_REPEATS);
    }

    /// The two ordinary cases say nothing. A person who has not turned
    /// the gate on does not want a line about it on every attempt.
    #[test]
    fn only_a_gate_that_did_something_says_so() {
        assert_eq!(GateReport::Disabled.describe(), None);
        assert_eq!(GateReport::NotACandidate.describe(), None);
        assert!(GateReport::Ran {
            outcome: GateOutcome::Reproduced,
            admitted: true,
        }
        .describe()
        .unwrap()
        .contains("admitted"));
        assert!(GateReport::Ran {
            outcome: GateOutcome::Transient,
            admitted: false,
        }
        .describe()
        .unwrap()
        .contains("rejected as noise"));
        assert!(GateReport::Failed("no".into())
            .describe()
            .unwrap()
            .contains("could not run"));
    }

    /// `Unverifiable` is admitted and `Transient` is not, which is the
    /// whole reason a failed replay is still handed to the gate rather
    /// than dropped.
    #[test]
    fn a_failed_replay_is_admitted_and_flagged_rather_than_dropped() {
        assert!(GateOutcome::Unverifiable.admits());
        assert!(GateOutcome::Reproduced.admits());
        assert!(!GateOutcome::Transient.admits());
        assert!(!GateOutcome::Volatile.admits());
    }

    // The end to end cases. Everything above is arithmetic about a
    // verdict that already exists; these two are the only checks that a
    // row ever reaches the ledger, which is the entire point of the
    // module and the thing that was missing before it.

    use crate::investigate::tests::{build_agent, well_formed_response};
    use crate::investigate::{run, PreregParams};
    use tempfile::tempdir;
    use zorp_track::prereg::{get_preregistration, ThresholdDirection};

    /// Set an attempt up whose outcome the stub will produce (42.0) and
    /// whose stated interval is handed in, so a test can choose whether
    /// the repeat lands inside it or outside.
    ///
    /// The original is built by hand rather than through a second `run`
    /// because `record_expectation` refuses once an outcome for the
    /// metric exists, which is integrity rule 1 doing its job: a
    /// forecast written after the result is a postdiction. So the
    /// forecast has to go in before the metric, and `run` records both.
    fn candidate_attempt(
        agent: &mut Agent,
        project: &Project,
        mode: &CheckpointMode,
        observed: f64,
        interval: (f64, f64),
    ) -> (Preregistration, String) {
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        run(
            agent,
            project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 1000.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            mode,
        )
        .unwrap();
        let prereg = get_preregistration(&project.store, "t1").unwrap().unwrap();

        let experiment = project.store.create_experiment("t1", &prereg.id).unwrap();
        super::super::record_conditions(agent, project, &experiment.id, mode).unwrap();
        project
            .store
            .record_expectation(
                &experiment.id,
                "latency_ms",
                (interval.0 + interval.1) / 2.0,
                interval.0,
                interval.1,
                0.8,
                &[],
            )
            .unwrap();
        project
            .store
            .record_metric(&experiment.id, "latency_ms", MetricValue::Number(observed))
            .unwrap();
        project
            .store
            .set_experiment_status(&experiment.id, ExperimentStatus::Completed)
            .unwrap();
        (prereg, experiment.id)
    }

    /// The whole producer, end to end: a surprise runs a repeat, the
    /// repeat agrees, and a row lands in a table that until now no
    /// production path had ever written to.
    #[test]
    fn a_surprise_that_repeats_puts_a_row_in_the_anomaly_ledger() {
        let mut agent = build_agent(well_formed_response());
        let seed_len = agent.transcript_len();
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        // Stub answers 42.0 every time; [0, 10] excludes it, so the
        // original is a candidate and the repeat reproduces it.
        let (prereg, original) = candidate_attempt(&mut agent, &project, &mode, 42.0, (0.0, 10.0));

        assert!(project.store.anomalies_for_track("t1").unwrap().is_empty());
        let after_original = agent.transcript_len();

        let replay = Replay {
            project: &project,
            track_id: "t1",
            prereg: &prereg,
            original_experiment_id: &original,
            task: "does caching help",
            seed_len,
            checkpoint_mode: &mode,
        };
        let report = run_gate_with(&mut agent, &replay, 1);

        assert_eq!(
            report,
            GateReport::Ran {
                outcome: GateOutcome::Reproduced,
                admitted: true,
            }
        );
        assert_eq!(project.store.anomalies_for_track("t1").unwrap().len(), 1);
        assert_eq!(project.store.noise_report().unwrap().reproduced, 1);

        // The repeat started where the original started, so it added the
        // same number of messages the original did and the transcript is
        // exactly as long as it was. Delete the `truncate_transcript`
        // call in `gate_inner` and this grows, which is the mutation
        // that matters most here: a repeat that can read the original's
        // answer turns `Reproduced` into "it was shown what it said last
        // time", and every row in the ledger becomes worthless.
        assert_eq!(agent.transcript_len(), after_original);
    }

    /// The other half, and the reason the gate exists at all. A surprise
    /// whose repeat lands back inside the interval is a one-off, and
    /// nothing goes in the ledger. It is still counted, so the noise
    /// rate is readable rather than invisible.
    #[test]
    fn a_surprise_that_does_not_repeat_is_counted_and_not_admitted() {
        let mut agent = build_agent(well_formed_response());
        let seed_len = agent.transcript_len();
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        // Original is far outside [40, 50]; the repeat answers 42.0,
        // which is inside it.
        let (prereg, original) =
            candidate_attempt(&mut agent, &project, &mode, 200.0, (40.0, 50.0));

        let replay = Replay {
            project: &project,
            track_id: "t1",
            prereg: &prereg,
            original_experiment_id: &original,
            task: "does caching help",
            seed_len,
            checkpoint_mode: &mode,
        };
        let report = run_gate_with(&mut agent, &replay, 1);

        assert_eq!(
            report,
            GateReport::Ran {
                outcome: GateOutcome::Transient,
                admitted: false,
            }
        );
        assert!(project.store.anomalies_for_track("t1").unwrap().is_empty());
        let noise = project.store.noise_report().unwrap();
        assert_eq!(noise.transient, 1);
        assert_eq!(noise.total(), 1);
    }

    /// An outcome inside its own interval is not a candidate, and the
    /// gate must not spend a single model call on it. Without the
    /// screen every attempt would pay for repeats.
    #[test]
    fn an_unsurprising_outcome_never_reaches_a_repeat() {
        let mut agent = build_agent(well_formed_response());
        let seed_len = agent.transcript_len();
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let (prereg, original) = candidate_attempt(&mut agent, &project, &mode, 42.0, (0.0, 100.0));

        let before = project.store.experiments_for("t1").unwrap().len();
        let replay = Replay {
            project: &project,
            track_id: "t1",
            prereg: &prereg,
            original_experiment_id: &original,
            task: "does caching help",
            seed_len,
            checkpoint_mode: &mode,
        };

        assert_eq!(
            run_gate_with(&mut agent, &replay, 1),
            GateReport::NotACandidate
        );
        assert_eq!(project.store.experiments_for("t1").unwrap().len(), before);
        assert_eq!(project.store.noise_report().unwrap().total(), 0);
    }
}
