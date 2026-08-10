# co-write Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build zorp's third capability, `co-write`: a new `zorp-agent co-write` subcommand that drafts a markdown artifact grounded in a track's recorded validate verdict and investigate metrics, writes it to `draft.md`, and checkpoints it for a human who becomes its author of record.

**Architecture:** A new `zorp-agent/src/co_write/` module (`mod.rs`, `error.rs`; no `result.rs`, the agent's answer is the draft directly), wired into `main.rs` as a new subcommand behind the `research` feature. One new small `zorp-track` read function (`Store::latest_checkpoint_time`) supports the mtime-warning heuristic; everything else needed (`get_validation`, `experiments_for`, `metrics_for`, `record_checkpoint`) already exists.

**Tech Stack:** Rust, `zorp-track` (DuckDB-backed `Store`), `zorp-agent`'s `Agent`/`Model`/`Outcome`.

## Global Constraints

- No new DuckDB schema changes; `checkpoints`, `experiments`, `metrics`, `validations` tables are reused as-is.
- `co_write::run`'s task prompt does NOT ask for a fenced JSON block; the agent's `Outcome::Complete(text)` is the draft's content verbatim, written to `draft.md`.
- `co_write::run` does NOT implement post-hoc numeric claim-checking against the drafted prose (grounding happens by only handing the agent real recorded numbers in the prompt, not by verifying its output afterward).
- Unlike `validate`/`investigate`, a rejected co-write checkpoint does NOT call `Store::set_track_status`; the track's status is untouched by either checkpoint outcome.
- `TrackError` is `#[non_exhaustive]`; any match on it needs a wildcard arm.
- `Project::track_dir(track_id)` only returns a path, it does not create the directory; callers must `fs::create_dir_all` it themselves before writing into it.
- `Store::get_validation(track_id)` returns `Result<Validation, TrackError>`, erroring `TrackError::NotFound { kind: "validation", .. }` when none exists; treat that specific error as "no validation" (optional), not a failure. Any other error variant propagates.
- Run `cargo test -p zorp-agent --features research` for zorp-agent changes, `cargo test -p zorp-track` for zorp-track changes.

---

### Task 1: `zorp-track`: read back a track's latest checkpoint time for a kind

**Files:**
- Modify: `zorp-track/src/checkpoint.rs`
- Test: `zorp-track/src/checkpoint.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub fn Store::latest_checkpoint_time(&self, track_id: &str, kind: &str) -> Result<Option<i64>, TrackError>` — returns the `resolved_at` of the most recent (`created_at DESC`) checkpoint row matching both `track_id` and `kind`, or `Ok(None)` if none exists.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block in `zorp-track/src/checkpoint.rs` (after the existing `record_checkpoint_persists_the_decision` test):

```rust
    #[test]
    fn latest_checkpoint_time_returns_none_when_absent() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        assert_eq!(store.latest_checkpoint_time("t1", "co-write").unwrap(), None);
    }

    #[test]
    fn latest_checkpoint_time_only_matches_the_given_kind() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::AutoApprove;
        store.record_checkpoint("t1", "validate", &mode, "novel?").unwrap();

        assert_eq!(store.latest_checkpoint_time("t1", "co-write").unwrap(), None);
    }

    #[test]
    fn latest_checkpoint_time_returns_the_most_recent_matching_row() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::AutoApprove;
        store.record_checkpoint("t1", "co-write", &mode, "draft 1 ready?").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        store.record_checkpoint("t1", "co-write", &mode, "draft 2 ready?").unwrap();

        let (latest_prompt,): (String,) = store
            .conn
            .query_row(
                "SELECT prompt_shown FROM checkpoints WHERE track_id = ? AND kind = ? ORDER BY created_at DESC LIMIT 1",
                duckdb::params!["t1", "co-write"],
                |r| Ok((r.get(0)?,)),
            )
            .unwrap();
        assert_eq!(latest_prompt, "draft 2 ready?");

        let time = store.latest_checkpoint_time("t1", "co-write").unwrap();
        assert!(time.is_some());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p zorp-track latest_checkpoint_time -- --nocapture`
Expected: FAIL with "cannot find function `latest_checkpoint_time` in this scope"

- [ ] **Step 3: Implement `latest_checkpoint_time`**

Add to `zorp-track/src/checkpoint.rs`, directly after `Store::record_checkpoint`'s closing brace (still inside `impl Store` if that's where it's defined — check the surrounding `impl` block; if `record_checkpoint` is the only method in that `impl Store` block, add this as a second method in the same block):

```rust
    /// Read back the `resolved_at` of the most recent checkpoint of
    /// `kind` for `track_id`, or `None` if no such checkpoint exists yet.
    /// Used by `co_write::run`'s mtime-warning heuristic, not for any
    /// integrity enforcement.
    pub fn latest_checkpoint_time(&self, track_id: &str, kind: &str) -> Result<Option<i64>, TrackError> {
        let row: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT resolved_at FROM checkpoints WHERE track_id = ? AND kind = ? ORDER BY created_at DESC LIMIT 1",
                duckdb::params![track_id, kind],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.flatten())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p zorp-track latest_checkpoint_time -- --nocapture`
Expected: PASS (all three), and `cargo test -p zorp-track` overall still passes.

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/checkpoint.rs
git commit -m "feat(zorp-track): add Store::latest_checkpoint_time"
```

---

### Task 2: `zorp-agent`: `co_write::error`

**Files:**
- Create: `zorp-agent/src/co_write/error.rs`
- Test: `zorp-agent/src/co_write/error.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub enum CoWriteError { TrackKilled, NoMetrics, AgentOutcome(String), Io(String), Track(zorp_track::TrackError) }`, `impl fmt::Display`, `impl std::error::Error`, `impl From<zorp_track::TrackError>`, `impl From<std::io::Error>`.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/co_write/error.rs`:

```rust
use std::fmt;

#[derive(Debug)]
pub enum CoWriteError {
    TrackKilled,
    NoMetrics,
    AgentOutcome(String),
    Io(String),
    Track(zorp_track::TrackError),
}

impl fmt::Display for CoWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoWriteError::TrackKilled => write!(f, "this track has already been killed"),
            CoWriteError::NoMetrics => write!(
                f,
                "this track has no recorded metrics yet; run investigate at least once before co-write"
            ),
            CoWriteError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            CoWriteError::Io(msg) => write!(f, "could not write draft.md: {msg}"),
            CoWriteError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CoWriteError {}

impl From<zorp_track::TrackError> for CoWriteError {
    fn from(e: zorp_track::TrackError) -> Self {
        CoWriteError::Track(e)
    }
}

impl From<std::io::Error> for CoWriteError {
    fn from(e: std::io::Error) -> Self {
        CoWriteError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(CoWriteError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_no_metrics_mentions_investigate() {
        assert!(CoWriteError::NoMetrics.to_string().contains("investigate"));
    }

    #[test]
    fn display_agent_outcome_includes_the_outcome() {
        let e = CoWriteError::AgentOutcome("StepLimit".to_string());
        assert!(e.to_string().contains("StepLimit"));
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: CoWriteError = io_err.into();
        assert!(matches!(e, CoWriteError::Io(_)));
    }
}
```

- [ ] **Step 2: Defer compilation to Task 4 (module wiring)**

This file cannot be exercised standalone until `zorp-agent/src/lib.rs` declares `mod co_write;` (Task 4). Create the file exactly as specified, do not touch `lib.rs`, and do not be alarmed that `cargo test` won't pick this up yet — that's expected. Proceed to Task 3.

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/co_write/error.rs
git commit -m "feat(zorp-agent): add CoWriteError"
```

---

### Task 3: `zorp-agent`: `co_write::run` orchestration

**Files:**
- Create: `zorp-agent/src/co_write/mod.rs`
- Test: `zorp-agent/src/co_write/mod.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `co_write::error::CoWriteError` (Task 2), `crate::agent::{Agent, Outcome}`, `zorp_track::{Project, checkpoint::CheckpointMode, experiment::MetricValue, track::TrackStatus, TrackError}`.
- Produces: `pub fn run(agent: &mut Agent, project: &Project, track_id: &str, hypothesis: &str, checkpoint_mode: &CheckpointMode) -> Result<bool, CoWriteError>`.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/co_write/mod.rs`:

```rust
mod error;

pub use error::CoWriteError;

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::MetricValue;
use zorp_track::track::TrackStatus;
use zorp_track::{Project, TrackError};
use std::fmt::Write as _;

const SYSTEM_PREAMBLE: &str = "\
You are drafting an evidence-based artifact from a research run record. \
Cite only the metric values and verdict given to you below; never invent \
a number. State confidence no higher than the evidence given supports.";

/// Collect every metric recorded across all of `track_id`'s experiments,
/// as `(experiment_id, metric_key, MetricValue)` triples, in experiment
/// order then metric order.
fn all_metrics(project: &Project, track_id: &str) -> Result<Vec<(String, String, MetricValue)>, TrackError> {
    let mut out = Vec::new();
    for experiment in project.store.experiments_for(track_id)? {
        for (key, value) in project.store.metrics_for(&experiment.id)? {
            out.push((experiment.id.clone(), key, value));
        }
    }
    Ok(out)
}

fn format_metric_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) => n.to_string(),
        MetricValue::Text(s) => s.clone(),
        MetricValue::Bool(b) => b.to_string(),
    }
}

