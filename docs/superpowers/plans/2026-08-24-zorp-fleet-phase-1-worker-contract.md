# fleet Phase 1: the worker contract, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make one `zorp-web` process answerable as a fleet worker: it says whether it is busy, says what it can actually run, says which version of its API it speaks, takes its secret from the environment, and refuses to drive its siblings.

**Architecture:** No new crate and no new protocol. Phase 1 is five small changes to `zorp-web` and one to `zorp-agent`'s policy. The endpoints a coordinator needs already exist as routes; four of the five tasks extend what they return or where their configuration comes from. The fifth extends the existing own-server denial in `zorp-agent/src/policy.rs` from one port to a list of named peers.

**Tech Stack:** Rust (workspace MSRV 1.95), axum, clap, serde_json. No new dependencies except one additional feature flag on clap, which is already a workspace dependency.

**Spec:** `docs/superpowers/specs/2026-08-24-zorp-fleet-distributed-design.md`

## Scope of this plan, and what it deliberately leaves out

The spec describes four phases. This plan covers **Phase 1 only**. That is a
deliberate split, for three reasons.

- **Phase 0 needs no code.** The spec says so, and it checks out: `Dockerfile`
  and `compose.yml` are both present at the repo root today. Phase 0 is a
  deployment exercise, not an implementation one.
- **Phase 2 cannot be planned yet.** The spec defers three open questions to
  Phases 2 and 3: whether a worker blocks or parks while a checkpoint waits,
  where track remotes live, and whether `zorp-fleet` reuses `zorp-web`'s
  session store shape. Writing task-level steps for the coordinator would mean
  inventing answers to those three questions inside a plan document, which is
  the wrong place to decide them. The spec's own handoff note says the
  checkpoint relay is the piece to brainstorm first. Brainstorm it, extend the
  spec, then plan Phase 2.
- **Phase 1 is independently useful and stops cleanly.** After this plan, a
  human running several worker pods by hand can ask each one whether it is
  busy and what it can run, and no agent on any pod can reach another pod.
  That is worth having whether or not `zorp-fleet` is ever built.

## Two corrections to the spec, applied in this plan

Both were found by reading the code the spec describes. Neither changes the
design; both change what the work is.

1. **`/api/health` and `/api/capabilities` already exist.** The spec calls them
   "two additions on the worker". They are both registered in
   `zorp-web/src/api.rs` today, at lines 106 and 113. `health()` returns
   `{"status":"ok"}` and nothing else; `capabilities()` reports exactly one
   capability, `web_search`. So Tasks 1 and 2 extend existing handlers. An
   implementer who takes the spec literally and adds a second route with the
   same path will make axum panic at startup.
2. **Per-worker tokens need an environment variable, not a new mechanism.**
   The spec asks for "one token per worker, not one shared token for the
   fleet". Each `zorp-web` already takes its own `--token`, so the per-worker
   part is satisfied by construction. What is missing is a way to supply it
   that suits a Kubernetes Secret: `--token` on the command line is visible in
   `ps` output and in a pod spec's `args`. Task 4 adds `ZORP_WEB_TOKEN`.

## Global Constraints

Every task's requirements implicitly include this section.

