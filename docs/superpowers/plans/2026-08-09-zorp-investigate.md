# investigate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build zorp's second capability, `investigate`: a new `zorp-agent investigate` subcommand that runs one staged, pre-registered attempt against a track, recording a typed metric to the existing `zorp-track` experiment foundation and checkpointing the result to a human.

**Architecture:** A new `zorp-agent/src/investigate/` module (mirroring `validate`'s file layout: `mod.rs`, `result.rs`, `error.rs`), wired into `main.rs` as a new subcommand behind the `research` feature, reusing the existing `Agent`/`attach_mcp_tools` machinery and the already-built `zorp-track` experiment/prereg/checkpoint primitives. One new small read function is added to `zorp-track` (`get_preregistration`) since nothing today reads a `Preregistration` back out of the database; everything else needed already exists.

**Tech Stack:** Rust, `zorp-track` (DuckDB-backed `Store`), `zorp-agent`'s `Agent`/`Model`/`Outcome`, `serde`/`serde_json` for the attempt-result JSON block.

## Global Constraints

- No new `zorp-track` schema changes: `experiments`, `metrics`, `preregistrations`, `checkpoints` tables already exist and are reused as-is.
- `investigate` does not compute or store a kill-threshold "direction" (above/below is favorable); the checkpoint prompt shows the human the metric value and threshold and lets them decide. Do not add threshold-comparison logic.
- `CheckpointMode` has exactly two variants, `Interactive(Arc<dyn Decider>)` and `AutoApprove`; there is no auto-reject constructor. Tests needing a rejected checkpoint must construct `CheckpointMode::Interactive(Arc::new(<a stub Decider that returns false>))`, the same pattern `zorp-track/src/checkpoint.rs`'s own tests use.
- `Store::record_checkpoint`'s `kind` parameter is a free-form `&str`, not an enum. Use `"investigate-prereg"` for the pre-registration checkpoint and `"investigate"` for the post-attempt checkpoint.
- `investigate::run`'s signature returns `Result<bool, InvestigateError>`, mirroring `validate::run`'s `Result<bool, ValidateError>` shape exactly (approved/rejected as the bool, everything else as the error).
- `investigate` does not require a search-capable MCP tool (unlike `validate`); do not add a `has_search_tool` style gate.
- Run `cargo test -p zorp-agent --features research` (not `cargo test --workspace`) to exercise any of this plan's zorp-agent code; run plain `cargo test -p zorp-track` for the zorp-track task.

---

### Task 1: `zorp-track`: read back a pre-registration

**Files:**
- Modify: `zorp-track/src/prereg.rs`
- Test: `zorp-track/src/prereg.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub fn get_preregistration(store: &Store, track_id: &str) -> Result<Option<Preregistration>, TrackError>` — `Ok(None)` if no row exists for `track_id`, `Ok(Some(Preregistration { .. }))` if one does, matching the struct already defined at `zorp-track/src/prereg.rs:10-20`.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `zorp-track/src/prereg.rs` (after the existing `parse_prereg_md_round_trips_render_prereg_md` test):

```rust
    #[test]
    fn get_preregistration_returns_none_when_absent() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        assert_eq!(get_preregistration(&store, "t1").unwrap(), None);
    }

    #[test]
    fn get_preregistration_returns_the_written_row() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let written = write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0).unwrap();

        let read_back = get_preregistration(&store, "t1").unwrap().unwrap();
        assert_eq!(read_back, written);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p zorp-track get_preregistration -- --nocapture`
Expected: FAIL with "cannot find function `get_preregistration` in this scope"

- [ ] **Step 3: Implement `get_preregistration`**

Add to `zorp-track/src/prereg.rs`, directly after `write_prereg`'s closing brace and before `verify_prereg_integrity`:

```rust
/// Read back the `preregistrations` row for `track_id`, if one exists.
/// `None` means no pre-registration has been written yet for this track
/// (a normal state for a fresh track, not an error); any other failure
/// to read is a real `TrackError`.
pub fn get_preregistration(store: &Store, track_id: &str) -> Result<Option<Preregistration>, TrackError> {
    let row = store
        .conn
        .query_row(
            "SELECT id, track_id, hypothesis_snapshot, metric_name, kill_threshold, file_path, file_hash, git_commit_hash, committed_at \
             FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |r| {
                let file_path: String = r.get(5)?;
                Ok(Preregistration {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    hypothesis_snapshot: r.get(2)?,
                    metric_name: r.get(3)?,
                    kill_threshold: r.get(4)?,
                    file_path: PathBuf::from(file_path),
                    file_hash: r.get(6)?,
                    git_commit_hash: r.get(7)?,
                    committed_at: r.get(8)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p zorp-track get_preregistration -- --nocapture`
Expected: PASS (both new tests), and `cargo test -p zorp-track` overall still passes (no regressions).

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/prereg.rs
git commit -m "feat(zorp-track): add get_preregistration to read back a track's prereg"
```

---

### Task 2: `zorp-agent`: `investigate::result` — parse an attempt's JSON block

**Files:**
- Create: `zorp-agent/src/investigate/result.rs`
- Test: `zorp-agent/src/investigate/result.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub struct AttemptResult { pub metric_value: f64, pub summary: String }`, `pub enum ParseError { NoFencedBlock, InvalidJson(String), MissingMetricValue }`, `pub fn parse_attempt_result(agent_output: &str) -> Result<AttemptResult, ParseError>`.
- This module's `ParseError` is separate from `validate::result::ParseError` (different variants; no shared type). This module's `all_fenced_blocks` is its own private copy, not imported from `validate`, since `validate::result::all_fenced_blocks` is private to that module.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/investigate/result.rs`:

```rust
use serde::Deserialize;
use std::fmt;

#[derive(Debug, Deserialize)]
struct RawAttemptResult {
    metric_value: Option<f64>,
    summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttemptResult {
    pub metric_value: f64,
    pub summary: String,
}

#[derive(Debug)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
    MissingMetricValue,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => write!(f, "no fenced JSON block found in the agent's answer"),
            ParseError::InvalidJson(msg) => write!(f, "fenced block was not valid JSON: {msg}"),
            ParseError::MissingMetricValue => write!(f, "fenced block has no metric_value"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Pull the contents of every fenced code block out of `text`, in order
/// of appearance. Mirrors `validate::result::all_fenced_blocks`
/// (duplicated rather than shared: that copy is private to the
/// `validate` module, and the result shapes the two modules parse
/// differ), for the same reason: the model may quote another fenced
/// block (a log line, a config snippet) before its final JSON answer.
fn all_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after_start = &rest[start + 3..];
        let content_start = after_start.find('\n').map(|i| i + 1).unwrap_or(0);
        let after_open = &after_start[content_start..];
        let Some(end) = after_open.find("```") else {
            break;
        };
        blocks.push(after_open[..end].trim_end().to_string());
        rest = &after_open[end + 3..];
    }
    blocks
}

