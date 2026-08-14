//! End-to-end integration test for `co_write::run`, using a stub `Model`
//! (no MCP server needed, same as `investigate`). Mirrors
//! `tests/investigate_integration.rs`'s shape.
#![cfg(feature = "research")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use zorp_agent::co_write::{run, CoWriteError};
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::track::TrackStatus;
use zorp_track::Project;

struct StubModel {
    response: String,
    calls: Arc<AtomicUsize>,
}

impl Model for StubModel {
    fn complete(
        &self,
        _messages: &[Message],
        _tools: &[serde_json::Value],
    ) -> Result<AssistantMessage, BoxErr> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(AssistantMessage {
            content: self.response.clone(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(StubModel {
            response: self.response.clone(),
            calls: self.calls.clone(),
        })
    }
}

struct RejectAll;
impl zorp_track::checkpoint::Decider for RejectAll {
    fn decide(&self, _prompt: &str) -> bool {
        false
    }
}

fn build_agent(response: &str) -> Agent {
    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel {
        response: response.to_string(),
        calls,
    };
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

fn track_with_one_metric(project: &Project, track_id: &str) {
    project
        .store
        .create_track(track_id, "does caching help")
        .unwrap();
    let exp = project
        .store
        .create_experiment(track_id, "no-prereg")
        .unwrap();
    project
        .store
        .set_experiment_status(&exp.id, ExperimentStatus::Completed)
        .unwrap();
    project
        .store
        .record_metric(&exp.id, "latency_ms", MetricValue::Number(42.0))
        .unwrap();
}

#[test]
fn full_round_trip_writes_draft_and_approves() {
    let mut agent = build_agent("# Draft\n\nLatency improved to 42ms.");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
    assert!(approved);

    let draft_path = project.track_dir("t1").join("draft.md");
    let content = std::fs::read_to_string(&draft_path).unwrap();
    assert_eq!(content, "# Draft\n\nLatency improved to 42ms.");
}

#[test]
fn rejected_checkpoint_leaves_track_status_unchanged() {
    let mut agent = build_agent("a draft");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::Interactive(Arc::new(RejectAll));

    let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
    assert!(!approved);

    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Active);
}

#[test]
fn a_second_call_overwrites_a_hand_edited_draft_and_still_succeeds() {
    let mut agent = build_agent("first draft");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();

    let draft_path = project.track_dir("t1").join("draft.md");

    // Hand-edit draft.md after the first run's checkpoint was recorded, so
    // its mtime is strictly later than the latest co-write checkpoint. That
    // is the condition co-write's stale-draft warning looks for. The warning
    // is advisory: the second run must still succeed and still overwrite.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&draft_path, "HUMAN EDIT").unwrap();

    let mut agent2 = build_agent("second draft, overwritten");
    let approved = run(&mut agent2, &project, "t1", "does caching help", &mode).unwrap();
    assert!(approved);

    let content = std::fs::read_to_string(&draft_path).unwrap();
    assert_eq!(content, "second draft, overwritten");
}

#[test]
fn no_metrics_refuses_before_running_the_agent() {
    let mut agent = build_agent("a draft");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project
        .store
        .create_track("t1", "does caching help")
        .unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
    assert!(matches!(err, CoWriteError::NoMetrics));
}