- MSRV is **1.95**, declared as `rust-version` in the root `Cargo.toml` and pinned by the `msrv` CI job.
- Shared dependency versions live in `[workspace.dependencies]` in the root `Cargo.toml`, never in member manifests.
- `Cargo.lock` is committed and CI builds `--locked`.
- Run `cargo build --workspace` and `cargo test --workspace` before considering any Rust change done.
- The tree is `cargo fmt` clean and CI gates on it. Run `cargo fmt --all` before committing.
- Prose in this repo (comments, docs, commit messages) uses **no em dashes or en dashes as punctuation**. Use a period, comma, colon, or a plain hyphenated compound word. Prefer short, direct sentences.
- `cargo test --workspace` does **not** exercise the `research` feature. This plan does not touch research-feature code, but if a task's change reaches `zorp-web` state or policy, also run `cargo test -p zorp-web --features research`.
- Add a short entry to `docs/DECISIONS.md` only where a task says to. Not every change earns one.
- **The standing rule this phase extends:** a command may not call the server it is running under (`docs/DECISIONS.md`, 2026-08-20). Task 5 widens it; nothing in this plan may narrow it.
- **The standing rule this phase must not erode:** workers never run with checkpoints auto-approved. No task here adds such a flag, and no task may make one easier to add.

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `zorp-web/src/api.rs` | Route table and HTTP handlers. `health` and `capabilities` live here. | 1, 2, 3 |
| `zorp-web/src/state.rs` | `AppState` and `SessionState`. Already carries `running: bool` per session. | 1 |
| `zorp-web/src/main.rs` | CLI parsing and server startup. Where `--token` and the new peer flag are read. | 4, 5 |
| `zorp-web/tests/worker_contract.rs` | **New.** One integration test file for the whole worker contract: health, capabilities, version. | 1, 2, 3 |
| `zorp-agent/src/policy.rs` | `Policy`, `with_own_server`, `calls_own_server`, `deny_reason_with`. | 5 |
| `docs/DECISIONS.md` | Decision log. One entry at the end of Task 5. | 5 |

A single new test file rather than one per task: these five behaviours are one
contract, a reader checking "what does a worker promise" should find it in one
place, and the file stays small.

---

### Task 1: `/api/health` says whether a turn is in flight

A coordinator scheduling work needs to know more than "the process is up". The
spec asks for "liveness plus whether a turn is in flight". `SessionState`
already carries `running: bool`, set by the turn machinery, so this is a read
across sessions and not new bookkeeping.

**Files:**
- Modify: `zorp-web/src/api.rs` (the `health` handler, currently at line 142)
- Modify: `zorp-web/src/state.rs` (add one method to `AppState`)
- Test: `zorp-web/tests/worker_contract.rs` (create)

**Interfaces:**
- Consumes: `AppState::ids() -> Vec<String>`, `AppState::get(&str) -> Option<Arc<Mutex<SessionState>>>`, `SessionState::running: bool`. All exist today in `zorp-web/src/state.rs`.
- Produces: `AppState::busy() -> bool`, used by nothing else in this plan but part of the worker contract. `/api/health` gains fields `busy: bool` and `running_turns: usize`; Task 3 adds `api_version` to the same payload.

- [ ] **Step 1: Write the failing test**

Create `zorp-web/tests/worker_contract.rs`:

```rust
//! What a worker promises a coordinator.
//!
//! These five behaviours (liveness, busy, capability, version, and the
//! secret's source) are one contract, so they are tested in one file. A
//! reader asking "what can a coordinator rely on" should not have to find
//! five test files to answer it.

use zorp_web::state::AppState;

/// A fresh server has no sessions, so it cannot be busy.
#[test]
fn a_server_with_no_sessions_is_not_busy() {
    let state = AppState::new();
    assert!(!state.busy(), "a server with no sessions reported itself busy");
}

/// A session that exists but is not running a turn does not make the worker
/// busy. This is the case that a naive "any session at all" check gets wrong,
/// and it is the common one: the sidebar is full of finished conversations.
#[test]
fn an_idle_session_does_not_make_a_worker_busy() {
    let state = AppState::new();
    state.create("session-one");
    assert!(!state.busy(), "an idle session made the worker report busy");
}

/// One running turn is enough.
#[test]
fn a_running_turn_makes_a_worker_busy() {
    let state = AppState::new();
    let session = state.create("session-one");
    session.lock().expect("session mutex").running = true;
    assert!(state.busy(), "a running turn did not make the worker report busy");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p zorp-web --test worker_contract`

Expected: FAIL to compile, with `no method named 'busy' found for struct 'AppState'`. A compile failure is the correct red here: the method does not exist yet.

- [ ] **Step 3: Add `AppState::busy`**

In `zorp-web/src/state.rs`, add to the `impl AppState` block, next to `ids`:

```rust
    /// Whether any session on this worker is running a turn right now.
    ///
    /// A fleet scheduler asks this before assigning work, so it counts turns
    /// and not sessions. A worker with fifty finished conversations and
    /// nothing running is free, and reporting it busy would strand it.
    ///
    /// A session whose mutex is poisoned counts as busy. The alternative is
    /// to skip it, which would report a worker free on the strength of a
    /// session nobody can read, and handing that worker more work is the
    /// worse of the two mistakes.
    pub fn busy(&self) -> bool {
        self.running_turns() > 0
    }

    /// How many turns are in flight. `busy` is the question a scheduler asks;
    /// this is the number a human watching a fleet wants to see.
    pub fn running_turns(&self) -> usize {
        self.ids()
            .into_iter()
            .filter_map(|id| self.get(&id))
            .filter(|session| match session.lock() {
                Ok(session) => session.running,
                Err(_) => true,
            })
            .count()
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p zorp-web --test worker_contract`

Expected: PASS, 3 tests.

- [ ] **Step 5: Report it from `/api/health`**

In `zorp-web/src/api.rs`, replace the `health` handler:

```rust
/// Liveness, plus whether this worker has room for work.
///
/// The busy fields are here and not on `/api/capabilities` because they
/// change every turn while capabilities change only when the binary or its
/// configuration does. A scheduler polls this one.
async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "busy": state.busy(),
        "running_turns": state.running_turns(),
    }))
}
```

The route at line 106 already exists and does not change. Note that `health`
now takes `State(state): State<AppState>`, which it did not before.

- [ ] **Step 6: Run the whole suite**

Run: `cargo test -p zorp-web`

Expected: PASS. If an existing test asserts the exact body of `/api/health`, update it to assert on the `status` field rather than on the whole object, and say in the test why: the payload grows as the worker contract does.

- [ ] **Step 7: Commit**

```bash
git add zorp-web/src/state.rs zorp-web/src/api.rs zorp-web/tests/worker_contract.rs
git commit -m "feat(web): health says whether a turn is in flight"
```

---

### Task 2: `/api/capabilities` says what can actually run here

The spec's reason for this endpoint is specific and worth keeping in view:
`validate` refuses to run without a search-capable tool and `deliver` refuses
without a huiban-prefixed one, so a scheduler that does not know what a worker
has will discover it from a refusal, after spending the assignment.

The handler exists and reports one capability. It gains two fields: the
features the binary was compiled with, and the tool names actually attached.

**Files:**
- Modify: `zorp-web/src/api.rs` (the `capabilities` handler, currently at line 163)
- Test: `zorp-web/tests/worker_contract.rs` (extend)

**Interfaces:**
- Consumes: `turn::policy(state.own_port) -> zorp_agent::Policy` and `zorp_agent::web_search_availability(&Policy)`, both already used by this handler. Plus `zorp_agent::Agent::tool_names() -> Vec<String>` (`zorp-agent/src/agent.rs:533`).
- Produces: `/api/capabilities` gains `features: Vec<String>` and `tools: Vec<String>`. The existing `web_search` object is unchanged, because `web/src/api.ts` reads it.

- [ ] **Step 1: Write the failing test**

Append to `zorp-web/tests/worker_contract.rs`:

```rust
/// The features a worker was compiled with, observed rather than declared.
///
/// The list must come from the same `cfg` the code is compiled under. A
/// hand-maintained constant drifts the first time someone adds a feature and
/// forgets this list, and a scheduler trusting a stale list assigns work that
/// then refuses.
#[test]
fn compiled_features_are_reported() {
    let features = zorp_web::api::compiled_features();

    // The test binary is built with whatever features the invocation asked
    // for, so assert the mapping rather than a fixed list: a feature is
    // present in the vector exactly when its cfg is on.
    assert_eq!(features.contains(&"search".to_string()), cfg!(feature = "search"));
    assert_eq!(features.contains(&"research".to_string()), cfg!(feature = "research"));
    assert_eq!(features.contains(&"recall".to_string()), cfg!(feature = "recall"));
    assert_eq!(features.contains(&"memory".to_string()), cfg!(feature = "memory"));
}

/// A default build has no optional features on, which is the state a plain
/// `cargo test -p zorp-web` runs in. Stated separately so the empty case is
/// visible rather than implied by the mapping above.
#[test]
#[cfg(not(any(feature = "search", feature = "research", feature = "recall", feature = "memory")))]
fn a_default_build_reports_no_optional_features() {
    assert!(
        zorp_web::api::compiled_features().is_empty(),
        "a default build claimed an optional feature"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p zorp-web --test worker_contract`

