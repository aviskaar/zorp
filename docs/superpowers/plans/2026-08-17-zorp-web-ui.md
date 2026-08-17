# zorp Web UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A local chat interface for `zorp-agent`, served by a new `zorp-web` binary, with streaming tool activity and browser-side approvals.

**Architecture:** `zorp-web` constructs a real `Agent` (it does not shell out to the CLI) and substitutes a `WebRenderer` for the terminal renderer. Renderer callbacks become typed JSON events on a channel, which an SSE endpoint streams to the browser. Approval-gated tools park the agent thread on a oneshot channel until a POST resolves them. The UI is static files talking to a documented HTTP API, so server and UI containerize separately.

**Tech Stack:** Rust, axum, tokio, serde_json, rusqlite (existing), TypeScript + esbuild for the UI, Docker.

**Spec:** `docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md`

## Global Constraints

- MSRV 1.82. Edition 2021.
- Shared dependency versions live in `[workspace.dependencies]` in the root `Cargo.toml`, not in member manifests.
- `Cargo.lock` is committed and CI builds `--locked`. Run `cargo build` after adding dependencies and commit the lockfile change.
- The tree must be `cargo fmt --all` clean; CI gates on it.
- Prose in code comments, docs, and commit messages uses no em dashes or en dashes as punctuation. Short direct sentences.
- `zorp-web` is a new workspace member. Do not modify `zorp-agent/src/` except where a task explicitly says to.
- No test may require a network connection or an API key.
- The server binds `127.0.0.1` unless both `--bind` and `--token` are given.

---

### Task 1: Scaffold the crate and a health endpoint

**Files:**
- Create: `zorp-web/Cargo.toml`
- Create: `zorp-web/src/main.rs`
- Create: `zorp-web/src/api.rs`
- Create: `zorp-web/tests/health.rs`
- Modify: `Cargo.toml` (workspace `members`, `[workspace.dependencies]`)

**Interfaces:**
- Consumes: nothing.
- Produces: `zorp_web::api::router() -> axum::Router`, and a binary `zorp-web` accepting `--bind`, `--port`, `--token`.

- [ ] **Step 1: Add the workspace member and shared deps**

In the root `Cargo.toml`, add `"zorp-web"` to `members` (before `"erbga"`), and add to `[workspace.dependencies]`:

```toml
axum = "0.7"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }
```

- [ ] **Step 2: Write `zorp-web/Cargo.toml`**

```toml
[package]
name = "zorp-web"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
description = "Local web UI server for the zorp agent."
license.workspace = true

[lints]
workspace = true

[dependencies]
zorp-agent = { path = "../zorp-agent" }
axum = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
clap = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Write the failing test**

Create `zorp-web/tests/health.rs`:

```rust
use std::net::SocketAddr;

/// Bind to port 0 so tests never collide on a fixed port.
async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router()).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn health_reports_ok() {
    let addr = spawn().await;
    let body = reqwest_get(&format!("http://{addr}/api/health")).await;
    assert!(body.contains("\"status\":\"ok\""), "got {body}");
}

/// Minimal GET so the crate does not take an HTTP client dependency for one test.
async fn reqwest_get(url: &str) -> String {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        ureq::get(&url).call().unwrap().into_string().unwrap()
    })
    .await
    .unwrap()
}
```

Add `ureq = { workspace = true }` to `zorp-web`'s `[dev-dependencies]`.

- [ ] **Step 4: Run it and watch it fail**

Run: `cargo test -p zorp-web --test health`
Expected: FAIL to compile, `zorp_web::api` does not exist.

- [ ] **Step 5: Write the minimal implementation**

Create `zorp-web/src/api.rs`:

```rust
use axum::{routing::get, Json, Router};
use serde_json::json;

pub fn router() -> Router {
    Router::new().route("/api/health", get(health))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}
```

Create `zorp-web/src/main.rs`:

```rust
use clap::Parser;

