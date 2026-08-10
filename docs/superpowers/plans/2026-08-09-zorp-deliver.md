# deliver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build zorp's fourth and last capability, `deliver`: a new `zorp-agent deliver` subcommand that matches a track's co-written draft against real academic venues via the huiban MCP server, writes a ranked shortlist to `venues.md`, and checkpoints it.

**Architecture:** A new `zorp-agent/src/deliver/` module (`mod.rs`, `error.rs`; no `result.rs`, same reasoning as `co_write`), wired into `main.rs` as a new subcommand behind the `research` feature. No `zorp-track` changes needed this time; everything required (`get_track`, `record_checkpoint`) already exists.

**Tech Stack:** Rust, `zorp-track` (DuckDB-backed `Store`), `zorp-agent`'s `Agent`/`Model`/`Outcome`, the huiban MCP server (external, real in production; stubbed in tests the same way `validate`'s test stubs a search server).

## Global Constraints

- No new DuckDB schema changes.
- `deliver::run`'s task prompt does NOT ask for a fenced JSON block; the agent's `Outcome::Complete(text)` is the shortlist's content verbatim, written to `venues.md`.
- Unlike `validate`/`investigate`, a rejected `deliver` checkpoint does NOT call `Store::set_track_status` — same posture as `co_write`.
- The huiban gate checks `agent.tool_names()` for anything starting with `"mcp__huiban__"`, mirroring `validate`'s `has_search_tool` pattern exactly (`agent.tool_names().iter().any(|n| n.starts_with("mcp__huiban__"))`).
- `mcp__<name>__<tool>`-prefixing is generic in `zorp-mcp` (`zorp-mcp/src/protocol.rs`, `format!("mcp__{server}__{tool}")`, `server` taken straight from the config's `[[server]] name = "..."` field); a test's stub `McpConfig` with `name = "huiban"` genuinely produces `mcp__huiban__*`-prefixed tool names with no huiban-specific code needed anywhere. The existing `zorp-agent/tests/fixtures/stub_search_mcp_server.rs` binary can be reused as-is for this, only the test's own `McpConfig` toml string needs `name = "huiban"` instead of `name = "stub"`.
- Run `cargo test -p zorp-agent --features research` for all zorp-agent changes in this plan.

---

### Task 1: `zorp-agent`: `deliver::error`

**Files:**
- Create: `zorp-agent/src/deliver/error.rs`
- Test: `zorp-agent/src/deliver/error.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub enum DeliverError { TrackKilled, NoDraft, NoVenueTool, AgentOutcome(String), Io(String), Track(zorp_track::TrackError) }`, `impl fmt::Display`, `impl std::error::Error`, `impl From<zorp_track::TrackError>`, `impl From<std::io::Error>`.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/deliver/error.rs`:

```rust
use std::fmt;

#[derive(Debug)]
pub enum DeliverError {
    TrackKilled,
    NoDraft,
    NoVenueTool,
    AgentOutcome(String),
    Io(String),
    Track(zorp_track::TrackError),
}

impl fmt::Display for DeliverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeliverError::TrackKilled => write!(f, "this track has already been killed"),
            DeliverError::NoDraft => write!(
                f,
                "this track has no draft.md yet; run co-write at least once before deliver"
            ),
            DeliverError::NoVenueTool => write!(
                f,
                "no huiban-prefixed tool is available; configure the huiban MCP server (--mcp or .zorp/mcp.toml)"
            ),
            DeliverError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            DeliverError::Io(msg) => write!(f, "could not write venues.md: {msg}"),
            DeliverError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DeliverError {}

impl From<zorp_track::TrackError> for DeliverError {
    fn from(e: zorp_track::TrackError) -> Self {
        DeliverError::Track(e)
    }
}

impl From<std::io::Error> for DeliverError {
    fn from(e: std::io::Error) -> Self {
        DeliverError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(DeliverError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_no_draft_mentions_co_write() {
        assert!(DeliverError::NoDraft.to_string().contains("co-write"));
    }

    #[test]
    fn display_no_venue_tool_mentions_huiban() {
        assert!(DeliverError::NoVenueTool.to_string().contains("huiban"));
    }

    #[test]
    fn display_agent_outcome_includes_the_outcome() {
        let e = DeliverError::AgentOutcome("StepLimit".to_string());
        assert!(e.to_string().contains("StepLimit"));
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: DeliverError = io_err.into();
        assert!(matches!(e, DeliverError::Io(_)));
    }
}
```

- [ ] **Step 2: Defer compilation to Task 3 (module wiring)**

This file cannot be exercised standalone until `zorp-agent/src/lib.rs` declares `mod deliver;` (Task 3). Create the file exactly as specified, do not touch `lib.rs`, and do not be alarmed that `cargo test` won't pick this up yet — that's expected. Proceed to Task 2.

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/deliver/error.rs
git commit -m "feat(zorp-agent): add DeliverError"
```

---

### Task 2: `zorp-agent`: `deliver::run` orchestration

**Files:**
- Create: `zorp-agent/src/deliver/mod.rs`
- Test: `zorp-agent/src/deliver/mod.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `deliver::error::DeliverError` (Task 1), `crate::agent::{Agent, Outcome}`, `zorp_track::{Project, checkpoint::CheckpointMode, track::TrackStatus}`.
- Produces: `pub fn run(agent: &mut Agent, project: &Project, track_id: &str, hypothesis: &str, checkpoint_mode: &CheckpointMode) -> Result<bool, DeliverError>`.

- [ ] **Step 1: Write the failing tests**

Create `zorp-agent/src/deliver/mod.rs`:

```rust
mod error;

pub use error::DeliverError;

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

fn has_huiban_tool(agent: &Agent) -> bool {
    agent.tool_names().iter().any(|n| n.starts_with("mcp__huiban__"))
}

fn build_task_prompt(hypothesis: &str, draft: &str) -> String {
    format!(
        "Determine this draft's scope and contribution type, then use the \
         available huiban tools to search for real conferences and journals \
         that fit. Rank the candidates you find, including each one's \
         deadline and ranking (CCF/CORE) where available.\n\n\
         Hypothesis: {hypothesis}\n\n\
         Draft:\n{draft}"
    )
}

/// Run deliver for a track that already has a co-written draft: find
/// real venues via huiban, write a ranked shortlist to `venues.md`, and
/// checkpoint it. Returns whether the checkpoint was approved. Like
/// `co_write::run`, neither outcome changes the track's status.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, DeliverError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(DeliverError::TrackKilled);
    }

    let draft_path = project.track_dir(track_id).join("draft.md");
    let draft = std::fs::read_to_string(&draft_path).map_err(|_| DeliverError::NoDraft)?;

    if !has_huiban_tool(agent) {
        return Err(DeliverError::NoVenueTool);
    }

    let task = build_task_prompt(hypothesis, &draft);
    let outcome = agent.run(&task);
    let shortlist = match outcome {
        Outcome::Complete(text) => text,
        Outcome::StepLimit => return Err(DeliverError::AgentOutcome("StepLimit".to_string())),
        Outcome::VerificationFailed { attempts } => {
            return Err(DeliverError::AgentOutcome(format!("VerificationFailed after {attempts} attempts")))
        }
        Outcome::Cancelled => return Err(DeliverError::AgentOutcome("Cancelled".to_string())),
        Outcome::RepeatedAction => return Err(DeliverError::AgentOutcome("RepeatedAction".to_string())),
        Outcome::Blocked => return Err(DeliverError::AgentOutcome("Blocked".to_string())),
        Outcome::Error(e) => return Err(DeliverError::AgentOutcome(format!("Error: {e}"))),
    };

    let track_dir = project.track_dir(track_id);
    std::fs::create_dir_all(&track_dir)?;
    let venues_path = track_dir.join("venues.md");
    std::fs::write(&venues_path, &shortlist)?;

    let prompt = format!(
        "deliver: shortlist written to {} ({} lines). Ready for review?",
        venues_path.display(),
        shortlist.lines().count()
    );
    let approved = project.store.record_checkpoint(track_id, "deliver", checkpoint_mode, &prompt)?;

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

    fn track_with_draft(project: &Project, track_id: &str) {
        project.store.create_track(track_id, "does caching help").unwrap();
        let track_dir = project.track_dir(track_id);
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(track_dir.join("draft.md"), "# Draft\n\nLatency improved.").unwrap();
    }

    #[test]
    fn killed_track_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        project.store.set_track_status("t1", TrackStatus::Killed).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, DeliverError::TrackKilled));
    }

    #[test]
    fn no_draft_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, DeliverError::NoDraft));
    }

    #[test]
    fn no_huiban_tool_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        let mode = CheckpointMode::terminal(true).unwrap();
        // No MCP tools attached: only built-in local tools are present.

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, DeliverError::NoVenueTool));
    }
}
```

Note: `no_huiban_tool_is_refused` cannot actually be satisfied by this test as written, since it never attaches any MCP tool at all, so `has_huiban_tool` correctly returns `false` and the test passes without needing a real or stub huiban tool. This is intentional and correct: this unit test only proves the gate fires when no huiban tool exists; a *positive* case (huiban tool present, gate passes, agent actually runs) is covered by Task 5's integration test, which attaches a stub MCP tool named `huiban`.

- [ ] **Step 2: Defer compilation to Task 3**

Same as Task 1: `mod deliver;` isn't declared in `lib.rs` yet. Create the file exactly as specified, do not touch `lib.rs`. Proceed to Task 3.

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/deliver/mod.rs
git commit -m "feat(zorp-agent): add deliver::run orchestration"
```