Expected: FAIL to compile, with `cannot find function 'compiled_features' in module 'zorp_web::api'`.

- [ ] **Step 3: Add `compiled_features` and extend the handler**

In `zorp-web/src/api.rs`, add above the `capabilities` handler:

```rust
/// The optional features this binary was compiled with.
///
/// Derived from `cfg!` at the point of compilation, so it cannot disagree
/// with the build. A scheduler reads this to know which capabilities are
/// even possible on this worker before it looks at which tools are attached.
pub fn compiled_features() -> Vec<String> {
    let mut features = Vec::new();
    if cfg!(feature = "search") {
        features.push("search".to_string());
    }
    if cfg!(feature = "research") {
        features.push("research".to_string());
    }
    if cfg!(feature = "recall") {
        features.push("recall".to_string());
    }
    if cfg!(feature = "memory") {
        features.push("memory".to_string());
    }
    features
}
```

Then extend the handler, keeping the existing `web_search` object exactly as
it is:

```rust
async fn capabilities(State(state): State<AppState>) -> Json<serde_json::Value> {
    let policy = turn::policy(state.own_port);
    let web_search = zorp_agent::web_search_availability(&policy);
    Json(json!({
        "web_search": {
            "available": web_search.available,
            "detail": web_search.detail,
        },
        "features": compiled_features(),
        "tools": attached_tool_names(&state),
    }))
}
```

- [ ] **Step 4: Add `attached_tool_names`**

The tool names must come from the same construction a real turn uses, for the
same reason `web_search` already does: a second list is a list that can be
wrong. Read how `turn.rs` builds its `Agent` and reuse that path. If building
an agent purely to list its tools is too costly to do per request, build it
once at startup and store the names on `AppState`; either is acceptable, and
the test below does not care which you choose.

```rust
/// The tools a turn on this worker would actually have.
///
/// Named from the same construction a turn uses. `deliver` refuses without a
/// huiban-prefixed tool and `validate` refuses without a search-capable one,
/// so a scheduler that cannot see this list finds out by wasting an
/// assignment on a refusal.
fn attached_tool_names(state: &AppState) -> Vec<String> {
    // Build the agent the way `turn::spawn_turn` does, then ask it.
    // See `zorp_agent::Agent::tool_names` at zorp-agent/src/agent.rs:533.
    todo_replace_with_the_turn_path(state)
}
```

**This is the one step in this plan that names no exact call.** Read
`zorp-web/src/turn.rs` and mirror how it constructs the agent, because that
construction is the thing being reported on and it may have changed since this
plan was written. Do not invent a second way to assemble a tool registry.

- [ ] **Step 5: Write the tool-list test**

Append to `zorp-web/tests/worker_contract.rs`:

```rust
/// The built-in tools are always attached, so the list is never empty.
/// An empty list would read as "this worker can do nothing", which would
/// take it out of every scheduling decision.
#[test]
fn attached_tools_are_reported_and_never_empty() {
    let state = AppState::new();
    let tools = zorp_web::api::tool_names_for_test(&state);
    assert!(!tools.is_empty(), "a worker reported no tools at all");
    assert!(
        tools.iter().any(|name| name == "run_command"),
        "the built-in tools were missing from the reported list: {tools:?}"
    );
}
```

