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
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
        &mode,
    )
    .unwrap();

    assert!(approved);
    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Active);
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
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
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
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
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
        Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
        &mode,
    )
    .unwrap_err();

    assert!(matches!(err, InvestigateError::TrackKilled));
}