pub mod api;

#[derive(Parser)]
#[command(version, about = "Local web UI for the zorp agent")]
struct Cli {
    /// Interface to listen on. Anything other than 127.0.0.1 requires --token,
    /// because a reachable server is agent-driven shell access to this machine.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = 7777)]
    port: u16,
    /// Shared secret required when binding to a non-loopback interface.
    #[arg(long)]
    token: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if cli.bind != "127.0.0.1" && cli.bind != "localhost" && cli.token.is_none() {
        eprintln!(
            "zorp-web: --bind {} exposes agent-driven shell access; --token is required",
            cli.bind
        );
        std::process::exit(2);
    }
    let addr = format!("{}:{}", cli.bind, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| {
        eprintln!("zorp-web: cannot bind {addr}: {e}");
        std::process::exit(1);
    });
    eprintln!("zorp-web: listening on http://{addr}");
    axum::serve(listener, api::router()).await.unwrap();
}
```

Add `zorp-web/src/lib.rs` exposing the module so the integration test can use it:

```rust
pub mod api;
```

and change `main.rs` to `use zorp_web::api;` instead of declaring the module.

- [ ] **Step 6: Run the test and the guard**

Run: `cargo test -p zorp-web --test health`
Expected: PASS.

Run: `cargo run -p zorp-web -- --bind 0.0.0.0`
Expected: exits 2 with the token message.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add Cargo.toml Cargo.lock zorp-web
git commit -m "feat(web): scaffold the zorp-web server with a health endpoint"
```

---

### Task 2: WebRenderer turns agent activity into typed events

**Files:**
- Create: `zorp-web/src/event.rs`
- Create: `zorp-web/src/renderer.rs`
- Test: `zorp-web/src/renderer.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `zorp_agent::Renderer` (methods `working`, `working_done`, `tool(&str, &str)`, `verify(&str, bool)`, `notice(&str)`, `assistant(&str)`).
- Produces: `Event` (serde `Serialize`, tagged by `type`, carrying `seq: u64`), and `WebRenderer::new(tx: std::sync::mpsc::Sender<Event>) -> WebRenderer`.

- [ ] **Step 1: Write the failing test**

Add to `zorp-web/src/renderer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use zorp_agent::Renderer;

    #[test]
    fn renderer_callbacks_become_events_in_order() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut r = WebRenderer::new(tx);
        r.tool("read_file", "a.txt (1 lines)");
        r.assistant("hello");
        drop(r);

        let events: Vec<Event> = rx.iter().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 1);
        assert!(matches!(&events[0].kind, EventKind::Tool { name, .. } if name == "read_file"));
        assert!(matches!(&events[1].kind, EventKind::Assistant { text } if text == "hello"));
    }

    /// The browser reconnects with Last-Event-ID, so seq must never repeat.
    #[test]
    fn seq_is_monotonic_across_kinds() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut r = WebRenderer::new(tx);
        r.working();
        r.notice("n");
        r.verify("cargo test", true);
        drop(r);
        let seqs: Vec<u64> = rx.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p zorp-web --lib renderer`
Expected: FAIL to compile, `WebRenderer` does not exist.

- [ ] **Step 3: Write `event.rs`**

```rust
use serde::Serialize;

/// One frame on the SSE stream. `seq` lets a reconnecting client resume with
/// Last-Event-ID and receive only what it missed.
#[derive(Debug, Serialize)]
pub struct Event {
    pub seq: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    Working,
    WorkingDone,
    Tool { name: String, summary: String },
    Verify { command: String, passed: bool },
    Notice { text: String },
    Assistant { text: String },
    ApprovalRequest { id: String, tool: String, arguments: String },
    Error { message: String },
    Done,
}
```

- [ ] **Step 4: Write `renderer.rs`**

```rust
use crate::event::{Event, EventKind};
use std::sync::mpsc::Sender;
use zorp_agent::Renderer;