---

### Task 3: `zorp-agent`: wire `mod deliver;` into `lib.rs`

**Files:**
- Modify: `zorp-agent/src/lib.rs`

**Interfaces:**
- Consumes: `deliver` module (Tasks 1-2).
- Produces: `zorp_agent::deliver::{run, DeliverError}` reachable from outside the crate.

- [ ] **Step 1: Add the module declaration**

In `zorp-agent/src/lib.rs`, immediately after the existing:

```rust
#[cfg(feature = "research")]
pub mod co_write;
```

add:

```rust
#[cfg(feature = "research")]
pub mod deliver;
```

- [ ] **Step 2: Run the full research-feature test suite**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS — every test from Task 1 (`deliver::error`, 5 tests) and Task 2 (`deliver::mod`, 3 tests) must pass now, on top of all prior passing tests (430 as of co-write's last task). If anything fails, fix `deliver/mod.rs` or `error.rs` before proceeding.

- [ ] **Step 3: Confirm no leakage into default-feature builds**

Run: `cargo build -p zorp-agent` (no `--features research`)
Expected: succeeds, `deliver` not part of that build's surface.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/lib.rs
git commit -m "feat(zorp-agent): expose deliver module behind the research feature"
```

---

### Task 4: `zorp-agent`: `deliver` CLI subcommand in `main.rs`

**Files:**
- Modify: `zorp-agent/src/main.rs`

**Interfaces:**
- Consumes: `zorp_agent::deliver::{run, DeliverError}` (Task 3), the existing `get_or_create_track` helper.
- Produces: `zorp-agent deliver "<hypothesis>"` as a working CLI command.

- [ ] **Step 1: Add the `Command::Deliver` variant**

In `zorp-agent/src/main.rs`, read the current `Command` enum first to confirm `CoWrite` is still the last variant (it was as of co-write's plan), and add immediately after it:

```rust
    /// Match a co-written draft against real venues.
    #[cfg(feature = "research")]
    Deliver { question: String },
```

- [ ] **Step 2: Add the match arm**

Immediately after `CoWrite`'s match arm, add:

```rust
        #[cfg(feature = "research")]
        Some(Command::Deliver { question }) => deliver(&question, cli.yes, &overrides),
```

- [ ] **Step 3: Add the `deliver` handler function**

Immediately after `fn co_write(...)`'s closing brace and before `get_or_create_track`, add (mirroring `fn co_write`'s structure exactly):

```rust
#[cfg(feature = "research")]
const DELIVER_SYSTEM_PREAMBLE: &str = "\
You are matching a finished draft against real academic venues using \
the tools available to you. Only report venues you actually found \
through those tools; never invent a conference or journal name.";

#[cfg(feature = "research")]
fn deliver(question: &str, auto_approve: bool, overrides: &Overrides) {
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
    let mut system = DELIVER_SYSTEM_PREAMBLE.to_string();
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

    match zorp_agent::deliver::run(&mut agent, &project, &track_id, question, &checkpoint_mode) {
        Ok(true) => println!("deliver: approved, shortlist ready for review at .zorp/tracks/{track_id}/venues.md"),
        Ok(false) => println!("deliver: not yet approved, shortlist left at .zorp/tracks/{track_id}/venues.md"),
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

Run: `cargo run -p zorp-agent --bin zorp-agent --features research -- deliver --help`
Expected: help text lists `deliver`.

Run `cargo run -p zorp-agent --bin zorp-agent --features research -- deliver "test hypothesis"` in a fresh temp directory (no prior co-write draft): expected to fail cleanly (no panic) with a `NoDraft`-derived message, since a freshly created track has no `draft.md` yet.

- [ ] **Step 5: Run the full suite once more**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS, no regressions.

- [ ] **Step 6: Commit**

```bash
git add zorp-agent/src/main.rs
git commit -m "feat(zorp-agent): add deliver CLI subcommand"
```

---

### Task 5: `zorp-agent`: end-to-end integration test

**Files:**
- Create: `zorp-agent/tests/deliver_integration.rs`

**Interfaces:**
- Consumes: `zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model, ToolCall, mcp_adapter::McpToolAdapter}`, `zorp_agent::deliver::{run, DeliverError}`, `zorp_mcp::{McpConfig, McpRegistry}`, `zorp_track::checkpoint::CheckpointMode`, `zorp_track::track::TrackStatus`, `zorp_track::Project`.

- [ ] **Step 1: Write the integration test**

Create `zorp-agent/tests/deliver_integration.rs`:

```rust
//! End-to-end integration test for `deliver::run`, using the same stub
//! MCP server binary `validate_integration.rs` already uses
//! (`tests/fixtures/stub_search_mcp_server.rs`), configured under the
//! server name `huiban` so its tools come out prefixed
//! `mcp__huiban__*` — the exact gate `deliver::run` checks for. No real
//! huiban-specific code is needed anywhere: `mcp__<name>__<tool>`
//! prefixing is generic in `zorp-mcp`, driven entirely by the config's
//! `name` field.
#![cfg(feature = "research")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use zorp_agent::deliver::{run, DeliverError};
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model, ToolCall};
use zorp_mcp::{McpConfig, McpRegistry};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

struct StubModel {
    response: String,
    search_tool_name: String,
    calls: Arc<AtomicUsize>,
}

impl Model for StubModel {
    fn complete(&self, _messages: &[Message], _tools: &[serde_json::Value]) -> Result<AssistantMessage, BoxErr> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            Ok(AssistantMessage {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "1".to_string(),
                    name: self.search_tool_name.clone(),
                    arguments: serde_json::json!({ "query": "does caching help" }),
                }],
                finish_reason: "tool_calls".to_string(),
                reasoning_content: None,
            })
        } else {
            Ok(AssistantMessage {
                content: self.response.clone(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(StubModel {
            response: self.response.clone(),
            search_tool_name: self.search_tool_name.clone(),
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

fn stub_server_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stub_search_mcp_server"))
}

fn build_agent_with_huiban_stub(response: &str) -> (Agent, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let config = McpConfig::from_toml_str(&format!(
        "[[server]]\nname = \"huiban\"\ntransport = \"stdio\"\ncommand = \"{}\"\ntrust = \"sandbox\"\n",
        stub_server_binary().display()
    ))
    .unwrap();
    let mut registry = McpRegistry::new(config);
    let tools = registry.discover();
    let search_tool_name = tools
        .iter()
        .find(|t| t.name == "search")
        .expect("stub server should advertise a `search` tool")
        .prefixed_name
        .clone();
    assert_eq!(search_tool_name, "mcp__huiban__search");

    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel { response: response.to_string(), search_tool_name: search_tool_name.clone(), calls };
    let mut agent = Agent::new(Box::new(model), "system", 5, dir.path().to_path_buf(), cancel_token(), ApprovalMode::AutoApprove)
        .register_builtins();
    let registry = Arc::new(Mutex::new(registry));
    for tool in tools {
        agent = agent.register(Box::new(zorp_agent::mcp_adapter::McpToolAdapter { tool, registry: registry.clone() }));
    }
    (agent, dir)
}

fn track_with_draft(project: &Project, track_id: &str) {
    project.store.create_track(track_id, "does caching help").unwrap();
    let track_dir = project.track_dir(track_id);
    std::fs::create_dir_all(&track_dir).unwrap();
    std::fs::write(track_dir.join("draft.md"), "# Draft\n\nLatency improved.").unwrap();
}

#[test]
fn full_round_trip_finds_venues_and_approves() {
    let (mut agent, agent_dir) = build_agent_with_huiban_stub("## Candidate Venues\n\n1. Example Systems Conference (deadline 2026-12-01, CORE A)");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_draft(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
    assert!(approved);

    let venues_path = project.track_dir("t1").join("venues.md");
    let content = std::fs::read_to_string(&venues_path).unwrap();
    assert!(content.contains("Example Systems Conference"));
    drop(agent_dir);
}

#[test]
fn rejected_checkpoint_leaves_track_status_unchanged() {
    let (mut agent, agent_dir) = build_agent_with_huiban_stub("## Candidate Venues\n\n1. Example Systems Conference");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_draft(&project, "t1");
    let mode = CheckpointMode::Interactive(Arc::new(RejectAll));

    let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
    assert!(!approved);

    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Active);
    drop(agent_dir);
}

#[test]
fn no_draft_refuses_before_running_the_agent() {
    let (mut agent, agent_dir) = build_agent_with_huiban_stub("## Candidate Venues\n\n1. Example Systems Conference");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project.store.create_track("t1", "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
    assert!(matches!(err, DeliverError::NoDraft));
    drop(agent_dir);
}

#[test]
fn no_huiban_tool_refuses_even_with_a_draft_present() {
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_draft(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    // Build an agent with no MCP tools attached at all, unlike the other
    // tests in this file.
    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel {
        response: "irrelevant".to_string(),
        search_tool_name: "irrelevant".to_string(),
        calls,
    };
    let mut agent = Agent::new(Box::new(model), "system", 5, std::env::temp_dir(), cancel_token(), ApprovalMode::AutoApprove)
        .register_builtins();

    let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
    assert!(matches!(err, DeliverError::NoVenueTool));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p zorp-agent --features research --test deliver_integration -- --nocapture`
Expected: PASS, all four tests.

- [ ] **Step 3: Run the full research-feature suite once more**

Run: `cargo test -p zorp-agent --features research`
Expected: PASS, no regressions in validate's, investigate's, or co_write's existing tests.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/tests/deliver_integration.rs
git commit -m "test(zorp-agent): add deliver end-to-end integration test"
```

---

### Task 6: Documentation updates

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `AGENTS.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/DECISIONS.md`

**Interfaces:** None (documentation only).

- [ ] **Step 1: Update status lines**

In `CLAUDE.md` and `AGENTS.md`, update BOTH the "What this repo is" paragraph (which now correctly says three of four are built, per co-write's Task 7 fix) AND the "Status" section further down, to say all four capabilities, validate, investigate, co-write, and deliver, are built and tested. Read the current text of both sections in both files first; do not assume their exact current wording.

- [ ] **Step 2: Update `docs/ARCHITECTURE.md`**

Add a line for `deliver`'s built status, following the pattern already used for validate/investigate/co-write, pointing at `docs/superpowers/specs/2026-08-09-zorp-deliver-design.md`. Since this is the last of the four, also check whether the file's "What's still open" section (or equivalent) should now say "none — all four capabilities are built" or be removed/renamed if it becomes empty; read the file's current structure before deciding.

- [ ] **Step 3: Update `README.md`**

Read the CURRENT full README.md first (this exact step tripped up the equivalent task twice before, on investigate and once nearly on co-write: there are at least three separate spots, a status banner near the top, the "Why zorp" section, and the roadmap checklist near the bottom, that each independently claim which capabilities are built). Update all of them consistently: check off `deliver` in the roadmap checklist, and update the status banner and "Why zorp" sentence to say all four capabilities are now built and tested. Do a final grep for "still being designed" and "not been built" across README.md, CLAUDE.md, AGENTS.md, and docs/ARCHITECTURE.md after editing to confirm nothing stale survives (the only remaining legitimate open item in the whole repo at this point should be the systems paper, which is explicitly out of scope for all four capabilities and tracked separately).

- [ ] **Step 4: Append a `docs/DECISIONS.md` entry**

Add a new entry at the top of the log (below the `---` separator, above the existing newest entry):

```markdown
## 2026-08-09: deliver's design: huiban-only, academic venues only, checkpoint doesn't kill the track

**Decision:** `deliver` is scoped to academic venue-matching only for v1,
not the broader "right format for any audience" language used elsewhere.
It requires a `draft.md` (from `co-write`) and a huiban-prefixed MCP
tool to be configured, checked the same way `validate` requires a
search-capable tool. The agent uses huiban to find and rank real
conferences and journals fitting the draft's scope, writes the shortlist
to `venues.md`, and checkpoints it. Rejecting the checkpoint does not
kill the track, matching `co-write`'s behavior, not `validate`'s or
`investigate`'s.

**Why:** A non-academic artifact has no equivalent of a "venue" in the
same concrete sense a paper does, and a generic reformatting mechanism
for arbitrary audiences is a different, larger problem than a first
version needs to solve. Requiring huiban specifically, rather than
falling back to generic search, avoids weak or fabricated venue matches
from a tool not built for this. Not killing the track on rejection
matches `co-write`'s reasoning: a shortlist not being good enough isn't
evidence anything upstream failed.

**Ruled out:** A general "format for any audience" mechanism for
non-academic artifacts (would need its own design if it becomes a real
need, not a bolt-on here). A shipped, static venue catalog (already
ruled out earlier in the decision log). Falling back to generic web
search when huiban isn't configured.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-deliver-design.md`
```

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md AGENTS.md docs/ARCHITECTURE.md docs/DECISIONS.md
git commit -m "docs: record deliver capability as built, all four capabilities complete"
```