fn build_task_prompt(hypothesis: &str, project: &Project, track_id: &str, metrics: &[(String, String, MetricValue)]) -> String {
    let mut task = format!("Hypothesis: {hypothesis}\n\n");

    match project.store.get_validation(track_id) {
        Ok(v) => {
            let _ = write!(
                task,
                "Validation verdict: {}\nRedundancy: {:.0}/100. Feasibility: {:.0}/100.\n\n",
                v.verdict, v.redundancy_score, v.feasibility_score
            );
        }
        Err(TrackError::NotFound { kind: "validation", .. }) => {}
        Err(_) => {}
    }

    task.push_str("Recorded metrics:\n");
    for (experiment_id, key, value) in metrics {
        let _ = writeln!(task, "- [{experiment_id}] {key} = {}", format_metric_value(value));
    }
    task.push_str("\nDraft a short evidence-based artifact (a decision memo or summary) based only on this data.");
    task
}

/// Run co-write for an already-created track with at least one recorded
/// metric: draft an artifact grounded in the track's evidence, write it
/// to `draft.md`, and checkpoint it. Returns whether the checkpoint was
/// approved. Unlike `validate::run`/`investigate::run`, neither outcome
/// changes the track's status.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, CoWriteError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(CoWriteError::TrackKilled);
    }

    let metrics = all_metrics(project, track_id)?;
    if metrics.is_empty() {
        return Err(CoWriteError::NoMetrics);
    }

    let task = build_task_prompt(hypothesis, project, track_id, &metrics);
    let outcome = agent.run(&task);
    let draft = match outcome {
        Outcome::Complete(text) => text,
        Outcome::StepLimit => return Err(CoWriteError::AgentOutcome("StepLimit".to_string())),
        Outcome::VerificationFailed { attempts } => {
            return Err(CoWriteError::AgentOutcome(format!("VerificationFailed after {attempts} attempts")))
        }
        Outcome::Cancelled => return Err(CoWriteError::AgentOutcome("Cancelled".to_string())),
        Outcome::RepeatedAction => return Err(CoWriteError::AgentOutcome("RepeatedAction".to_string())),
        Outcome::Blocked => return Err(CoWriteError::AgentOutcome("Blocked".to_string())),
        Outcome::Error(e) => return Err(CoWriteError::AgentOutcome(format!("Error: {e}"))),
    };

    let track_dir = project.track_dir(track_id);
    std::fs::create_dir_all(&track_dir)?;
    let draft_path = track_dir.join("draft.md");

    if let Ok(meta) = std::fs::metadata(&draft_path) {
        if let Ok(modified) = meta.modified() {
            let mtime_millis = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if let Some(last_checkpoint) = project.store.latest_checkpoint_time(track_id, "co-write")? {
                if mtime_millis > last_checkpoint {
                    eprintln!("zorp-agent: draft.md appears to have been edited since it was last generated by co-write");
                }
            }
        }
    }

    std::fs::write(&draft_path, &draft)?;

    let prompt = format!(
        "co-write: draft written to {} ({} lines, {} metrics). Ready for review?",
        draft_path.display(),
        draft.lines().count(),
        metrics.len()
    );
    let approved = project.store.record_checkpoint(track_id, "co-write", checkpoint_mode, &prompt)?;

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
    use zorp_track::experiment::ExperimentStatus;

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

    fn build_agent(response: &str) -> Agent {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel { response: response.to_string(), calls };
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

    fn track_with_one_metric(project: &Project, track_id: &str) {
        project.store.create_track(track_id, "does caching help").unwrap();
        let exp = project.store.create_experiment(track_id, "no-prereg").unwrap();
        project.store.set_experiment_status(&exp.id, ExperimentStatus::Completed).unwrap();
        project.store.record_metric(&exp.id, "latency_ms", MetricValue::Number(42.0)).unwrap();
    }

    #[test]
    fn killed_track_is_refused() {
        let mut agent = build_agent("a draft");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_one_metric(&project, "t1");
        project.store.set_track_status("t1", TrackStatus::Killed).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, CoWriteError::TrackKilled));
    }

    #[test]
    fn no_metrics_is_refused() {
        let mut agent = build_agent("a draft");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, CoWriteError::NoMetrics));
    }

    #[test]
    fn full_run_writes_draft_and_checkpoints() {
        let mut agent = build_agent("Latency improved to 42ms.");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_one_metric(&project, "t1");
        let mode = CheckpointMode::terminal(true).unwrap();

        let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
        assert!(approved);

        let draft_path = project.track_dir("t1").join("draft.md");
        let content = std::fs::read_to_string(&draft_path).unwrap();
        assert_eq!(content, "Latency improved to 42ms.");
    }

    #[test]
    fn rejected_checkpoint_does_not_kill_the_track() {
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

    struct RejectAll;
    impl zorp_track::checkpoint::Decider for RejectAll {
        fn decide(&self, _prompt: &str) -> bool {
            false
        }
    }
}
```

Note: `create_experiment` in the tests above passes `"no-prereg"` as a placeholder `prereg_id` — `create_experiment` does not validate that a `preregistrations` row exists for that id (it only validates the track exists, see `zorp-track/src/experiment.rs`'s `create_experiment`), so this is safe for a test fixture and does not require actually calling `write_prereg` first.

- [ ] **Step 2: Defer compilation to Task 4**

Same as Task 2: `mod co_write;` isn't declared in `lib.rs` yet. Create the file exactly as specified, do not touch `lib.rs`. Proceed to Task 4.

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/co_write/mod.rs
git commit -m "feat(zorp-agent): add co_write::run orchestration"
```