/// Bridges the agent's terminal-shaped activity callbacks onto a channel the
/// SSE endpoint drains. Sends are best-effort: a browser that has gone away
/// must not stall the agent.
pub struct WebRenderer {
    tx: Sender<Event>,
    seq: u64,
}

impl WebRenderer {
    pub fn new(tx: Sender<Event>) -> Self {
        WebRenderer { tx, seq: 0 }
    }

    fn emit(&mut self, kind: EventKind) {
        let event = Event { seq: self.seq, kind };
        self.seq += 1;
        let _ = self.tx.send(event);
    }
}

impl Renderer for WebRenderer {
    fn working(&mut self) {
        self.emit(EventKind::Working);
    }
    fn working_done(&mut self) {
        self.emit(EventKind::WorkingDone);
    }
    fn tool(&mut self, name: &str, summary: &str) {
        self.emit(EventKind::Tool {
            name: name.to_string(),
            summary: summary.to_string(),
        });
    }
    fn verify(&mut self, command: &str, passed: bool) {
        self.emit(EventKind::Verify {
            command: command.to_string(),
            passed,
        });
    }
    fn notice(&mut self, text: &str) {
        self.emit(EventKind::Notice {
            text: text.to_string(),
        });
    }
    fn assistant(&mut self, text: &str) {
        self.emit(EventKind::Assistant {
            text: text.to_string(),
        });
    }
}
```

Declare both modules in `zorp-web/src/lib.rs`:

```rust
pub mod api;
pub mod event;
pub mod renderer;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p zorp-web --lib`
Expected: PASS, 2 tests.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add zorp-web
git commit -m "feat(web): map agent renderer callbacks onto typed events"
```

---

### Task 3: Run a turn and stream it over SSE

**Files:**
- Create: `zorp-web/src/turn.rs`
- Modify: `zorp-web/src/api.rs`
- Create: `zorp-web/tests/turn.rs`

**Interfaces:**
- Consumes: `WebRenderer`, `Event`.
- Produces: `POST /api/sessions/:id/turn` and `GET /api/sessions/:id/events`; `turn::spawn_turn(state, session_id, message)`.

- [ ] **Step 1: Write the failing test**

Create `zorp-web/tests/turn.rs`. It scripts a model over a local socket, the same way `zorp-agent/tests/common/mod.rs` does. Copy that helper into `zorp-web/tests/common/mod.rs` verbatim first, then:

```rust
mod common;
use common::mock_script;

#[tokio::test]
async fn a_turn_streams_assistant_text_then_done() {
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"hello from the model"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("s.db"));

    let addr = spawn_server().await;
    let session = post_json(&format!("http://{addr}/api/sessions"), "{}").await;
    let id = session["id"].as_str().unwrap().to_string();

    post_json(
        &format!("http://{addr}/api/sessions/{id}/turn"),
        r#"{"message":"hi"}"#,
    )
    .await;

    let stream = get_text(&format!("http://{addr}/api/sessions/{id}/events")).await;
    assert!(stream.contains("hello from the model"), "got {stream}");
    assert!(stream.contains("\"type\":\"done\""), "got {stream}");
}
```

Write `spawn_server`, `post_json`, and `get_text` as blocking `ureq` calls inside `tokio::task::spawn_blocking`, mirroring Task 1's helper.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p zorp-web --test turn`
Expected: FAIL, the routes do not exist.

- [ ] **Step 3: Implement the turn runner**

`zorp-web/src/turn.rs` builds the agent the way `zorp-agent/src/main.rs` does, using `zorp_agent::HttpModel::try_from_env()`, `Agent::new(...)`, `.register_builtins_filtered(None)`, and `.with_renderer(Box::new(WebRenderer::new(tx)))`. Run it inside `tokio::task::spawn_blocking`, because the agent loop is synchronous and would otherwise stall the runtime. On completion emit `EventKind::Done`; on `Outcome::Error` emit `EventKind::Error` carrying `outcome.describe()`.

