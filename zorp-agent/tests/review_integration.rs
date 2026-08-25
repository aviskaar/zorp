//! End-to-end integration test for `review::run`: a real `Project` on
//! disk, a real track, and a stub `Model` that answers every reviewer and
//! every refuter from a script. No network and no live model, on purpose:
//! everything this exercises (the loop, the report, the record) is
//! deterministic, and a test that needed a model could not assert on any
//! of it.
#![cfg(feature = "research")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use zorp_agent::review::dimension::Selection;
use zorp_agent::review::{run, Bounds, ReviewError};
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

const PAPER: &str = "# Caching and latency\n\n\
We observe a 14% reduction in latency across the suite.\n\n\
The evaluation ran once on a single machine.\n";

/// Answers every agent with the same scripted reply. Which reply is
/// decided by what the prompt asks for, not by call order, so the test
/// does not depend on how many agents the loop happens to run.
struct Scripted {
    finding: String,
    vote: String,
    calls: Arc<AtomicUsize>,
}

impl Model for Scripted {
    fn complete(
        &self,
        messages: &[Message],
        _tools: &[serde_json::Value],
    ) -> Result<AssistantMessage, BoxErr> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let asked: String = messages.iter().map(|m| m.text().into_owned()).collect();
        let content = if asked.contains("REFUTE") {
            self.vote.clone()
        } else {
            self.finding.clone()
        };
        Ok(AssistantMessage {
            content,
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(Scripted {
            finding: self.finding.clone(),
            vote: self.vote.clone(),
            calls: self.calls.clone(),
        })
    }
}

fn agent(finding: &str, vote: &str) -> Agent {
    Agent::new(
        Box::new(Scripted {
            finding: finding.to_string(),
            vote: vote.to_string(),
            calls: Arc::new(AtomicUsize::new(0)),
        }),
        "system",
        3,
        std::env::temp_dir(),
        cancel_token(),
        ApprovalMode::AutoApprove,
    )
    .register_builtins()
}

fn nothing() -> &'static str {
    "```json\n{\"findings\": []}\n```"
}

fn one_finding() -> String {
    "```json\n{\"findings\": [{\"severity\": \"blocking\", \"claim\": \"the latency figure has \
     no spread\", \"anchor\": \"We observe a 14% reduction in latency\", \"evidence\": \"a \
     single run is reported as a result\"}]}\n```"
        .to_string()
}

fn upheld() -> &'static str {
    "```json\n{\"vote\": \"upheld\", \"reason\": \"the paper really does report one run\"}\n```"
}

fn small_bounds() -> Bounds {
    Bounds {
        max_rounds: 2,
        quiet_rounds: 2,
        max_depth: 2,
        max_agents: 40,
        refuters_per_finding: 3,
    }
}

fn track_with_paper(project: &Project, track_id: &str) -> std::path::PathBuf {
    project
        .store
        .create_track(track_id, "does caching help")
        .unwrap();
    let dir = project.track_dir(track_id);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("draft.md");
    std::fs::write(&path, PAPER).unwrap();
    path
}

#[test]
fn a_review_writes_a_report_and_a_structured_record_and_checkpoints() {
    let mut a = agent(&one_finding(), upheld());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_paper(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(
        &mut a,
        &project,
        "t1",
        None,
        None,
        &Selection::parse("statistical-validity").unwrap(),
        &small_bounds(),
        &mode,
    )
    .unwrap();
    assert!(approved);

    let track_dir = project.track_dir("t1");
    let md = std::fs::read_to_string(track_dir.join("review.md")).unwrap();
    assert!(md.contains("# Paper review"));
    assert!(md.contains("the latency figure has no spread"));
    assert!(md.contains("Verification: upheld"));

    // The markdown is for a person; the JSON is the record. Severities
    // and verdicts have to survive it, because re-parsing the prose
    // would not recover them.
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(track_dir.join("review.json")).unwrap())
            .unwrap();
    assert_eq!(json["findings"][0]["severity"], "blocking");
    assert_eq!(json["findings"][0]["verification"]["verdict"], "upheld");
    assert_eq!(json["stop"]["kind"], "round_cap");

    // The run itself is on the record, not only the file.
    let checkpoint = project
        .store
        .latest_checkpoint_time("t1", "review")
        .unwrap();
    assert!(checkpoint.is_some(), "review must record a checkpoint");
}

/// A reviewer that always finds something tells you nothing, so a paper
/// with nothing wrong has to come back with nothing.
#[test]
fn a_clean_paper_produces_a_report_with_no_findings() {
    let mut a = agent(nothing(), upheld());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_paper(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    run(
        &mut a,
        &project,
        "t1",
        None,
        None,
        &Selection::parse("statistical-validity").unwrap(),
        &small_bounds(),
        &mode,
    )
    .unwrap();

    let md = std::fs::read_to_string(project.track_dir("t1").join("review.md")).unwrap();
    assert!(md.contains("No findings."));
}

#[test]
fn a_killed_track_is_refused() {
    let mut a = agent(nothing(), upheld());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_paper(&project, "t1");
    project
        .store
        .set_track_status("t1", TrackStatus::Killed)
        .unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(
        &mut a,
        &project,
        "t1",
        None,
        None,
        &Selection::parse("statistical-validity").unwrap(),
        &small_bounds(),
        &mode,
    )
    .unwrap_err();
    assert!(matches!(err, ReviewError::TrackKilled));
}

#[test]
fn a_missing_paper_says_how_to_get_one() {
    let mut a = agent(nothing(), upheld());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project
        .store
        .create_track("t1", "does caching help")
        .unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(
        &mut a,
        &project,
        "t1",
        None,
        None,
        &Selection::parse("statistical-validity").unwrap(),
        &small_bounds(),
        &mode,
    )
    .unwrap_err();
    assert!(matches!(err, ReviewError::NoPaper(_)));
    assert!(err.to_string().contains("co-write"));
}

/// A paper outside the track is the case that lets this be developed
/// against a real paper in the repository.
#[test]
fn an_explicit_paper_path_is_reviewed_instead_of_the_draft() {
    let mut a = agent(&one_finding(), upheld());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project
        .store
        .create_track("t1", "does caching help")
        .unwrap();
    let elsewhere = dir.path().join("elsewhere.md");
    std::fs::write(&elsewhere, PAPER).unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    run(
        &mut a,
        &project,
        "t1",
        Some(elsewhere.to_str().unwrap()),
        None,
        &Selection::parse("statistical-validity").unwrap(),
        &small_bounds(),
        &mode,
    )
    .unwrap();

    let md = std::fs::read_to_string(project.track_dir("t1").join("review.md")).unwrap();
    assert!(md.contains("elsewhere.md"));
}

/// The traceability dimension is zorp's own thesis applied to a paper, so
/// when there is no evidence to trace to, the report has to say the check
/// did not run rather than let its silence read as a pass.
#[test]
fn traceability_is_skipped_and_named_when_the_track_has_no_evidence() {
    let mut a = agent(nothing(), upheld());
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_paper(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    run(
        &mut a,
        &project,
        "t1",
        None,
        None,
        &Selection::parse("claim-evidence-traceability").unwrap(),
        &small_bounds(),
        &mode,
    )
    .unwrap();

    let md = std::fs::read_to_string(project.track_dir("t1").join("review.md")).unwrap();
    assert!(md.contains("Claim to evidence traceability: not run"));
    assert!(md.contains("This review is incomplete"));
}