/// Parse the agent's final answer into an `AttemptResult`. Scans every
/// fenced block (not just the first) and tries to deserialize each into
/// the expected shape, same discipline as `validate::result::
/// parse_validation_result`. `metric_value` is required: a block that
/// parses as JSON but omits it (or the model's answer had no valid
/// block at all) is a scoring failure, not a silent zero.
pub fn parse_attempt_result(agent_output: &str) -> Result<AttemptResult, ParseError> {
    let blocks = all_fenced_blocks(agent_output);
    if blocks.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let mut last_err = None;
    let raw: RawAttemptResult = 'found: {
        for block in &blocks {
            match serde_json::from_str(block) {
                Ok(raw) => break 'found raw,
                Err(e) => last_err = Some(e),
            }
        }
        return Err(ParseError::InvalidJson(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ));
    };

    let metric_value = raw.metric_value.ok_or(ParseError::MissingMetricValue)?;
    Ok(AttemptResult { metric_value, summary: raw.summary })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(json: &str) -> String {
        format!("Here is my finding.\n```json\n{json}\n```\n")
    }

    #[test]
    fn parses_a_well_formed_block() {
        let text = wrap(r#"{"metric_value": 42.5, "summary": "latency improved"}"#);
        let result = parse_attempt_result(&text).unwrap();
        assert_eq!(result.metric_value, 42.5);
        assert_eq!(result.summary, "latency improved");
    }

    #[test]
    fn missing_block_errors() {
        let err = parse_attempt_result("no block here at all").unwrap_err();
        assert!(matches!(err, ParseError::NoFencedBlock));
    }

    #[test]
    fn missing_metric_value_errors() {
        let text = wrap(r#"{"summary": "no number given"}"#);
        let err = parse_attempt_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::MissingMetricValue));
    }

    #[test]
    fn skips_a_decoy_leading_fenced_block_and_parses_the_json_one() {
        let text = format!(
            "Here's the config I found:\n```yaml\nkey: value\n```\nAnd here is my finding.\n```json\n{}\n```\n",
            r#"{"metric_value": 7.0, "summary": "done"}"#
        );
        let result = parse_attempt_result(&text).unwrap();
        assert_eq!(result.metric_value, 7.0);
    }

    #[test]
    fn invalid_json_in_block_errors() {
        let text = wrap("{ not json");
        let err = parse_attempt_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p zorp-agent --features research investigate::result -- --nocapture`
Expected: FAIL — `zorp-agent/src/investigate/` doesn't exist yet, so this won't even compile as a module (the crate-level `mod investigate;` is added in Task 6). For this task, verify the file's own logic instead by temporarily running `cargo test -p zorp-agent --features research` — expect a compile error naming `investigate` as an unresolved module. This is expected; proceed to Task 3 before this compiles cleanly. Do not add `mod investigate;` yet — that is Task 6's job, after `error.rs` (Task 3) and `mod.rs` (Task 4) also exist.

- [ ] **Step 3: Note for reviewer**

This task intentionally leaves the crate unable to compile `investigate::result`'s tests in isolation, because `zorp-agent/src/lib.rs` does not yet declare `mod investigate;`. Task compilation and full test passes happen once Task 6 wires the module in. Commit this file now; Task 6's own test run is what proves these tests actually pass.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/investigate/result.rs
git commit -m "feat(zorp-agent): add investigate::result attempt-JSON parsing"
```

---

### Task 3: `zorp-agent`: `investigate::error`

**Files:**
- Create: `zorp-agent/src/investigate/error.rs`
- Test: `zorp-agent/src/investigate/error.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `investigate::result::ParseError` (Task 2).
- Produces: `pub enum InvestigateError { TrackKilled, PreregRequired { missing: &'static str }, PreregMismatch { field: &'static str, recorded: String, provided: String }, AgentOutcome(String), Scoring(ParseError), Track(zorp_track::TrackError) }`, `impl fmt::Display`, `impl std::error::Error`, `impl From<ParseError>`, `impl From<zorp_track::TrackError>`.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/investigate/error.rs`:

```rust
use super::result::ParseError;
use std::fmt;

#[derive(Debug)]
pub enum InvestigateError {
    TrackKilled,
    PreregRequired { missing: &'static str },
    PreregMismatch { field: &'static str, recorded: String, provided: String },
    AgentOutcome(String),
    Scoring(ParseError),
    Track(zorp_track::TrackError),
}

impl fmt::Display for InvestigateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvestigateError::TrackKilled => write!(f, "this track has already been killed"),
            InvestigateError::PreregRequired { missing } => write!(
                f,
                "no pre-registration exists for this track yet; --{missing} is required on the first investigate call"
            ),
            InvestigateError::PreregMismatch { field, recorded, provided } => write!(
                f,
                "--{field} ({provided}) does not match the track's recorded pre-registration ({recorded})"
            ),
            InvestigateError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            InvestigateError::Scoring(e) => write!(f, "could not score the attempt: {e}"),
            InvestigateError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InvestigateError {}

impl From<ParseError> for InvestigateError {
    fn from(e: ParseError) -> Self {
        InvestigateError::Scoring(e)
    }
}

impl From<zorp_track::TrackError> for InvestigateError {
    fn from(e: zorp_track::TrackError) -> Self {
        InvestigateError::Track(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(InvestigateError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_prereg_required_names_the_missing_flag() {
        let e = InvestigateError::PreregRequired { missing: "metric-name" };
        assert!(e.to_string().contains("--metric-name"));
    }

    #[test]
    fn display_prereg_mismatch_names_both_values() {
        let e = InvestigateError::PreregMismatch {
            field: "kill-threshold",
            recorded: "100".to_string(),
            provided: "50".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("100") && s.contains("50"));
    }

    #[test]
    fn from_parse_error_wraps_correctly() {
        let e: InvestigateError = ParseError::NoFencedBlock.into();
        assert!(matches!(e, InvestigateError::Scoring(ParseError::NoFencedBlock)));
    }
}
```

- [ ] **Step 2: Confirm it compiles once wired (deferred, same as Task 2)**

As in Task 2, this file cannot be exercised standalone until `mod investigate;` exists (Task 6). Proceed to Task 4.

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/investigate/error.rs
git commit -m "feat(zorp-agent): add InvestigateError"
```

---

### Task 4: `zorp-agent`: `investigate::run` orchestration

**Files:**
- Create: `zorp-agent/src/investigate/mod.rs`
- Test: `zorp-agent/src/investigate/mod.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `investigate::result::{parse_attempt_result, AttemptResult, ParseError}` (Task 2), `investigate::error::InvestigateError` (Task 3), `crate::agent::{Agent, Outcome}`, `zorp_track::{Project, checkpoint::CheckpointMode, experiment::{ExperimentStatus, MetricValue}, prereg::{write_prereg, get_preregistration}, track::TrackStatus}`.
- Produces: `pub struct PreregParams<'a> { pub metric_name: &'a str, pub kill_threshold: f64 }`, `pub fn run(agent: &mut Agent, project: &Project, track_id: &str, hypothesis: &str, prereg_params: Option<PreregParams>, checkpoint_mode: &CheckpointMode) -> Result<bool, InvestigateError>` — later tasks (main.rs wiring, Task 5) call this exactly as `validate::run` is called from the `validate` handler today.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/investigate/mod.rs`:

```rust
mod error;
mod result;

pub use error::InvestigateError;
pub use result::{parse_attempt_result, AttemptResult, ParseError};

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::{get_preregistration, write_prereg};
use zorp_track::track::TrackStatus;
use zorp_track::Project;

const TASK_PROMPT_PREFIX: &str = "\
Work the following hypothesis using whatever tools are available to you. \
When you're done, report the value of the metric named '";

const TASK_PROMPT_SUFFIX: &str = "\
' that your work produced.\n\n\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{\"metric_value\": <number>, \"summary\": \"<one sentence>\"}\n\
```\n\n\
Hypothesis: ";

/// Parameters a caller supplies only on the first `run` call for a
/// track, when no pre-registration exists yet.
pub struct PreregParams<'a> {
    pub metric_name: &'a str,
    pub kill_threshold: f64,
}