Hold per-session state in an `Arc<Mutex<HashMap<String, SessionState>>>` where `SessionState` owns the receiver end and a `Vec<Event>` backlog for reconnects.

- [ ] **Step 4: Implement the SSE route**

`GET /api/sessions/:id/events` returns `axum::response::sse::Sse` over a stream that first replays the backlog after `Last-Event-ID`, then yields live events. Set each frame's `id` to the event `seq`.

- [ ] **Step 5: Run the test**

Run: `cargo test -p zorp-web --test turn`
Expected: PASS.

- [ ] **Step 6: Reject a second concurrent turn**

Add a test asserting that POSTing a second turn while one is running returns 409, then implement it. Interleaved turns on one agent would corrupt the transcript.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add zorp-web
git commit -m "feat(web): run a turn on a blocking thread and stream it over SSE"
```

---

### Task 4: Browser-side approvals

**Files:**
- Create: `zorp-web/src/approval.rs`
- Modify: `zorp-web/src/api.rs`, `zorp-web/src/turn.rs`
- Create: `zorp-web/tests/approval.rs`

**Interfaces:**
- Consumes: `EventKind::ApprovalRequest`.
- Produces: `POST /api/sessions/:id/approve` taking `{"id": "...", "allow": true}`.

- [ ] **Step 1: Write the failing test**

Script a model that requests `write_file`, then assert the stream contains an `approval_request` naming `write_file`, that no file exists yet, that POSTing `{"allow": false}` produces a `tool` event whose summary is `denied`, and that the file still does not exist.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p zorp-web --test approval`
Expected: FAIL, no approval route.

- [ ] **Step 3: Implement the gate**

Register a pending approval keyed by a generated id, emit `ApprovalRequest`, and block the agent thread on `std::sync::mpsc::Receiver::recv_timeout`. On timeout return deny, matching the CLI's non-interactive behavior. The POST handler looks the id up and sends the decision.

- [ ] **Step 4: Run the test**

Run: `cargo test -p zorp-web --test approval`
Expected: PASS.

- [ ] **Step 5: Add the timeout test**

Assert that an approval nobody answers denies rather than hanging, using a short timeout injected for the test.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add zorp-web
git commit -m "feat(web): gate tool approvals on a browser decision"
```

---

### Task 5: Session list and replay

**Files:**
- Create: `zorp-web/src/session.rs`
- Modify: `zorp-web/src/api.rs`
- Create: `zorp-web/tests/sessions.rs`

**Interfaces:**
- Consumes: the existing `sessions.db` schema, table `messages(session_id, seq, role, content)`.
- Produces: `GET /api/sessions`, `GET /api/sessions/:id`.

- [ ] **Step 1: Write the failing test**

Run one turn, then assert `GET /api/sessions` returns an array containing that session id, and `GET /api/sessions/:id` returns its messages in `seq` order with `role` and `content` present.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p zorp-web --test sessions`
Expected: FAIL.

- [ ] **Step 3: Implement the reads**

Open the store at `ZORP_STATE_DB` and select from `messages` ordered by `seq`. Read-only: the UI never writes history directly, the agent does.

- [ ] **Step 4: Run the test, then commit**

```bash
cargo fmt --all
git add zorp-web
git commit -m "feat(web): list and replay sessions from the existing store"
```

---

### Task 6: The static UI

**Files:**
- Create: `web/index.html`, `web/src/main.ts`, `web/src/api.ts`, `web/package.json`
- Create: `web/README.md`

**Interfaces:**
- Consumes: the HTTP API from Tasks 3 to 5.
- Produces: a bundle at `web/dist/`.

- [ ] **Step 1: Write `web/src/api.ts`**

