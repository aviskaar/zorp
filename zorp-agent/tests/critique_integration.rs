//! End-to-end integration test for `critique::run`, using stub `Model`s
//! (no network, no MCP server). Mirrors `tests/co_write_integration.rs`'s
//! shape, and deliberately starts from a draft that `co_write::run`
//! actually produced, because the seam between the two is where a
//! critique pass either earns its place or does not.
#![cfg(feature = "research")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use zorp_agent::critique::{run, CritiqueError};
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::track::TrackStatus;
use zorp_track::Project;

struct FixedModel {
    response: String,
}

impl Model for FixedModel {
    fn complete(
        &self,
        _messages: &[Message],
        _tools: &[serde_json::Value],
    ) -> Result<AssistantMessage, BoxErr> {
        Ok(AssistantMessage {
            content: self.response.clone(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(FixedModel {
            response: self.response.clone(),
        })
    }
}

struct ScriptedModel {
    responses: Arc<Mutex<VecDeque<String>>>,
    calls: Arc<AtomicUsize>,
}

impl Model for ScriptedModel {
    fn complete(
        &self,
        _messages: &[Message],
        _tools: &[serde_json::Value],
    ) -> Result<AssistantMessage, BoxErr> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let next = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| -> BoxErr { "scripted model ran out of responses".into() })?;
        Ok(AssistantMessage {
            content: next,
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(ScriptedModel {
            responses: self.responses.clone(),
            calls: self.calls.clone(),
        })
    }
}

fn agent_for(model: Box<dyn Model>) -> Agent {
    Agent::new(
        model,
        "system",
        5,
        std::env::temp_dir(),
        cancel_token(),
        ApprovalMode::AutoApprove,
    )
    .register_builtins()
}

fn scripted_agent(responses: &[&str]) -> (Agent, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let model = ScriptedModel {
        responses: Arc::new(Mutex::new(
            responses.iter().map(|s| s.to_string()).collect(),
        )),
        calls: calls.clone(),
    };
    (agent_for(Box::new(model)), calls)
}

fn no_claims() -> String {
    "```json\n{\"claims\": []}\n```\n".to_string()
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

const INVENTED: &str = "# Findings\n\nLatency improved to 42ms. Throughput reached 900 rps.\n";
const GROUNDED: &str = "# Findings\n\nLatency improved to 42ms.\n";

/// The whole point, end to end: co-write drafts a figure the record does
/// not hold, and critique catches it without anybody telling it which
/// figure was invented.
#[test]
fn a_figure_co_write_invented_is_caught_and_removed() {
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let mut writer = agent_for(Box::new(FixedModel {
        response: INVENTED.to_string(),
    }));
    zorp_agent::co_write::run(&mut writer, &project, "t1", "does caching help", &mode).unwrap();

    let (mut critic, calls) = scripted_agent(&[&no_claims(), GROUNDED, &no_claims()]);
    let report = run(&mut critic, &project, "t1", 2, &mode).unwrap();

    assert_eq!(report.initial(), 1, "{:?}", report.rounds);
    assert_eq!(report.remaining(), 0);
    assert!(report.draft_changed);
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    let track_dir = project.track_dir("t1");
    assert_eq!(
        std::fs::read_to_string(track_dir.join("draft.md")).unwrap(),
        GROUNDED
    );
    assert_eq!(
        std::fs::read_to_string(track_dir.join("draft.pre-critique.md")).unwrap(),
        INVENTED
    );

    // The criticism itself is in the run record, not only in the file
    // that got rewritten.
    let rounds = project.store.critiques_for("t1").unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0].findings.len(), 1);
    assert_eq!(rounds[0].findings[0].kind, "number-not-in-record");
    assert!(rounds[0].findings[0].detail.contains("900"));
    assert!(rounds[1].findings.is_empty());
    assert!(rounds[1].accepted);
}

/// A draft that the record does back must survive the pass untouched.
#[test]
fn a_grounded_draft_survives_the_pass_unchanged() {
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let mut writer = agent_for(Box::new(FixedModel {
        response: GROUNDED.to_string(),
    }));
    zorp_agent::co_write::run(&mut writer, &project, "t1", "does caching help", &mode).unwrap();

    let (mut critic, calls) = scripted_agent(&[
        "```json\n{\"claims\": [{\"claim\": \"Latency improved to 42ms.\", \"evidence\": \"metric:latency_ms\"}]}\n```",
    ]);
    let report = run(&mut critic, &project, "t1", 2, &mode).unwrap();

    assert!(report.was_clean());
    assert!(!report.draft_changed);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no revision was requested");
    assert_eq!(
        std::fs::read_to_string(project.track_dir("t1").join("draft.md")).unwrap(),
        GROUNDED
    );
}

#[test]
fn a_killed_track_is_refused_and_a_missing_draft_says_to_run_co_write() {
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let (mut critic, _) = scripted_agent(&[&no_claims()]);
    let err = run(&mut critic, &project, "t1", 2, &mode).unwrap_err();
    assert!(matches!(err, CritiqueError::NoDraft), "got {err:?}");
    assert!(err.to_string().contains("co-write"));

    std::fs::create_dir_all(project.track_dir("t1")).unwrap();
    std::fs::write(project.track_dir("t1").join("draft.md"), GROUNDED).unwrap();
    project
        .store
        .set_track_status("t1", TrackStatus::Killed)
        .unwrap();
    let err = run(&mut critic, &project, "t1", 2, &mode).unwrap_err();
    assert!(matches!(err, CritiqueError::TrackKilled), "got {err:?}");
}