/// Run one investigate attempt for `track_id`. On the first call for a
/// track (no prereg on file), `prereg_params` must be `Some` and is
/// written, checkpointed, and (if approved) used for this same attempt.
/// On a later call, `prereg_params` is optional; if given, it must match
/// the recorded prereg exactly. Returns whether the post-attempt
/// checkpoint was approved (mirrors `validate::run`'s `Result<bool, _>`
/// shape); a rejected *prereg* checkpoint also returns `Ok(false)`, with
/// no attempt run.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    prereg_params: Option<PreregParams>,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, InvestigateError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(InvestigateError::TrackKilled);
    }

    let existing = get_preregistration(&project.store, track_id)?;
    let prereg = match (existing, prereg_params) {
        (Some(existing), None) => existing,
        (Some(existing), Some(params)) => {
            if existing.metric_name != params.metric_name {
                return Err(InvestigateError::PreregMismatch {
                    field: "metric-name",
                    recorded: existing.metric_name,
                    provided: params.metric_name.to_string(),
                });
            }
            if existing.kill_threshold != params.kill_threshold {
                return Err(InvestigateError::PreregMismatch {
                    field: "kill-threshold",
                    recorded: existing.kill_threshold.to_string(),
                    provided: params.kill_threshold.to_string(),
                });
            }
            existing
        }
        (None, None) => return Err(InvestigateError::PreregRequired { missing: "metric-name and --kill-threshold" }),
        (None, Some(params)) => {
            let track_dir = project.track_dir(track_id);
            let written = write_prereg(
                &project.store,
                &track_dir,
                track_id,
                hypothesis,
                params.metric_name,
                params.kill_threshold,
            )?;
            let prereg_prompt = format!(
                "investigate: pre-register metric '{}' with kill threshold {}. Hypothesis: {}\nProceed to run the first attempt?",
                written.metric_name, written.kill_threshold, hypothesis
            );
            let approved = project.store.record_checkpoint(track_id, "investigate-prereg", checkpoint_mode, &prereg_prompt)?;
            if !approved {
                project.store.set_track_status(track_id, TrackStatus::Killed)?;
                return Ok(false);
            }
            written
        }
    };

    let experiment = project.store.create_experiment(track_id, &prereg.id)?;
    project.store.set_experiment_status(&experiment.id, ExperimentStatus::Running)?;

    let task = format!("{TASK_PROMPT_PREFIX}{}{TASK_PROMPT_SUFFIX}{hypothesis}", prereg.metric_name);
    let outcome = agent.run(&task);
    let text = match outcome {
        Outcome::Complete(text) => text,
        Outcome::StepLimit => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("StepLimit".to_string()));
        }
        Outcome::VerificationFailed { attempts } => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome(format!("VerificationFailed after {attempts} attempts")));
        }
        Outcome::Cancelled => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("Cancelled".to_string()));
        }
        Outcome::RepeatedAction => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("RepeatedAction".to_string()));
        }
        Outcome::Blocked => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("Blocked".to_string()));
        }
        Outcome::Error(e) => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome(format!("Error: {e}")));
        }
    };

    let attempt = match parse_attempt_result(&text) {
        Ok(a) => a,
        Err(e) => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(e.into());
        }
    };

    project.store.record_metric(&experiment.id, &prereg.metric_name, MetricValue::Number(attempt.metric_value))?;
    project.store.set_experiment_status(&experiment.id, ExperimentStatus::Completed)?;

    let prompt = format!(
        "investigate: {} = {} (kill threshold {}). {}\nKeep this track alive?",
        prereg.metric_name, attempt.metric_value, prereg.kill_threshold, attempt.summary
    );
    let approved = project.store.record_checkpoint(track_id, "investigate", checkpoint_mode, &prompt)?;
    if !approved {
        project.store.set_track_status(track_id, TrackStatus::Killed)?;
    }

    Ok(approved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssistantMessage, Message, Model};
    use crate::BoxErr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

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

    fn well_formed_response() -> String {
        "Done.\n```json\n{\"metric_value\": 42.0, \"summary\": \"worked\"}\n```\n".to_string()
    }

    fn build_agent(response: String) -> Agent {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel { response, calls };
        Agent::new(
            Box::new(model),
            "system",
            5,
            std::env::temp_dir(),
            crate::cancel_token(),
            crate::ApprovalMode::AutoApprove,
        )
        .register_builtins()
    }

    #[test]
    fn killed_track_is_refused_before_creating_an_experiment() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        project.store.set_track_status("t1", TrackStatus::Killed).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", None, &mode).unwrap_err();
        assert!(matches!(err, InvestigateError::TrackKilled));
    }

    #[test]
    fn missing_prereg_params_on_first_call_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", None, &mode).unwrap_err();
        assert!(matches!(err, InvestigateError::PreregRequired { .. }));
    }

    #[test]
    fn mismatched_prereg_params_on_a_later_call_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
            &mode,
        )
        .unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams { metric_name: "latency_ms", kill_threshold: 50.0 }),
            &mode,
        )
        .unwrap_err();
        assert!(matches!(err, InvestigateError::PreregMismatch { field: "kill-threshold", .. }));
    }

    #[test]
    fn first_call_writes_prereg_runs_attempt_and_records_metric() {
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

        let prereg = get_preregistration(&project.store, "t1").unwrap().unwrap();
        assert_eq!(prereg.metric_name, "latency_ms");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p zorp-agent --features research investigate:: -- --nocapture`
Expected: FAIL — `mod investigate;` still isn't declared in `lib.rs` (Task 6), so this won't compile yet. Proceed to Task 5.

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/investigate/mod.rs
git commit -m "feat(zorp-agent): add investigate::run orchestration"
```

---

### Task 5: `zorp-agent`: wire `mod investigate;` into `lib.rs`

**Files:**
- Modify: `zorp-agent/src/lib.rs`

**Interfaces:**
- Consumes: `investigate` module (Tasks 2-4).
- Produces: `zorp_agent::investigate::{run, PreregParams, InvestigateError, AttemptResult, ParseError}` reachable from outside the crate (needed by `main.rs`, Task 6, and the integration test, Task 7).

- [ ] **Step 1: Add the module declaration**

In `zorp-agent/src/lib.rs`, immediately after the existing:

```rust
#[cfg(feature = "research")]
pub mod validate;
```

add:

```rust
#[cfg(feature = "research")]
pub mod investigate;
```

- [ ] **Step 2: Run the full research-feature test suite**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS — this is the first point where Tasks 2-4's tests actually compile and run. All tests from Task 1 (zorp-track), Task 2 (`investigate::result`), Task 3 (`investigate::error`), and Task 4 (`investigate::run`) must pass now. If any fail, fix `investigate/mod.rs`, `result.rs`, or `error.rs` before proceeding — do not move to Task 6 with failing tests.

- [ ] **Step 3: Confirm no leakage into default-feature builds**

Run: `cargo build -p zorp-agent` (no `--features research`)
Expected: succeeds, and `investigate` is not part of the public API surface in this build (it's entirely behind `#[cfg(feature = "research")]`, same as `validate`).

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/lib.rs
git commit -m "feat(zorp-agent): expose investigate module behind the research feature"
```

---

### Task 6: `zorp-agent`: `investigate` CLI subcommand in `main.rs`

**Files:**
- Modify: `zorp-agent/src/main.rs`
- Test: manual CLI smoke test (documented in Step 4 below); the orchestration logic itself is already unit-tested in Task 4.

**Interfaces:**
- Consumes: `zorp_agent::investigate::{run, PreregParams, InvestigateError}` (Task 5), the existing `get_or_create_track` helper (`main.rs:784`), the existing `Command` enum, `Overrides`, `HttpModel`, `Agent`, `attach_mcp_tools`, `zorp_track::Project`, `zorp_track::checkpoint::CheckpointMode`.
- Produces: `zorp-agent investigate "<hypothesis>" [--metric-name <name>] [--kill-threshold <n>]` as a working CLI command.

- [ ] **Step 1: Add the `Command::Investigate` variant**

In `zorp-agent/src/main.rs`, immediately after the existing:

```rust
    /// Validate whether a question is worth investigating.
    #[cfg(feature = "research")]
    Validate { question: String },
```

add:

```rust
    /// Run one staged, pre-registered investigate attempt against a track.
    #[cfg(feature = "research")]
    Investigate {
        question: String,
        #[arg(long = "metric-name")]
        metric_name: Option<String>,
        #[arg(long = "kill-threshold")]
        kill_threshold: Option<f64>,
    },
```

- [ ] **Step 2: Add the match arm**

Immediately after the existing:

```rust
        #[cfg(feature = "research")]
        Some(Command::Validate { question }) => validate(&question, cli.yes, &overrides),
```

add:

```rust
        #[cfg(feature = "research")]
        Some(Command::Investigate { question, metric_name, kill_threshold }) => {
            investigate(&question, metric_name, kill_threshold, cli.yes, &overrides)
        }
```

- [ ] **Step 3: Add the `investigate` handler function**

Immediately after the closing brace of the existing `fn validate(...)` (`main.rs:782`) and before `get_or_create_track`, add:

```rust
#[cfg(feature = "research")]
const INVESTIGATE_SYSTEM_PREAMBLE: &str = "\
You are running one staged attempt on a hypothesis that has already been \
pre-registered: a metric name and a kill threshold were committed before \
this attempt started and cannot be changed by you. Work the problem, then \
report the metric's actual value honestly, even if it misses the \
threshold.";

#[cfg(feature = "research")]
#[allow(clippy::too_many_arguments)]
fn investigate(
    question: &str,
    metric_name: Option<String>,
    kill_threshold: Option<f64>,
    auto_approve: bool,
    overrides: &Overrides,
) {
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let mut system = INVESTIGATE_SYSTEM_PREAMBLE.to_string();
    system.push_str("\n\n");
    system.push_str(&compose_system_with_persona(&cwd, persona(&cwd, &merged).as_deref()));
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key,
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated));

    agent = attach_mcp_tools(agent, overrides, true);

    let project = match zorp_track::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };
    let track_id = zorp_track::id::track_id(question);
    if let Err(e) = get_or_create_track(&project.store, &track_id, question) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    }
    let checkpoint_mode = match zorp_track::checkpoint::CheckpointMode::terminal(auto_approve) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };

    let prereg_params = match (metric_name.as_deref(), kill_threshold) {
        (Some(name), Some(threshold)) => Some(zorp_agent::investigate::PreregParams {
            metric_name: name,
            kill_threshold: threshold,
        }),
        (None, None) => None,
        _ => {
            eprintln!("zorp-agent: --metric-name and --kill-threshold must be given together");
            std::process::exit(2);
        }
    };

    match zorp_agent::investigate::run(&mut agent, &project, &track_id, question, prereg_params, &checkpoint_mode) {
        Ok(true) => println!("investigate: approved, track {track_id} stays active"),
        Ok(false) => println!("investigate: rejected, track {track_id} killed"),
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 4: Manual CLI smoke test**

Run: `cargo build -p zorp-agent --features research`
Expected: builds cleanly.

Run: `cargo run -p zorp-agent --features research -- investigate --help`
Expected: help text lists `investigate`, `--metric-name`, `--kill-threshold` as options, and running `cargo run -p zorp-agent --features research -- investigate "test hypothesis"` without `--metric-name`/`--kill-threshold` against a fresh temp track fails cleanly with an "InvestigateError::PreregRequired"-derived message (`--metric-name and --kill-threshold` in the error text) rather than a panic. (This requires `ZORP_BASE_URL`/`ZORP_API_KEY` to be set to reach the prereg-required check quickly since a `Project::open` happens first, or run inside a directory with no `.zorp` yet — either way, no panic is the pass condition.)

- [ ] **Step 5: Commit**

```bash
git add zorp-agent/src/main.rs
git commit -m "feat(zorp-agent): add investigate CLI subcommand"
```

---

### Task 7: `zorp-agent`: end-to-end integration test

**Files:**
- Create: `zorp-agent/tests/investigate_integration.rs`

**Interfaces:**
- Consumes: `zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model}`, `zorp_agent::investigate::{run, PreregParams, InvestigateError}`, `zorp_track::checkpoint::CheckpointMode`, `zorp_track::Project`.

- [ ] **Step 1: Write the integration test**

Create `zorp-agent/tests/investigate_integration.rs`:

```rust
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
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p zorp-agent --features research --test investigate_integration -- --nocapture`
Expected: PASS, all four tests.

- [ ] **Step 3: Run the full research-feature suite once more**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS, no regressions in `validate`'s existing tests.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/tests/investigate_integration.rs
git commit -m "test(zorp-agent): add investigate end-to-end integration test"
```

---

### Task 8: Documentation updates

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `AGENTS.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/DECISIONS.md`

**Interfaces:** None (documentation only).

- [ ] **Step 1: Update status lines**

In `CLAUDE.md` and `AGENTS.md`, find the "Status" section stating "The four capabilities that sit on top (validate, investigate, co-write, deliver) have not been built yet." Change to: "validate and investigate are built and tested; co-write and deliver have not been built yet."

- [ ] **Step 2: Update `docs/ARCHITECTURE.md`**

Add a line (following whatever pattern it already uses for validate's built status) noting `investigate` is built, pointing at `docs/superpowers/specs/2026-08-09-zorp-investigate-design.md`.

- [ ] **Step 3: Update `README.md`**

If `README.md` lists validate as an available/working subcommand (check its current capability list or usage examples section), add `investigate` alongside it with a one-line description and the `--metric-name`/`--kill-threshold` flags, following whatever format the validate entry already uses.

- [ ] **Step 4: Append a `docs/DECISIONS.md` entry**

Add a new entry at the top of the log (below the `---` separator, above the existing newest entry), following the file's established format (Decision / Why / Ruled out / Full writeup):

```markdown
## 2026-08-09: investigate's design: CLI-supplied prereg, one attempt per call, checkpoint decides kill

**Decision:** `investigate` takes `--metric-name`/`--kill-threshold` as CLI
arguments (not agent-proposed) the first time it runs for a track, writes
and checkpoints the pre-registration, then runs exactly one attempt per
invocation, records a typed metric via the existing `zorp-track`
experiment tables, and hands the kill/keep decision to a human checkpoint
rather than comparing the metric to the threshold in code.

**Why:** A human-committed threshold is the whole point of
pre-registration; an agent-proposed one would defeat it. One attempt per
invocation keeps every attempt visible at a checkpoint instead of burning
budget inside a single call before a human sees anything. No stored
"kill direction" (above/below is favorable) means no risk of that logic
guessing wrong; the checkpoint prompt shows the human the number and the
threshold and lets them decide, matching the existing "no hard experiment
budget" decision.

**Ruled out:** Multi-attempt loops within a single invocation. Automatic
threshold comparison deciding kill/keep without a human. Requiring a
prior `validate` approval before `investigate` can run (the existing
standalone-capabilities decision already rules this out).

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-investigate-design.md`
```

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md AGENTS.md docs/ARCHITECTURE.md docs/DECISIONS.md
git commit -m "docs: record investigate capability as built"
```
