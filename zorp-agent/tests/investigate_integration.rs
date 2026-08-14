//! End-to-end integration test for `investigate::run`, using a stub
//! `Model` (no MCP server needed: unlike `validate`, `investigate` does
//! not require a search-capable tool). Mirrors
//! `tests/validate_integration.rs`'s shape and the note there about why
//! this whole file compiles to nothing outside the `research` feature.
#![cfg(feature = "research")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use zorp_agent::investigate::{run, InvestigateError, PreregParams};
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::ThresholdDirection;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

struct StubModel {
    response: String,
    calls: Arc<AtomicUsize>,
}

impl Model for StubModel {
    fn complete(&self, _messages: &[Message], _tools: &[serde_json::Value]) -> Result<AssistantMessage, BoxErr> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(AssistantMessage {
            content: self.response.clone(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(StubModel { response: self.response.clone(), calls: self.calls.clone() })
    }
}

struct Rejecting;
impl zorp_track::checkpoint::Decider for Rejecting {
    fn decide(&self, _prompt: &str) -> bool {
        false
    }
}

struct RejectSecondCall {
    calls: AtomicUsize,
}
impl zorp_track::checkpoint::Decider for RejectSecondCall {
    fn decide(&self, _prompt: &str) -> bool {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        n == 0 // first call (prereg checkpoint): approve; second call (post-attempt checkpoint): reject
    }
}

fn build_agent(response: &str) -> Agent {
    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel { response: response.to_string(), calls };
    Agent::new(
        Box::new(model),
        "system",
        5,
        std::env::temp_dir(),
        cancel_token(),
        ApprovalMode::AutoApprove,
    )
    .register_builtins()
}

fn well_formed_response() -> &'static str {
    "Done.\n```json\n{\"metric_value\": 42.0, \"summary\": \"worked\"}\n```\n"
}

#[test]
fn full_round_trip_prereg_attempt_metric_checkpoint_approved() {
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap();

    assert!(approved);
    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Active);

    // The attempt's metric actually landed in the run record, under the
    // pre-registered name, and the experiment finished Completed.
    let experiments = project.store.experiments_for("t1").unwrap();
    assert_eq!(experiments.len(), 1);
    assert_eq!(experiments[0].status, ExperimentStatus::Completed);
    let metrics = project.store.metrics_for(&experiments[0].id).unwrap();
    assert_eq!(metrics, vec![("latency_ms".to_string(), MetricValue::Number(42.0))]);
}

#[test]
fn a_threshold_breach_kills_the_track_even_under_auto_approve() {
    // The stub reports latency_ms = 42.0 against a lower-is-better kill
    // threshold of 10.0: a breach. AutoApprove approves every checkpoint
    // it is asked, so a Killed track here proves the kill is enforced
    // without asking, not decided.
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 10.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap();

    assert!(!approved);
    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Killed);

    // The metric itself was still recorded honestly before the kill.
    let experiments = project.store.experiments_for("t1").unwrap();
    assert_eq!(experiments.len(), 1);
    assert_eq!(experiments[0].status, ExperimentStatus::Completed);
    let metrics = project.store.metrics_for(&experiments[0].id).unwrap();
    assert_eq!(metrics, vec![("latency_ms".to_string(), MetricValue::Number(42.0))]);

    // The kill left its record behind: an investigate-threshold row
    // exists (record_enforced_kill's persistence of the reason itself is
    // asserted in zorp-track's unit tests, where the connection is
    // reachable).
    let killed_at = project.store.latest_checkpoint_time("t1", "investigate-threshold").unwrap();
    assert!(killed_at.is_some());
}

#[test]
fn a_metric_within_the_threshold_keeps_the_track_active() {
    // 42.0 against a lower-is-better threshold of 100.0: no breach, so
    // the normal checkpoint runs and (auto-approved) the track stays
    // Active.
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap();

    assert!(approved);
    assert_eq!(project.store.get_track("t1").unwrap().status, TrackStatus::Active);
}

#[test]
fn a_higher_is_better_metric_below_the_threshold_is_killed() {
    // 42.0 against a higher-is-better threshold of 50.0: the metric fell
    // short, so the track is killed.
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "accuracy", kill_threshold: 50.0, threshold_direction: ThresholdDirection::HigherIsBetter }),
        &mode,
    )
    .unwrap();

    assert!(!approved);
    assert_eq!(project.store.get_track("t1").unwrap().status, TrackStatus::Killed);
}

#[test]
fn a_legacy_prereg_without_a_direction_is_not_enforced() {
    // A prereg.md written before threshold directions existed, indexed
    // by the rebuild on Project::open. Enforcement must not guess a
    // direction: even a wildly breaching-looking value leaves the
    // decision to the checkpoint.
    let dir = tempdir().unwrap();
    let track_id = "t1";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "does caching help").unwrap();
        let track_dir = project.track_dir(track_id);
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(
            track_dir.join("prereg.md"),
            "# Pre-registration: t1\n\nHypothesis: does caching help\nMetric: latency_ms\nKill threshold: 10\n",
        )
        .unwrap();
    }

    let project = Project::open(dir.path()).unwrap();
    let mut agent = build_agent(well_formed_response());
    let mode = CheckpointMode::terminal(true).unwrap();

    // 42.0 would breach a lower-is-better threshold of 10, but no
    // direction is recorded, so the auto-approved checkpoint keeps the
    // track alive.
    let approved = run(&mut agent, &project, track_id, "does caching help", None, &mode).unwrap();
    assert!(approved);
    assert_eq!(project.store.get_track(track_id).unwrap().status, TrackStatus::Active);
}

#[test]
fn an_unscorable_answer_fails_the_experiment() {
    // The model answers without the required fenced JSON block, so
    // parse_attempt_result fails. The experiment must not be left
    // Running: it ends Failed, and no metric is recorded.
    let mut agent = build_agent("I could not measure anything, sorry.");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap_err();
    assert!(matches!(err, InvestigateError::Scoring(_)), "unexpected error: {err}");

    let experiments = project.store.experiments_for("t1").unwrap();
    assert_eq!(experiments.len(), 1);
    assert_eq!(experiments[0].status, ExperimentStatus::Failed);
    assert!(project.store.metrics_for(&experiments[0].id).unwrap().is_empty());
}

#[test]
fn rejected_post_attempt_checkpoint_kills_the_track() {
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::Interactive(Arc::new(RejectSecondCall { calls: AtomicUsize::new(0) }));

    let approved = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap();

    assert!(!approved);
    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Killed);
    // Confirm the code path actually reached the post-attempt checkpoint
    // (not the prereg checkpoint): RejectSecondCall only rejects on its
    // second `decide()` invocation, so a Killed track here proves the
    // prereg checkpoint was approved, the attempt ran, and the metric
    // was recorded (create_experiment/record_metric/set_experiment_status)
    // before the post-attempt checkpoint rejected.
}

#[test]
fn rejected_prereg_checkpoint_kills_the_track_before_any_attempt_runs() {
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::Interactive(Arc::new(Rejecting));

    let approved = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap();

    assert!(!approved);
    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Killed);
}

#[test]
fn a_killed_track_refuses_a_second_investigate_call() {
    let mut agent = build_agent(well_formed_response());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    project.store.set_track_status("t1", TrackStatus::Killed).unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(
        &mut agent,
        &project,
        "t1",
        "does caching help",
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0, threshold_direction: ThresholdDirection::LowerIsBetter }),
        &mode,
    )
    .unwrap_err();

    assert!(matches!(err, InvestigateError::TrackKilled));
}