Expose whatever thin wrapper this needs (`tool_names_for_test`) as
`#[doc(hidden)] pub` rather than making the internal helper public.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p zorp-web --test worker_contract`
Then: `cargo test -p zorp-web --features search --test worker_contract`

Expected: PASS in both. The second run is the point of the feature test: it is
the only way to see the mapping produce a non-empty list.

- [ ] **Step 7: Commit**

```bash
git add zorp-web/src/api.rs zorp-web/tests/worker_contract.rs
git commit -m "feat(web): capabilities says which features and tools are really here"
```

---

### Task 3: version the worker API

The spec says that once `zorp-fleet` depends on this API, breaking it is a
decision and not a side effect of UI work. That needs a number a coordinator
can check and a test that makes changing the number deliberate.

**Do not version by path prefix.** Moving the routes under `/api/v1/` would
break every call in `web/src/api.ts` and buys nothing Phase 1 needs. A single
integer in the capabilities payload, plus a test pinning it, gives the
coordinator what it needs and leaves the browser alone.

**Files:**
- Modify: `zorp-web/src/api.rs`
- Test: `zorp-web/tests/worker_contract.rs` (extend)

**Interfaces:**
- Produces: `zorp_web::api::WORKER_API_VERSION: u32`, and an `api_version` field on both `/api/health` and `/api/capabilities`. On health as well as capabilities so that a coordinator can learn the version from the endpoint it already polls, without a second request.

- [ ] **Step 1: Write the failing test**

Append to `zorp-web/tests/worker_contract.rs`:

```rust
/// The worker API version is pinned so that changing it is a decision.
///
/// This test exists to fail. If you are here because it failed, you changed
/// the shape of what a worker promises. That is allowed, and the steps are:
/// bump WORKER_API_VERSION, update this constant, and add an entry to
/// docs/DECISIONS.md saying what changed and what a coordinator must do
/// about it. What is not allowed is changing the payload and leaving the
/// version alone, because a coordinator has no other way to notice.
#[test]
fn the_worker_api_version_is_pinned() {
    assert_eq!(
        zorp_web::api::WORKER_API_VERSION,
        1,
        "the worker API version changed; see this test's comment"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p zorp-web --test worker_contract`

Expected: FAIL to compile, with `cannot find value 'WORKER_API_VERSION' in module 'zorp_web::api'`.

- [ ] **Step 3: Add the constant and report it**

In `zorp-web/src/api.rs`, near the top:

```rust
/// The version of the contract a worker offers a fleet coordinator.
///
/// Bumped when the shape of `/api/health` or `/api/capabilities` changes in a
/// way a coordinator could notice: a field removed, a field's meaning
/// changed, a type changed. Adding a field is not a break, because a
/// coordinator reading by name does not see it.
///
/// `worker_contract.rs` pins this number so a change has to be typed twice.
pub const WORKER_API_VERSION: u32 = 1;
```

Add `"api_version": WORKER_API_VERSION,` to the objects returned by both
`health` and `capabilities`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p zorp-web --test worker_contract`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add zorp-web/src/api.rs zorp-web/tests/worker_contract.rs
git commit -m "feat(web): the worker API says which version it speaks"
```

---

### Task 4: a worker takes its token from the environment

The spec wants one token per worker rather than one shared across the fleet.
Each `zorp-web` already takes its own `--token`, so the per-worker part is
already true. What is missing is a way to pass it that suits a Kubernetes
Secret. A token in `args` is visible in `ps` on the node and in the pod spec
to anyone who can read it.

**Files:**
- Modify: `Cargo.toml` (add the `env` feature to the workspace clap dependency)
- Modify: `zorp-web/src/main.rs` (the `token` field, line 17)
- Test: `zorp-web/tests/worker_contract.rs` (extend)

**Interfaces:**
- Produces: `ZORP_WEB_TOKEN` as a fallback source for `--token`. The explicit flag continues to win, so nothing that works today changes.

- [ ] **Step 1: Add the clap feature**

In the root `Cargo.toml`, line 58, change:

```toml
clap = { version = "4", features = ["derive"] }
```

to:

```toml
clap = { version = "4", features = ["derive", "env"] }
```

Member manifests keep `clap = { workspace = true }` and are not touched, per
the workspace dependency rule.

- [ ] **Step 2: Write the failing test**

Append to `zorp-web/tests/worker_contract.rs`:

```rust
/// The token may come from the environment, so a Kubernetes Secret can carry
/// it without putting it in the pod spec's args, where it shows up in `ps`
/// on the node.
///
/// Parsed through clap rather than by calling std::env::var in a handler,
/// so there is exactly one place a token is read and `--help` documents it.
#[test]
fn the_token_can_come_from_the_environment() {
    // Uses clap's parser directly rather than spawning a server: the
    // question is where the value comes from, not what the server does
    // with it.
    let parsed = zorp_web::cli_for_test(&["zorp-web"], &[("ZORP_WEB_TOKEN", "from-the-env")]);
    assert_eq!(parsed.token.as_deref(), Some("from-the-env"));
}

/// An explicit flag beats the environment, so an operator debugging one pod
/// can override the Secret without editing it.
#[test]
fn an_explicit_token_flag_beats_the_environment() {
    let parsed = zorp_web::cli_for_test(
        &["zorp-web", "--token", "from-the-flag"],
        &[("ZORP_WEB_TOKEN", "from-the-env")],
    );
    assert_eq!(parsed.token.as_deref(), Some("from-the-flag"));
}
```

Note: `Cli` currently lives in `zorp-web/src/main.rs` and is not reachable from
an integration test. Move the struct into the library (`zorp-web/src/cli.rs`,
re-exported from `lib.rs`) and have `main.rs` use it. That move is part of this
task, not a separate one, because the test cannot exist without it.

`cli_for_test` sets the named variables, parses the argv, and restores the
environment. Environment variables are process-global, so mark these two tests
`#[serial]` if the crate already uses `serial_test`, or put both assertions in
one test function if it does not. Do not add a new dependency for this.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p zorp-web --test worker_contract`

Expected: FAIL to compile, `cannot find function 'cli_for_test'`.

- [ ] **Step 4: Implement**

Move `Cli` to `zorp-web/src/cli.rs`, and on the `token` field:

```rust
    /// Shared secret, required when binding to a non-loopback interface.
    ///
    /// Also readable from `ZORP_WEB_TOKEN`, which is how a worker in a
    /// cluster should get it: a Secret mounted as env, one per worker, not
    /// one shared across the fleet. The flag wins when both are set.
    #[arg(long, env = "ZORP_WEB_TOKEN")]
    token: Option<String>,
```

Make the field `pub`, add `cli_for_test`, and leave the non-loopback check in
`main.rs` at line 80 exactly as it is. It reads `cli.token`, which is now
populated from either source, so the guard keeps working unchanged and a
worker bound to `0.0.0.0` with only the environment variable set still starts.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p zorp-web`
Then: `cargo build --workspace` (the clap feature change touches every binary)

Expected: PASS and a clean build.

- [ ] **Step 6: Document it**

Add `ZORP_WEB_TOKEN` to the README's environment variable list, beside the
other `ZORP_` variables.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock zorp-web/src/cli.rs zorp-web/src/main.rs zorp-web/src/lib.rs zorp-web/tests/worker_contract.rs README.md
git commit -m "feat(web): a worker can take its token from the environment"
```

---

### Task 5: a worker cannot drive its siblings

This is the security task, and the one with a standing rule behind it. The
2026-08-20 decision says a command may not call the server it is running
under. In a cluster that is not enough: an agent on pod A that can `curl` pod
B's API can start turns on pod B, which is the fleet's own control plane used
as a lateral movement path.

The existing mechanism is exactly the right shape to extend. `Policy` holds
`own_server_port: Option<u16>`, `calls_own_server` checks the command text for
`host:port` against a fixed host list, and `deny_reason_with` returns the
denial. Task 5 adds a list of peer authorities beside the port.

**Files:**
- Modify: `zorp-agent/src/policy.rs` (`Policy`, `deny_reason_with`, and a new `with_fleet_peers`)
- Modify: `zorp-web/src/cli.rs` and `zorp-web/src/main.rs` (a repeatable flag)
- Modify: `zorp-web/src/turn.rs` (`policy`, so a turn gets the peers)
- Modify: `zorp-web/src/state.rs` (carry the peer list on `AppState`)
- Test: `zorp-agent/src/policy.rs` unit tests, and `zorp-web/tests/worker_contract.rs`
- Modify: `docs/DECISIONS.md`

**Interfaces:**
- Consumes: `Policy::with_own_server(u16)`, `deny_reason_with(command, repo_root, own_server_port)`, both in `zorp-agent/src/policy.rs`.
- Produces: `Policy::with_fleet_peers(peers: Vec<String>) -> Policy`. `deny_reason_with` gains a `fleet_peers: &[String]` parameter. `AppState::with_fleet_peers(Vec<String>)`. `turn::policy` gains the peer list.

- [ ] **Step 1: Write the failing test**

In `zorp-agent/src/policy.rs`, in the existing test module:

```rust
    /// A worker must not be able to drive a sibling worker.
    ///
    /// The 2026-08-20 rule stopped an agent calling the server it runs
    /// under. In a fleet that leaves the obvious gap open: pod A calling pod
    /// B's API is the same capability, aimed sideways, and the fleet's own
    /// control plane is the path.
    #[test]
    fn a_command_naming_a_sibling_worker_is_denied() {
        let policy = Policy::default().with_fleet_peers(vec!["worker-2.zorp.svc:7777".to_string()]);
        let call = run_command_call("curl -s http://worker-2.zorp.svc:7777/api/sessions");
        assert!(
            matches!(policy.decide(&call), Decision::Deny(_)),
            "a command naming a sibling worker was not denied"
        );
    }

    /// Case does not get you past it, for the same reason the own-server
    /// check lowercases first.
    #[test]
    fn a_sibling_named_in_a_different_case_is_still_denied() {
        let policy = Policy::default().with_fleet_peers(vec!["worker-2.zorp.svc:7777".to_string()]);
        let call = run_command_call("curl -s http://WORKER-2.ZORP.SVC:7777/api/sessions");
        assert!(matches!(policy.decide(&call), Decision::Deny(_)));
    }

    /// The coordinator counts as a peer. It is the most valuable target of
    /// the three, because it holds every worker's token.
    #[test]
    fn a_command_naming_the_coordinator_is_denied() {
        let policy = Policy::default().with_fleet_peers(vec!["fleet-coordinator:8080".to_string()]);
        let call = run_command_call("curl http://fleet-coordinator:8080/api/jobs");
        assert!(matches!(policy.decide(&call), Decision::Deny(_)));
    }

    /// With no peers configured, nothing new is denied. A single-machine
    /// zorp behaves exactly as it does today.
    #[test]
    fn without_peers_an_ordinary_command_is_untouched() {
        let policy = Policy::default();
        let call = run_command_call("curl https://example.com/data.json");
        assert!(!matches!(policy.decide(&call), Decision::Deny(_)));
    }
```

Use whatever helper the existing tests in this file use to build a
`run_command` `ToolCall`; `run_command_call` above stands in for it. Match the
file's existing style rather than introducing a new one.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p zorp-agent policy`

Expected: FAIL to compile, `no method named 'with_fleet_peers'`.

- [ ] **Step 3: Implement in `policy.rs`**

Add the field to `Policy` (default empty), then:

```rust
    /// Machines this worker shares a fleet with: sibling workers and the
    /// coordinator, as `host` or `host:port`.
    ///
    /// The 2026-08-20 rule denies calls to the server this agent runs under.
    /// This is the same rule aimed sideways. An agent that can reach a
    /// sibling's API can start turns on it, so the fleet's control plane
    /// would be a lateral movement path between pods.
    ///
    /// Matching is a lowercased substring of the command text, the same
    /// blunt instrument `calls_own_server` uses. It is deliberately blunt:
    /// a false positive costs one denied command with a legible reason, and
    /// a false negative costs the boundary.
    pub fn with_fleet_peers(mut self, peers: Vec<String>) -> Policy {
        self.fleet_peers = peers
            .into_iter()
            .map(|peer| peer.trim().to_ascii_lowercase())
            .filter(|peer| !peer.is_empty())
            .collect();
        self
    }
```

In `deny_reason_with`, after the existing `own_server_port` block and before
the denylist check:

```rust
    for peer in fleet_peers {
        if normalized.contains(peer.as_str()) {
            return Some(format!(
                "the command names {peer}, another machine in this fleet. \
                 An agent on one worker may not drive another worker or the \
                 coordinator"
            ));
        }
    }
```

Add the `fleet_peers: &[String]` parameter and update every call site.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p zorp-agent policy`

Expected: PASS, including the pre-existing own-server tests, which must not
have changed behaviour.

- [ ] **Step 5: Wire it through `zorp-web`**

Add to the CLI:

```rust
    /// Another machine in this fleet, as `host` or `host:port`, repeatable.
    ///
    /// Names a sibling worker or the coordinator. An agent on this worker is
    /// denied any command naming one of them. Nothing is denied by default,
    /// so a single-machine zorp is unaffected.
    #[arg(long = "fleet-peer", env = "ZORP_FLEET_PEERS", value_delimiter = ',')]
    fleet_peers: Vec<String>,
```

Carry it on `AppState` beside `own_port`, and pass it through `turn::policy`
so a real turn runs under it. `turn::policy` is also what `/api/capabilities`
calls, so the reported policy stays the policy the agent actually gets, which
is the property that call site was built to have.

- [ ] **Step 6: Test the wiring end to end**

Append to `zorp-web/tests/worker_contract.rs`:

```rust
/// The peers reach the policy a turn actually runs under, not just the CLI.
/// A flag that parses but never reaches the agent is the failure this test
/// exists to catch.
#[test]
fn configured_peers_reach_the_turn_policy() {
    let state = AppState::new().with_fleet_peers(vec!["worker-2:7777".to_string()]);
    let policy = zorp_web::turn::policy_for_test(&state);
    let call = /* build a run_command call for "curl http://worker-2:7777/" */;
    assert!(
        matches!(policy.decide(&call), zorp_agent::Decision::Deny(_)),
        "a configured fleet peer did not reach the policy a turn runs under"
    );
}
```

- [ ] **Step 7: Run everything**

```bash
cargo fmt --all
cargo build --workspace
cargo test --workspace
cargo test -p zorp-web --features research
cargo clippy --workspace --all-targets
```

Expected: all pass.

- [ ] **Step 8: Record the decision**

Add an entry at the top of `docs/DECISIONS.md`, dated 2026-08-24, saying: the
2026-08-20 own-server denial is extended to named fleet peers; matching is a
lowercased substring and deliberately blunt; nothing is denied by default so a
single-machine zorp is unchanged; and the reason, which is that an agent able
to reach a sibling's API can start turns on it. Link the fleet spec.

- [ ] **Step 9: Commit**

```bash
git add zorp-agent/src/policy.rs zorp-web/src/cli.rs zorp-web/src/main.rs zorp-web/src/state.rs zorp-web/src/turn.rs zorp-web/tests/worker_contract.rs docs/DECISIONS.md
git commit -m "feat(agent): an agent may not drive a sibling worker or the coordinator"
```

---

## Self-review against the spec

**Spec coverage for Phase 1.** The spec lists Phase 1 as: add `/api/health`
and `/api/capabilities`, version the API, add per-worker tokens and the
sibling-call denial.

| Phase 1 requirement | Task | Note |
|---|---|---|
| `/api/health`, liveness plus in-flight turn | 1 | Route exists; handler extended |
| `/api/capabilities`, features and attached tools | 2 | Route exists; handler extended |
| Version the API | 3 | Constant plus pinning test, not a path prefix |
| Per-worker tokens | 4 | Already per worker; adds an env source fit for a Secret |
| Sibling-call denial | 5 | Extends the 2026-08-20 own-server rule |

**Not covered here, and deliberately.** TLS between coordinator and workers
(the spec says "inside a cluster the links get TLS or a service mesh that
provides it") is a deployment concern with no code in `zorp-web`, and belongs
with Phase 0's compose and Kubernetes material rather than in a Rust task. The
fleet-level rate-limit cap lives in the scheduler, which is Phase 2. Both are
noted so a reader does not think they were missed.

**Placeholder scan.** One deliberate gap, flagged in place: Task 2 Step 4 does
not name the exact call that builds the agent, because that construction lives
in `turn.rs` and may have moved. The step says to read it and mirror it, and
says explicitly not to invent a second registry. Every other step carries the
code it needs.

**Type consistency.** `AppState::busy()` and `AppState::running_turns()`
defined in Task 1 and used in Task 1 only. `compiled_features()` defined and
used in Task 2. `WORKER_API_VERSION` defined in Task 3 and read by both
handlers. `Policy::with_fleet_peers(Vec<String>)` defined in Task 5 Step 3 and
used in Steps 1, 5 and 6, with the same signature throughout.
`deny_reason_with` gains one parameter in Task 5 and every call site is updated
in the same step.

## Before starting

The spec this plan implements is not committed on `main` yet. It exists only
in a working tree. Land the spec first, so that the plan's `Spec:` link
resolves for anyone who picks this up later.