---

### Task 4: `zorp-agent`: wire `mod co_write;` into `lib.rs`

**Files:**
- Modify: `zorp-agent/src/lib.rs`

**Interfaces:**
- Consumes: `co_write` module (Tasks 2-3).
- Produces: `zorp_agent::co_write::{run, CoWriteError}` reachable from outside the crate.

- [ ] **Step 1: Add the module declaration**

In `zorp-agent/src/lib.rs`, immediately after the existing:

```rust
#[cfg(feature = "research")]
pub mod investigate;
```

add:

```rust
#[cfg(feature = "research")]
pub mod co_write;
```

- [ ] **Step 2: Run the full research-feature test suite**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS — every test from Task 1 (zorp-track), Task 2 (`co_write::error`), and Task 3 (`co_write::mod`) must pass now, on top of all prior passing tests. If anything fails, fix `co_write/mod.rs` or `error.rs` before proceeding.

- [ ] **Step 3: Confirm no leakage into default-feature builds**

Run: `cargo build -p zorp-agent` (no `--features research`)
Expected: succeeds, `co_write` not part of that build's surface.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/lib.rs
git commit -m "feat(zorp-agent): expose co_write module behind the research feature"
```

---

### Task 5: `zorp-agent`: `co-write` CLI subcommand in `main.rs`

**Files:**
- Modify: `zorp-agent/src/main.rs`

**Interfaces:**
- Consumes: `zorp_agent::co_write::{run, CoWriteError}` (Task 4), the existing `get_or_create_track` helper.
- Produces: `zorp-agent co-write "<hypothesis>"` as a working CLI command.

- [ ] **Step 1: Add the `Command::CoWrite` variant**

In `zorp-agent/src/main.rs`, read the current `Command` enum first to find the exact current position of the `Investigate` variant (this plan is written after investigate shipped, so it's already there), and add immediately after it:

```rust
    /// Draft an artifact from a track's recorded evidence.
    #[cfg(feature = "research")]
    CoWrite { question: String },