A typed client: `newSession()`, `listSessions()`, `getSession(id)`, `sendTurn(id, message)`, `approve(id, approvalId, allow)`, and `streamEvents(id, onEvent)` wrapping `EventSource`. Read the API base from `window.ZORP_API_BASE ?? ""` so the UI can be served from a different origin than the server.

- [ ] **Step 2: Write `web/src/main.ts`**

A message list, an input box, a sidebar listing sessions, and an approval card rendered when an `approval_request` arrives, with allow and deny buttons wired to `approve()`. Render `tool` events as a compact activity line, matching the CLI's `● name  summary` shape.

- [ ] **Step 3: Build it**

```bash
cd web && npm install && npx esbuild src/main.ts --bundle --outfile=dist/main.js
```

- [ ] **Step 4: Verify by hand**

Start the server, open `web/index.html`, send "read README.md and reply with its first line", and confirm the activity line appears before the answer and an approval card appears for a write.

- [ ] **Step 5: Commit**

```bash
git add web
git commit -m "feat(web): static chat UI with streaming activity and approvals"
```

---

### Task 7: Containers

**Files:**
- Create: `zorp-web/Dockerfile`, `web/Dockerfile`, `compose.yml`
- Modify: `README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: two images and a compose file that runs both.

- [ ] **Step 1: Write `zorp-web/Dockerfile`**

Two stages. Build with `rust:1.82-slim`, `cargo build --release -p zorp-web`. Runtime on `debian:12-slim` with `ca-certificates`, a non-root uid 1000, `WORKDIR /work`, and `CMD ["zorp-web", "--bind", "0.0.0.0", "--port", "7777"]`. The image requires `ZORP_WEB_TOKEN` to be passed through as `--token`, so an exposed container is never tokenless.

- [ ] **Step 2: Write `web/Dockerfile`**

`node:22-slim` build stage running esbuild, runtime `nginx:alpine` serving `dist/`.

- [ ] **Step 3: Write `compose.yml`**

Two services, the UI on 8080 and the server on 7777, the server bind-mounting `.:/work` so the agent operates on the user's project, and `ZORP_BASE_URL`/`ZORP_MODEL`/`ZORP_API_KEY` passed through from the host environment.

- [ ] **Step 4: Verify**

```bash
docker compose up --build -d
curl -fsS localhost:7777/api/health
docker compose down
```

Expected: `{"status":"ok"}`.

- [ ] **Step 5: Document and commit**

Add a "Web UI" section to `README.md` covering the local command, the compose path, and the sentence that matters: exposing the server on a network interface gives whoever reaches it agent-driven shell access to that machine.

```bash
git add zorp-web/Dockerfile web/Dockerfile compose.yml README.md
git commit -m "feat(web): containerize the server and UI separately"
```

---

## Self-Review

**Spec coverage.** D1 (local bind, token gate) is Task 1 Step 5 and Task 7 Step 1. D2 (separate artifacts, CORS-able base URL) is Task 6 Step 1 and Task 7. D3 (construct, do not wrap) is Task 3 Step 3. D4 (SSE) is Task 3 Step 4. D5 (approvals in v1) is Task 4. D6 (plain TypeScript, esbuild) is Task 6. D7 (session sidebar) is Task 5 and Task 6 Step 2. Error handling from the spec is covered: reconnect by `Last-Event-ID` in Task 3 Step 4, model failure as an `error` event in Task 3 Step 3, approval timeout in Task 4 Step 5, and second-turn rejection in Task 3 Step 6.

**Placeholders.** Tasks 3 to 7 describe implementations in prose rather than complete code blocks, because their bodies depend on axum API details that should be read from the installed version rather than guessed here. Each step names the exact types, functions, and files involved. Task 1 and Task 2, where the interfaces other tasks depend on are established, carry full code.

**Type consistency.** `Event`/`EventKind` in Task 2 are the only event types, and Tasks 3 and 4 add variants declared in that same enum. `WebRenderer::new(Sender<Event>)` is used unchanged in Task 3.