```

- [ ] **Step 2: Add the match arm**

Immediately after `Investigate`'s match arm, add:

```rust
        #[cfg(feature = "research")]
        Some(Command::CoWrite { question }) => co_write(&question, cli.yes, &overrides),
```

- [ ] **Step 3: Add the `co_write` handler function**

Immediately after `fn investigate(...)`'s closing brace and before `get_or_create_track`, add (mirroring `fn investigate`'s structure exactly, minus the prereg-params parsing since co-write has none):

```rust
#[cfg(feature = "research")]
const CO_WRITE_SYSTEM_PREAMBLE: &str = "\
You are drafting an evidence-based artifact from a research run record. \
Cite only the metric values and verdict given to you; never invent a \
number. State confidence no higher than the evidence given supports.";

#[cfg(feature = "research")]
fn co_write(question: &str, auto_approve: bool, overrides: &Overrides) {
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
    let mut system = CO_WRITE_SYSTEM_PREAMBLE.to_string();
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

    match zorp_agent::co_write::run(&mut agent, &project, &track_id, question, &checkpoint_mode) {
        Ok(true) => println!("co-write: approved, draft ready for review at .zorp/tracks/{track_id}/draft.md"),
        Ok(false) => println!("co-write: not yet approved, draft left at .zorp/tracks/{track_id}/draft.md"),
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

Run: `cargo run -p zorp-agent --features research -- co-write --help`
Expected: help text lists `co-write`.

Run `cargo run -p zorp-agent --features research -- co-write "test hypothesis"` in a fresh temp directory (no prior track): expected to fail cleanly (no panic) with a `CoWriteError::NoMetrics`-derived message, since a freshly created track has no metrics yet.

- [ ] **Step 5: Run the full suite once more**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS, no regressions.

- [ ] **Step 6: Commit**

```bash
git add zorp-agent/src/main.rs
git commit -m "feat(zorp-agent): add co-write CLI subcommand"
```

---

### Task 6: `zorp-agent`: end-to-end integration test

**Files:**
- Create: `zorp-agent/tests/co_write_integration.rs`

**Interfaces:**
- Consumes: `zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model}`, `zorp_agent::co_write::{run, CoWriteError}`, `zorp_track::checkpoint::CheckpointMode`, `zorp_track::experiment::{ExperimentStatus, MetricValue}`, `zorp_track::track::TrackStatus`, `zorp_track::Project`.

- [ ] **Step 1: Write the integration test**

Create `zorp-agent/tests/co_write_integration.rs`:

```rust
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

struct RejectAll;
impl zorp_track::checkpoint::Decider for RejectAll {
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

fn track_with_one_metric(project: &Project, track_id: &str) {
    project.store.create_track(track_id, "does caching help").unwrap();
    let exp = project.store.create_experiment(track_id, "no-prereg").unwrap();
    project.store.set_experiment_status(&exp.id, ExperimentStatus::Completed).unwrap();
    project.store.record_metric(&exp.id, "latency_ms", MetricValue::Number(42.0)).unwrap();
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
fn a_second_call_overwrites_the_draft_and_still_succeeds() {
    let mut agent = build_agent("first draft");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_one_metric(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();

    let mut agent2 = build_agent("second draft, overwritten");
    let approved = run(&mut agent2, &project, "t1", "does caching help", &mode).unwrap();
    assert!(approved);

    let draft_path = project.track_dir("t1").join("draft.md");
    let content = std::fs::read_to_string(&draft_path).unwrap();
    assert_eq!(content, "second draft, overwritten");
}

#[test]
fn no_metrics_refuses_before_running_the_agent() {
    let mut agent = build_agent("a draft");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
    assert!(matches!(err, CoWriteError::NoMetrics));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p zorp-agent --features research --test co_write_integration -- --nocapture`
Expected: PASS, all four tests.

- [ ] **Step 3: Run the full research-feature suite once more**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS, no regressions in validate's or investigate's existing tests.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/tests/co_write_integration.rs
git commit -m "test(zorp-agent): add co-write end-to-end integration test"
```

---

### Task 7: Documentation updates

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `AGENTS.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/DECISIONS.md`

**Interfaces:** None (documentation only).

- [ ] **Step 1: Update status lines**

In `CLAUDE.md` and `AGENTS.md`, find the "Status" section (currently reading "validate and investigate are built and tested; co-write and deliver have not been built yet") and change to: "validate, investigate, and co-write are built and tested; deliver has not been built yet."

- [ ] **Step 2: Update `docs/ARCHITECTURE.md`**

Add a line for `co-write`'s built status, following whatever pattern the file already uses for validate/investigate, pointing at `docs/superpowers/specs/2026-08-09-zorp-co-write-design.md`.

- [ ] **Step 3: Update `README.md`**

Update the status banner, the "Why zorp" closing sentence, and the roadmap checklist (check off `co-write`), following the same pattern investigate's equivalent update already used (validate and investigate are checked; co-write should be checked now too, deliver stays unchecked). Do not reintroduce "sandboxed" or any other overclaim into the co-write line; describe it accurately as drafting from recorded evidence with a human as author of record.

- [ ] **Step 4: Append a `docs/DECISIONS.md` entry**

Add a new entry at the top of the log (below the `---` separator, above the existing newest entry), following the file's established format:

```markdown
## 2026-08-09: co-write's design: grounded drafting, no post-hoc claim-check, rejection doesn't kill the track

**Decision:** `co-write` hands the agent the track's actual recorded
evidence (validate's verdict if present, every metric investigate
recorded) as structured data in the prompt and instructs it to cite only
those figures, rather than drafting freely and then scanning the output
to verify numeric claims afterward. Requires at least one recorded
metric to run at all. The agent's answer is written directly to
`draft.md`, no scored JSON block. Unlike validate and investigate,
rejecting co-write's checkpoint does not kill the track: a draft not
being ready isn't evidence the investigation failed.

**Why:** Grounding at the input side (only real numbers ever reach the
model) is simpler and more reliable than extracting and re-verifying
numeric claims from free-form prose after the fact, which is a much
harder problem with its own false-positive/negative risk. Requiring a
metric to exist keeps co-write from drafting off a validate pass alone,
which is a go/no-go check, not evidence. Not killing the track on
rejection matches the normal expected path once a draft exists: a human
takes over editing `draft.md` directly, or the call runs again.

**Ruled out:** A post-hoc claim-check pass over the drafted prose.
Tamper-evidence hashing of `draft.md` like `prereg.md`'s SHA-256 (a
mtime-based warning only, not an integrity guarantee). Killing the track
on a rejected co-write checkpoint.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-co-write-design.md`
```

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md AGENTS.md docs/ARCHITECTURE.md docs/DECISIONS.md
git commit -m "docs: record co-write capability as built"
```
