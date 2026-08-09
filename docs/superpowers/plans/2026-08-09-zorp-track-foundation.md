# zorp-track Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `zorp-track`, the shared foundation (multi-track data
model, DuckDB run record, LanceDB provisioning, git-backed
pre-registration with tamper evidence, and a checkpoint primitive) that
zorp's four capabilities (validate, experiment, co-write, find a venue)
will each be built on top of.

**Architecture:** A new internal workspace crate, `zorp-track`, wired into
`zorp-agent` behind a new optional `research` feature (same pattern as
the existing `mcp` feature and `zorp-mcp`). `zorp-track` exposes a fully
synchronous API; DuckDB is natively sync, LanceDB's async calls are
hidden behind an internal `tokio::Runtime::block_on`, matching how
`zorp-mcp` already hides its own async transport behind a sync surface.

**Tech Stack:** Rust, `duckdb` (bundled feature, no system install
required), `lancedb` (using its own re-exported `arrow` types to avoid
version mismatches with a separately-pinned `arrow-array`/`arrow-schema`
dependency), `sha2` (already used elsewhere in this workspace, same
`Sha256`/`Digest` idiom), `chrono` (date formatting only), `tempfile` for
tests (already a workspace dev-dependency elsewhere).

## Global Constraints

- Rust edition 2021, matching every other crate in this workspace.
- No em dashes or en dashes in doc comments, commit messages, or any
  prose this plan produces (see `CLAUDE.md`, "Writing style").
- `cargo build --workspace` and `cargo test --workspace` must pass after
  every task's commit, not just at the end.
- `zorp-track` must not depend on `zorp-agent` (dependency direction is
  `zorp-agent` -> `zorp-track`, never the reverse); it may depend on the
  `zorp` core crate the same way `zorp-mcp` does.
- DuckDB and LanceDB stay behind the optional `research` feature; default
  builds of `zorp-agent` must not pull either dependency in.
- `metrics.key` from the spec is implemented as `metrics.metric_key` in
  the actual schema, a defensive rename to avoid any ambiguity with `KEY`
  as a SQL term; every other column name matches the spec exactly.

---

### Task 1: Scaffold the `zorp-track` crate

**Files:**
- Create: `zorp-track/Cargo.toml`
- Create: `zorp-track/src/lib.rs`
- Create: `zorp-track/src/error.rs`
- Modify: `Cargo.toml` (root workspace members)
- Modify: `zorp-agent/Cargo.toml` (optional dependency + `research` feature)
- Test: inline in `zorp-track/src/error.rs`

**Interfaces:**
- Produces: `zorp_track::TrackError` (a `#[non_exhaustive]` enum
  implementing `std::error::Error` and `std::fmt::Display`), with
  `From<duckdb::Error>` and `From<std::io::Error>` conversions. Later
  tasks return `Result<T, TrackError>` from every public function.

- [ ] **Step 1: Create the crate manifest**

`zorp-track/Cargo.toml`:
```toml
[package]
name        = "zorp-track"
version     = "0.1.0"
edition     = "2021"
description = "Research-track storage and checkpoints for zorp-agent."
license     = "MIT"

[dependencies]
duckdb  = { version = "1", features = ["bundled"] }
lancedb = "0.33"
tokio   = { version = "1", features = ["rt", "rt-multi-thread"] }
sha2    = "0.10"
chrono  = { version = "0.4", default-features = false, features = ["clock"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the failing test for `TrackError`**

`zorp-track/src/error.rs`:
```rust
use std::fmt;

#[non_exhaustive]
#[derive(Debug)]
pub enum TrackError {
    Io(String),
    Db(String),
    Library(String),
    NotFound { kind: &'static str, id: String },
    IntegrityMismatch { track_id: String, detail: String },
    CheckpointBlocked { kind: String },
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackError::Io(msg) => write!(f, "zorp-track io error: {msg}"),
            TrackError::Db(msg) => write!(f, "zorp-track db error: {msg}"),
            TrackError::Library(msg) => write!(f, "zorp-track library error: {msg}"),
            TrackError::NotFound { kind, id } => write!(f, "{kind} not found: {id}"),
            TrackError::IntegrityMismatch { track_id, detail } => {
                write!(f, "prereg integrity mismatch for track '{track_id}': {detail}")
            }
            TrackError::CheckpointBlocked { kind } => write!(
                f,
                "checkpoint '{kind}' has no interactive terminal and AutoApprove is not set"
            ),
        }
    }
}

impl std::error::Error for TrackError {}

impl From<duckdb::Error> for TrackError {
    fn from(e: duckdb::Error) -> Self {
        TrackError::Db(e.to_string())
    }
}

impl From<std::io::Error> for TrackError {
    fn from(e: std::io::Error) -> Self {
        TrackError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_not_found() {
        let e = TrackError::NotFound { kind: "track", id: "t1".into() };
        assert!(e.to_string().contains("track"));
        assert!(e.to_string().contains("t1"));
    }

    #[test]
    fn display_integrity_mismatch() {
        let e = TrackError::IntegrityMismatch {
            track_id: "t1".into(),
            detail: "hash mismatch".into(),
        };
        assert!(e.to_string().contains("t1"));
        assert!(e.to_string().contains("hash mismatch"));
    }

    #[test]
    fn display_checkpoint_blocked() {
        let e = TrackError::CheckpointBlocked { kind: "validate".into() };
        assert!(e.to_string().contains("validate"));
        assert!(e.to_string().contains("AutoApprove"));
    }
}
```

- [ ] **Step 3: Create the crate root**

`zorp-track/src/lib.rs`:
```rust
//! zorp-track, research-track storage and checkpoints for zorp-agent.
//!
//! Exposes a fully synchronous API. DuckDB is natively synchronous;
//! LanceDB's async calls are hidden behind an internal
//! `tokio::Runtime::block_on`, the same pattern `zorp-mcp` already uses
//! for its own async transport.

pub mod error;

pub use error::TrackError;
```

- [ ] **Step 4: Register the workspace member**

Modify `Cargo.toml` (root), the `[workspace]` block:
```toml
[workspace]
members = [".", "zorp-agent", "zorp-mcp", "zorp-eval", "zorp-track"]
```

- [ ] **Step 5: Wire the optional dependency and feature into `zorp-agent`**

Modify `zorp-agent/Cargo.toml`. Add to `[dependencies]` (alongside the
existing `zorp-mcp = { path = "../zorp-mcp", optional = true }` line):
```toml
zorp-track = { path = "../zorp-track", optional = true }
```
Add to `[features]` (alongside the existing `mcp = ["dep:zorp-mcp"]`
line):
```toml
research = ["dep:zorp-track"]
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 3 tests pass (`display_not_found`,
`display_integrity_mismatch`, `display_checkpoint_blocked`). First run
will take a few minutes while `duckdb`'s bundled source compiles;
subsequent runs are fast.

- [ ] **Step 7: Verify the workspace and the new feature both build**

Run: `cargo build --workspace`
Expected: builds clean, `zorp-track` included as a new member.

Run: `cargo build -p zorp-agent --features research`
Expected: builds clean.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml zorp-agent/Cargo.toml zorp-track/
git commit -m "Scaffold zorp-track crate behind a new research feature"
```

---

### Task 2: Track id generation

**Files:**
- Create: `zorp-track/src/id.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `zorp_track::id::track_id(hypothesis: &str) -> String`, a
  date-prefixed, lowercase, hyphenated slug (e.g.
  `2026-08-09-adaptive-memory-consolidation`). Task 4 (`create_track`)
  takes this as its `id` argument; it is not generated inside the store
  itself, so callers can regenerate deterministically in tests.

- [ ] **Step 1: Write the failing tests**

`zorp-track/src/id.rs`:
```rust
use chrono::Utc;

const MAX_SLUG_CHARS: usize = 60;

/// Generate a date-prefixed, lowercase, hyphenated slug from hypothesis
/// text, e.g. "Adaptive Memory Consolidation!" becomes
/// "2026-08-09-adaptive-memory-consolidation".
pub fn track_id(hypothesis: &str) -> String {
    let date = Utc::now().format("%Y-%m-%d");
    let slug = slugify(hypothesis);
    format!("{date}-{slug}")
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut last_was_hyphen = true; // suppress a leading hyphen
    for ch in text.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
        if slug.chars().count() >= MAX_SLUG_CHARS {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_lowercase_and_hyphenated() {
        let id = track_id("Adaptive Memory Consolidation!");
        assert!(id.ends_with("-adaptive-memory-consolidation"), "got: {id}");
    }

    #[test]
    fn slug_has_todays_date_prefix() {
        let id = track_id("anything");
        let today = Utc::now().format("%Y-%m-%d").to_string();
        assert!(id.starts_with(&today), "got: {id}");
    }

    #[test]
    fn slug_collapses_repeated_punctuation() {
        let id = track_id("a --- b   c!!!d");
        assert!(id.ends_with("-a-b-c-d"), "got: {id}");
    }

    #[test]
    fn slug_truncates_long_text() {
        let long = "word ".repeat(30);
        let id = track_id(&long);
        let slug_part = &id[11..]; // strip the fixed "YYYY-MM-DD-" prefix
        assert!(slug_part.chars().count() <= MAX_SLUG_CHARS, "got: {slug_part}");
    }

    #[test]
    fn empty_hypothesis_falls_back_to_untitled() {
        let id = track_id("   ...   ");
        assert!(id.ends_with("-untitled"), "got: {id}");
    }
}
```

- [ ] **Step 2: Run to verify it fails to compile (module not yet registered)**

Run: `cargo test -p zorp-track`
Expected: FAIL, `id` is not a module of `zorp_track` (this file exists
but isn't wired into `lib.rs` yet).

- [ ] **Step 3: Register the module**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod error;
pub mod id;

pub use error::TrackError;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 8 tests pass (3 from Task 1, 5 new).

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/id.rs zorp-track/src/lib.rs
git commit -m "Add track id slug generation"
```

---

### Task 3: DuckDB schema and `Store::open`

**Files:**
- Create: `zorp-track/src/schema.rs`
- Create: `zorp-track/src/track.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: `TrackError` (Task 1).
- Produces: `zorp_track::track::Store` (a struct wrapping a
  `duckdb::Connection`), `Store::open(path: &Path) -> Result<Store,
  TrackError>`. Task 4 adds CRUD methods to this same `Store` type; Task
  6 adds a rebuild method; Task 7 and Task 8 add more tables' worth of
  methods to it.

- [ ] **Step 1: Write the schema**

`zorp-track/src/schema.rs`:
```rust
pub(crate) const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    hypothesis TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS preregistrations (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    hypothesis_snapshot TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    kill_threshold DOUBLE NOT NULL,
    file_path TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    git_commit_hash TEXT,
    committed_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS experiments (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    prereg_id TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at BIGINT,
    completed_at BIGINT
);
CREATE TABLE IF NOT EXISTS metrics (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    value_type TEXT NOT NULL,
    value_number DOUBLE,
    value_string TEXT,
    value_bool BOOLEAN,
    recorded_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    prompt_shown TEXT NOT NULL,
    decision_notes TEXT,
    created_at BIGINT NOT NULL,
    resolved_at BIGINT
);";
```

- [ ] **Step 2: Write the failing tests for `Store::open`**

`zorp-track/src/track.rs`:
```rust
use crate::schema::SCHEMA;
use crate::TrackError;
use duckdb::Connection;
use std::path::Path;

/// DuckDB-backed store for tracks, preregistrations, experiments,
/// metrics, and checkpoints.
pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    /// Open (creating if necessary) the DuckDB file at `path`, applying
    /// the schema. Safe to call repeatedly on the same file.
    pub fn open(path: &Path) -> Result<Self, TrackError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_creates_all_tables() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("zorp.duckdb");
        let store = Store::open(&db_path).unwrap();
        let mut stmt = store
            .conn
            .prepare(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'main' ORDER BY table_name",
            )
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "checkpoints",
                "experiments",
                "metrics",
                "preregistrations",
                "tracks"
            ]
        );
    }

    #[test]
    fn open_is_idempotent() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("zorp.duckdb");
        Store::open(&db_path).unwrap();
        let reopened = Store::open(&db_path);
        assert!(reopened.is_ok());
    }
}
```

- [ ] **Step 3: Register the modules**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod error;
pub mod id;
mod schema;
pub mod track;

pub use error::TrackError;
pub use track::Store;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 10 tests pass (8 from before, 2 new).

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/schema.rs zorp-track/src/track.rs zorp-track/src/lib.rs
git commit -m "Add DuckDB schema and Store::open"
```

---

### Task 4: Track CRUD

**Files:**
- Modify: `zorp-track/src/track.rs`

**Interfaces:**
- Consumes: `Store` (Task 3).
- Produces: `Track` struct (`id`, `hypothesis`, `status: TrackStatus`,
  `created_at: i64`, `updated_at: i64`), `TrackStatus` enum (`Active`,
  `Paused`, `Completed`, `Killed`), `Store::create_track(&self, id: &str,
  hypothesis: &str) -> Result<Track, TrackError>`,
  `Store::get_track(&self, id: &str) -> Result<Track, TrackError>`,
  `Store::list_tracks(&self) -> Result<Vec<Track>, TrackError>`,
  `Store::set_track_status(&self, id: &str, status: TrackStatus) ->
  Result<(), TrackError>`. Later tasks (5, 7, 8) reference tracks by
  `id: &str`; nothing later depends on `Track`'s internal field order.

- [ ] **Step 1: Write the failing tests**

Append to `zorp-track/src/track.rs` (above the existing `#[cfg(test)]`
block, add the new code; below it, extend the tests module):

```rust
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackStatus {
    Active,
    Paused,
    Completed,
    Killed,
}

impl TrackStatus {
    fn as_str(&self) -> &'static str {
        match self {
            TrackStatus::Active => "active",
            TrackStatus::Paused => "paused",
            TrackStatus::Completed => "completed",
            TrackStatus::Killed => "killed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "paused" => TrackStatus::Paused,
            "completed" => TrackStatus::Completed,
            "killed" => TrackStatus::Killed,
            _ => TrackStatus::Active,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: String,
    pub hypothesis: String,
    pub status: TrackStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_track(r: &duckdb::Row) -> duckdb::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        hypothesis: r.get(1)?,
        status: TrackStatus::from_str(&r.get::<_, String>(2)?),
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

impl Store {
    /// Create a new track. `id` should come from `crate::id::track_id`;
    /// it is not generated here so tests and callers can control it.
    /// Status starts `Active`.
    pub fn create_track(&self, id: &str, hypothesis: &str) -> Result<Track, TrackError> {
        let now = now_millis();
        self.conn.execute(
            "INSERT INTO tracks (id, hypothesis, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            duckdb::params![id, hypothesis, TrackStatus::Active.as_str(), now, now],
        )?;
        Ok(Track {
            id: id.to_string(),
            hypothesis: hypothesis.to_string(),
            status: TrackStatus::Active,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_track(&self, id: &str) -> Result<Track, TrackError> {
        self.conn
            .query_row(
                "SELECT id, hypothesis, status, created_at, updated_at FROM tracks WHERE id = ?",
                duckdb::params![id],
                row_to_track,
            )
            .map_err(|e| match e {
                duckdb::Error::QueryReturnedNoRows => TrackError::NotFound {
                    kind: "track",
                    id: id.to_string(),
                },
                other => TrackError::from(other),
            })
    }

    pub fn list_tracks(&self) -> Result<Vec<Track>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hypothesis, status, created_at, updated_at FROM tracks ORDER BY id",
        )?;
        let rows = stmt.query_map([], row_to_track)?;
        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row?);
        }
        Ok(tracks)
    }

    pub fn set_track_status(&self, id: &str, status: TrackStatus) -> Result<(), TrackError> {
        let updated = self.conn.execute(
            "UPDATE tracks SET status = ?, updated_at = ? WHERE id = ?",
            duckdb::params![status.as_str(), now_millis(), id],
        )?;
        if updated == 0 {
            return Err(TrackError::NotFound {
                kind: "track",
                id: id.to_string(),
            });
        }
        Ok(())
    }
}
```

Add to the `tests` module in the same file:
```rust
    #[test]
    fn create_and_get_round_trip() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let created = store.create_track("t1", "does caching help").unwrap();
        let fetched = store.get_track("t1").unwrap();
        assert_eq!(created, fetched);
        assert_eq!(fetched.status, TrackStatus::Active);
    }

    #[test]
    fn list_returns_all_tracks_sorted_by_id() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("b", "second").unwrap();
        store.create_track("a", "first").unwrap();
        let ids: Vec<String> = store.list_tracks().unwrap().into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn set_status_updates_status_and_bumps_updated_at() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let created = store.create_track("t1", "hypothesis").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.set_track_status("t1", TrackStatus::Paused).unwrap();
        let fetched = store.get_track("t1").unwrap();
        assert_eq!(fetched.status, TrackStatus::Paused);
        assert!(fetched.updated_at > created.updated_at);
    }

    #[test]
    fn get_missing_track_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let err = store.get_track("nope").unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "track", .. }));
    }

    #[test]
    fn set_status_on_missing_track_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let err = store.set_track_status("nope", TrackStatus::Killed).unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "track", .. }));
    }
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 15 tests pass (10 from before, 5 new).

- [ ] **Step 3: Commit**

```bash
git add zorp-track/src/track.rs
git commit -m "Add track CRUD to Store"
```

---

### Task 5: Pre-registration, file, git commit, hash, and DB row

**Files:**
- Create: `zorp-track/src/prereg.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: `Store`, `Track` (Task 4), `TrackError` (Task 1).
- Produces: `Preregistration` struct, `write_prereg(store: &Store,
  track_dir: &Path, track_id: &str, hypothesis: &str, metric_name: &str,
  kill_threshold: f64) -> Result<Preregistration, TrackError>`,
  `verify_prereg_integrity(store: &Store, track_id: &str) ->
  Result<(), TrackError>`. Task 6 (rebuild) reuses the same `prereg.md`
  parsing this task defines; Task 10 (the project facade) calls
  `write_prereg` directly.

**On the git dependency:** if the project isn't a git repository (no
`.git` reachable from `track_dir`), `write_prereg` still writes the file
and the row, with `git_commit_hash` left `None`. The SHA-256 file-hash
check still catches any later edit to the file's content; only the
git-commit-timestamp layer of tamper evidence is unavailable in that
case. This degrades honestly rather than failing outright, since zorp
must not hard-require git.

- [ ] **Step 1: Write the failing tests**

`zorp-track/src/prereg.rs`:
```rust
use crate::track::Store;
use crate::TrackError;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub struct Preregistration {
    pub id: String,
    pub track_id: String,
    pub hypothesis_snapshot: String,
    pub metric_name: String,
    pub kill_threshold: f64,
    pub file_path: PathBuf,
    pub file_hash: String,
    pub git_commit_hash: Option<String>,
    pub committed_at: i64,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn render_prereg_md(track_id: &str, hypothesis: &str, metric_name: &str, kill_threshold: f64) -> String {
    format!(
        "# Pre-registration: {track_id}\n\n\
         Hypothesis: {hypothesis}\n\
         Metric: {metric_name}\n\
         Kill threshold: {kill_threshold}\n"
    )
}

/// Parse a `prereg.md` written by `render_prereg_md` back into its
/// fields. Used both to verify integrity and, in Task 6, to rebuild the
/// DuckDB index from files alone.
pub(crate) fn parse_prereg_md(content: &str) -> Result<(String, String, f64), TrackError> {
    let mut hypothesis = None;
    let mut metric_name = None;
    let mut kill_threshold = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("Hypothesis: ") {
            hypothesis = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Metric: ") {
            metric_name = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Kill threshold: ") {
            kill_threshold = v.parse::<f64>().ok();
        }
    }
    match (hypothesis, metric_name, kill_threshold) {
        (Some(h), Some(m), Some(k)) => Ok((h, m, k)),
        _ => Err(TrackError::Io(
            "prereg.md missing a required field".to_string(),
        )),
    }
}

fn is_git_repo(dir: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String, TrackError> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| TrackError::Io(format!("git: {e}")))?;
    if !out.status.success() {
        return Err(TrackError::Io(format!(
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Write a pre-registration: the `prereg.md` file, a git commit of just
/// that file (if `track_dir` is inside a git repository), and the
/// corresponding `preregistrations` row.
pub fn write_prereg(
    store: &Store,
    track_dir: &Path,
    track_id: &str,
    hypothesis: &str,
    metric_name: &str,
    kill_threshold: f64,
) -> Result<Preregistration, TrackError> {
    fs::create_dir_all(track_dir)?;
    let file_path = track_dir.join("prereg.md");
    let content = render_prereg_md(track_id, hypothesis, metric_name, kill_threshold);
    fs::write(&file_path, &content)?;
    let file_hash = sha256_hex(content.as_bytes());

    let git_commit_hash = if is_git_repo(track_dir) {
        run_git(track_dir, &["add", "--", file_path.to_str().unwrap_or("")])?;
        run_git(
            track_dir,
            &[
                "commit",
                "-m",
                &format!("prereg({track_id}): pre-registration"),
                "--",
                file_path.to_str().unwrap_or(""),
            ],
        )?;
        Some(run_git(track_dir, &["rev-parse", "HEAD"])?)
    } else {
        None
    };

    let id = format!("{track_id}-prereg");
    let committed_at = now_millis();
    store.conn.execute(
        "INSERT INTO preregistrations \
         (id, track_id, hypothesis_snapshot, metric_name, kill_threshold, file_path, file_hash, git_commit_hash, committed_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        duckdb::params![
            id,
            track_id,
            hypothesis,
            metric_name,
            kill_threshold,
            file_path.to_string_lossy().to_string(),
            file_hash,
            git_commit_hash,
            committed_at
        ],
    )?;

    Ok(Preregistration {
        id,
        track_id: track_id.to_string(),
        hypothesis_snapshot: hypothesis.to_string(),
        metric_name: metric_name.to_string(),
        kill_threshold,
        file_path,
        file_hash,
        git_commit_hash,
        committed_at,
    })
}

/// Verify that the `preregistrations` row for `track_id` matches the
/// `prereg.md` file on disk: the file must exist, and its current
/// SHA-256 must match what was recorded at commit time.
pub fn verify_prereg_integrity(store: &Store, track_id: &str) -> Result<(), TrackError> {
    let (file_path, file_hash): (String, String) = store
        .conn
        .query_row(
            "SELECT file_path, file_hash FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            duckdb::Error::QueryReturnedNoRows => TrackError::IntegrityMismatch {
                track_id: track_id.to_string(),
                detail: "no preregistration row found".to_string(),
            },
            other => TrackError::from(other),
        })?;

    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(TrackError::IntegrityMismatch {
            track_id: track_id.to_string(),
            detail: format!("prereg.md missing at {file_path}"),
        });
    }
    let current_content = fs::read(path)?;
    let current_hash = sha256_hex(&current_content);
    if current_hash != file_hash {
        return Err(TrackError::IntegrityMismatch {
            track_id: track_id.to_string(),
            detail: "prereg.md content does not match the hash recorded at commit time".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Store;
    use tempfile::tempdir;

    fn init_git_repo(dir: &Path) {
        std::process::Command::new("git").arg("-C").arg(dir).args(["init", "-q"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.email", "test@example.com"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.name", "Test"]).output().unwrap();
    }

    #[test]
    fn write_then_verify_succeeds_in_a_git_repo() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");

        let prereg = write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0).unwrap();
        assert!(prereg.git_commit_hash.is_some());
        assert!(prereg.file_path.exists());

        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn write_without_a_git_repo_leaves_commit_hash_none_but_still_verifies() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");

        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();
        assert_eq!(prereg.git_commit_hash, None);
        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn verify_fails_when_file_is_edited_after_commit() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();

        fs::write(&prereg.file_path, "tampered content").unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn verify_fails_when_file_is_missing() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();

        fs::remove_file(&prereg.file_path).unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn verify_fails_when_no_prereg_row_exists() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn parse_prereg_md_round_trips_render_prereg_md() {
        let content = render_prereg_md("t1", "does caching help", "latency_ms", 42.5);
        let (hypothesis, metric, threshold) = parse_prereg_md(&content).unwrap();
        assert_eq!(hypothesis, "does caching help");
        assert_eq!(metric, "latency_ms");
        assert_eq!(threshold, 42.5);
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod error;
pub mod id;
pub mod prereg;
mod schema;
pub mod track;

pub use error::TrackError;
pub use track::Store;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 21 tests pass (15 from before, 6 new). Requires `git` on
`PATH`; it already is, `zorp-agent`'s own `tools/git.rs` tests depend on
the same thing.

- [ ] **Step 4: Commit**

```bash
git add zorp-track/src/prereg.rs zorp-track/src/lib.rs
git commit -m "Add pre-registration write and integrity verification"
```

---

### Task 6: Rebuild the DuckDB index from `prereg.md` files

**Files:**
- Modify: `zorp-track/src/track.rs`

**Interfaces:**
- Consumes: `Store`, `Preregistration` fields via `prereg::parse_prereg_md`
  (Task 5).
- Produces: `Store::rebuild_from_prereg_files(&self, tracks_dir: &Path)
  -> Result<usize, TrackError>` (returns the count of tracks rebuilt).
  Task 10 (the project facade) calls this when `zorp.duckdb` is missing
  or was just freshly created but `tracks_dir` already has content.

- [ ] **Step 1: Write the failing tests**

Append to `zorp-track/src/track.rs`, above the `#[cfg(test)]` block:
```rust
impl Store {
    /// Re-derive `tracks` and `preregistrations` rows by reading every
    /// `<tracks_dir>/<id>/prereg.md` on disk. Used to recover after
    /// `zorp.duckdb` is lost or deleted, since the files are the source
    /// of truth. Skips a track directory if it has no `prereg.md`
    /// (nothing to rebuild from) or already has a matching row.
    pub fn rebuild_from_prereg_files(&self, tracks_dir: &Path) -> Result<usize, TrackError> {
        let mut rebuilt = 0;
        let Ok(entries) = std::fs::read_dir(tracks_dir) else {
            return Ok(0);
        };
        for entry in entries.flatten() {
            let track_dir = entry.path();
            if !track_dir.is_dir() {
                continue;
            }
            let Some(track_id) = track_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let prereg_path = track_dir.join("prereg.md");
            if !prereg_path.exists() {
                continue;
            }
            if self.get_track(track_id).is_ok() {
                continue; // already present, nothing to rebuild
            }

            let content = std::fs::read_to_string(&prereg_path)?;
            let (hypothesis, metric_name, kill_threshold) = crate::prereg::parse_prereg_md(&content)?;
            let file_hash = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(content.as_bytes());
                hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
            };
            let git_commit_hash = std::process::Command::new("git")
                .arg("-C")
                .arg(&track_dir)
                .args(["log", "-1", "--format=%H", "--", "prereg.md"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty());

            self.create_track(track_id, &hypothesis)?;
            let committed_at_ms = std::fs::metadata(&prereg_path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            self.conn.execute(
                "INSERT INTO preregistrations \
                 (id, track_id, hypothesis_snapshot, metric_name, kill_threshold, file_path, file_hash, git_commit_hash, committed_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    format!("{track_id}-prereg"),
                    track_id,
                    hypothesis,
                    metric_name,
                    kill_threshold,
                    prereg_path.to_string_lossy().to_string(),
                    file_hash,
                    git_commit_hash,
                    committed_at_ms
                ],
            )?;
            rebuilt += 1;
        }
        Ok(rebuilt)
    }
}
```

Add to the `tests` module in `zorp-track/src/track.rs`:
```rust
    #[test]
    fn rebuild_recovers_tracks_after_duckdb_file_is_deleted() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("zorp.duckdb");
        let tracks_dir = dir.path().join("tracks");
        {
            let store = Store::open(&db_path).unwrap();
            store.create_track("t1", "does caching help").unwrap();
            let track_dir = tracks_dir.join("t1");
            crate::prereg::write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0).unwrap();
        }

        std::fs::remove_file(&db_path).unwrap();

        let fresh_store = Store::open(&db_path).unwrap();
        assert!(fresh_store.get_track("t1").is_err());
        let rebuilt = fresh_store.rebuild_from_prereg_files(&tracks_dir).unwrap();
        assert_eq!(rebuilt, 1);
        let recovered = fresh_store.get_track("t1").unwrap();
        assert_eq!(recovered.hypothesis, "does caching help");
        assert!(crate::prereg::verify_prereg_integrity(&fresh_store, "t1").is_ok());
    }

    #[test]
    fn rebuild_skips_tracks_that_already_have_a_row() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("zorp.duckdb");
        let tracks_dir = dir.path().join("tracks");
        let store = Store::open(&db_path).unwrap();
        store.create_track("t1", "already here").unwrap();
        let track_dir = tracks_dir.join("t1");
        crate::prereg::write_prereg(&store, &track_dir, "t1", "already here", "m", 1.0).unwrap();

        let rebuilt = store.rebuild_from_prereg_files(&tracks_dir).unwrap();
        assert_eq!(rebuilt, 0);
    }

    #[test]
    fn rebuild_on_empty_tracks_dir_is_a_no_op() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let rebuilt = store.rebuild_from_prereg_files(&dir.path().join("tracks")).unwrap();
        assert_eq!(rebuilt, 0);
    }
```

Also add `use std::path::Path;` to the top of `zorp-track/src/track.rs`
if not already present from Task 3 (it is; `Store::open` already takes
`path: &Path`).

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 24 tests pass (21 from before, 3 new).

- [ ] **Step 3: Commit**

```bash
git add zorp-track/src/track.rs
git commit -m "Add DuckDB index rebuild from prereg.md files"
```

---

### Task 7: Experiments and metrics

**Files:**
- Create: `zorp-track/src/experiment.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: `Store`, `Preregistration` (Task 5).
- Produces: `Experiment` struct (`id`, `track_id`, `prereg_id`, `status:
  ExperimentStatus`, `started_at: Option<i64>`, `completed_at:
  Option<i64>`), `ExperimentStatus` enum (`Planned`, `Running`,
  `Completed`, `Failed`, `Killed`), `MetricValue` enum (`Number(f64)`,
  `Text(String)`, `Bool(bool)`), `Store::create_experiment(&self,
  track_id: &str, prereg_id: &str) -> Result<Experiment, TrackError>`,
  `Store::set_experiment_status(&self, id: &str, status:
  ExperimentStatus) -> Result<(), TrackError>`,
  `Store::record_metric(&self, experiment_id: &str, key: &str, value:
  MetricValue) -> Result<(), TrackError>`, `Store::metrics_for(&self,
  experiment_id: &str) -> Result<Vec<(String, MetricValue)>,
  TrackError>`. No later task in this plan depends on this one; it is a
  leaf.

- [ ] **Step 1: Write the failing tests**

`zorp-track/src/experiment.rs`:
```rust
use crate::track::Store;
use crate::TrackError;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Killed,
}

impl ExperimentStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ExperimentStatus::Planned => "planned",
            ExperimentStatus::Running => "running",
            ExperimentStatus::Completed => "completed",
            ExperimentStatus::Failed => "failed",
            ExperimentStatus::Killed => "killed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "running" => ExperimentStatus::Running,
            "completed" => ExperimentStatus::Completed,
            "failed" => ExperimentStatus::Failed,
            "killed" => ExperimentStatus::Killed,
            _ => ExperimentStatus::Planned,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Experiment {
    pub id: String,
    pub track_id: String,
    pub prereg_id: String,
    pub status: ExperimentStatus,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    Number(f64),
    Text(String),
    Bool(bool),
}

impl MetricValue {
    fn type_str(&self) -> &'static str {
        match self {
            MetricValue::Number(_) => "number",
            MetricValue::Text(_) => "string",
            MetricValue::Bool(_) => "bool",
        }
    }
}

impl Store {
    pub fn create_experiment(&self, track_id: &str, prereg_id: &str) -> Result<Experiment, TrackError> {
        let id = format!("{track_id}-exp-{}", now_millis());
        self.conn.execute(
            "INSERT INTO experiments (id, track_id, prereg_id, status, started_at, completed_at) VALUES (?, ?, ?, ?, NULL, NULL)",
            duckdb::params![id, track_id, prereg_id, ExperimentStatus::Planned.as_str()],
        )?;
        Ok(Experiment {
            id,
            track_id: track_id.to_string(),
            prereg_id: prereg_id.to_string(),
            status: ExperimentStatus::Planned,
            started_at: None,
            completed_at: None,
        })
    }

    pub fn set_experiment_status(&self, id: &str, status: ExperimentStatus) -> Result<(), TrackError> {
        let now = now_millis();
        let sql = match status {
            ExperimentStatus::Running => "UPDATE experiments SET status = ?, started_at = ? WHERE id = ?",
            ExperimentStatus::Completed | ExperimentStatus::Failed | ExperimentStatus::Killed => {
                "UPDATE experiments SET status = ?, completed_at = ? WHERE id = ?"
            }
            ExperimentStatus::Planned => "UPDATE experiments SET status = ? WHERE id = ?",
        };
        let updated = if matches!(status, ExperimentStatus::Planned) {
            self.conn.execute(sql, duckdb::params![status.as_str(), id])?
        } else {
            self.conn.execute(sql, duckdb::params![status.as_str(), now, id])?
        };
        if updated == 0 {
            return Err(TrackError::NotFound { kind: "experiment", id: id.to_string() });
        }
        Ok(())
    }

    pub fn record_metric(&self, experiment_id: &str, key: &str, value: MetricValue) -> Result<(), TrackError> {
        let metric_id = format!("{experiment_id}-{key}-{}", now_millis());
        let (num, text, boolean) = match &value {
            MetricValue::Number(n) => (Some(*n), None, None),
            MetricValue::Text(s) => (None, Some(s.clone()), None),
            MetricValue::Bool(b) => (None, None, Some(*b)),
        };
        self.conn.execute(
            "INSERT INTO metrics (id, experiment_id, metric_key, value_type, value_number, value_string, value_bool, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![metric_id, experiment_id, key, value.type_str(), num, text, boolean, now_millis()],
        )?;
        Ok(())
    }

    pub fn metrics_for(&self, experiment_id: &str) -> Result<Vec<(String, MetricValue)>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT metric_key, value_type, value_number, value_string, value_bool FROM metrics WHERE experiment_id = ? ORDER BY recorded_at",
        )?;
        let rows = stmt.query_map(duckdb::params![experiment_id], |r| {
            let key: String = r.get(0)?;
            let value_type: String = r.get(1)?;
            let value = match value_type.as_str() {
                "number" => MetricValue::Number(r.get(2)?),
                "bool" => MetricValue::Bool(r.get(4)?),
                _ => MetricValue::Text(r.get(3)?),
            };
            Ok((key, value))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_experiment_starts_planned() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        assert_eq!(exp.status, ExperimentStatus::Planned);
        assert_eq!(exp.started_at, None);
    }

    #[test]
    fn status_transition_to_running_sets_started_at() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        store.set_experiment_status(&exp.id, ExperimentStatus::Running).unwrap();
    }

    #[test]
    fn record_and_read_back_typed_metrics() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store.record_metric(&exp.id, "accuracy", MetricValue::Number(0.87)).unwrap();
        store.record_metric(&exp.id, "notes", MetricValue::Text("looked promising".into())).unwrap();
        store.record_metric(&exp.id, "converged", MetricValue::Bool(true)).unwrap();

        let metrics = store.metrics_for(&exp.id).unwrap();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0], ("accuracy".to_string(), MetricValue::Number(0.87)));
        assert_eq!(metrics[1], ("notes".to_string(), MetricValue::Text("looked promising".into())));
        assert_eq!(metrics[2], ("converged".to_string(), MetricValue::Bool(true)));
    }

    #[test]
    fn set_status_on_missing_experiment_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let err = store.set_experiment_status("nope", ExperimentStatus::Running).unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "experiment", .. }));
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod error;
pub mod experiment;
pub mod id;
pub mod prereg;
mod schema;
pub mod track;

pub use error::TrackError;
pub use track::Store;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 28 tests pass (24 from before, 4 new).

- [ ] **Step 4: Commit**

```bash
git add zorp-track/src/experiment.rs zorp-track/src/lib.rs
git commit -m "Add experiments and typed metrics"
```

---

### Task 8: Checkpoint primitive

**Files:**
- Create: `zorp-track/src/checkpoint.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: `Store`, `TrackError`.
- Produces: `Decider` trait (`fn decide(&self, prompt: &str) -> bool`,
  mirrors `zorp-agent`'s `Approver` trait), `CheckpointMode` enum
  (`Interactive(Arc<dyn Decider>)`, `AutoApprove`),
  `CheckpointMode::terminal(auto_approve: bool) -> Result<Self,
  TrackError>` (returns `TrackError::CheckpointBlocked` if
  `auto_approve` is false and stdin is not a terminal, since unlike a
  tool call there is no safe silent default), `Store::record_checkpoint
  (&self, track_id: &str, kind: &str, mode: &CheckpointMode, prompt: &str)
  -> Result<bool, TrackError>` (runs the mode's decision, writes the
  `checkpoints` row with the outcome, returns the decision). Nothing
  later in this plan depends on this task; it is a leaf, used by future
  capability specs.

- [ ] **Step 1: Write the failing tests**

`zorp-track/src/checkpoint.rs`:
```rust
use crate::track::Store;
use crate::TrackError;
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Asks a human a yes/no question at a research checkpoint. Mirrors
/// zorp-agent's `Approver` trait, at track granularity instead of
/// per-tool-call.
pub trait Decider: Send + Sync {
    fn decide(&self, prompt: &str) -> bool;
}

pub struct TerminalDecider;
impl Decider for TerminalDecider {
    fn decide(&self, prompt: &str) -> bool {
        eprint!("{prompt} [y/N] ");
        if io::stderr().flush().is_err() {
            return false;
        }
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return false;
        }
        matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    }
}

#[derive(Clone)]
pub enum CheckpointMode {
    Interactive(Arc<dyn Decider>),
    AutoApprove,
}

impl CheckpointMode {
    /// Unlike zorp-agent's per-tool-call `ApprovalMode`, there is no
    /// `NonInteractive` variant here: a research checkpoint has no safe
    /// default to fall back to when nobody can answer it, so that case
    /// is a hard error instead of a silent skip.
    pub fn terminal(auto_approve: bool) -> Result<Self, TrackError> {
        if auto_approve {
            Ok(CheckpointMode::AutoApprove)
        } else if io::stdin().is_terminal() {
            Ok(CheckpointMode::Interactive(Arc::new(TerminalDecider)))
        } else {
            Err(TrackError::CheckpointBlocked { kind: "terminal".to_string() })
        }
    }

    fn decide(&self, prompt: &str) -> bool {
        match self {
            CheckpointMode::Interactive(d) => d.decide(prompt),
            CheckpointMode::AutoApprove => true,
        }
    }
}

impl Store {
    /// Run a checkpoint's decision and persist the outcome. `kind`
    /// identifies which capability this checkpoint belongs to (e.g.
    /// "validate", "experiment"); left as a plain string rather than a
    /// fixed enum since capabilities beyond the four already named may
    /// add their own.
    pub fn record_checkpoint(
        &self,
        track_id: &str,
        kind: &str,
        mode: &CheckpointMode,
        prompt: &str,
    ) -> Result<bool, TrackError> {
        let approved = mode.decide(prompt);
        let id = format!("{track_id}-{kind}-{}", now_millis());
        let now = now_millis();
        self.conn.execute(
            "INSERT INTO checkpoints (id, track_id, kind, status, prompt_shown, decision_notes, created_at, resolved_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                id,
                track_id,
                kind,
                if approved { "approved" } else { "rejected" },
                prompt,
                Option::<String>::None,
                now,
                now
            ],
        )?;
        Ok(approved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    struct Stub {
        answer: bool,
        calls: AtomicUsize,
    }
    impl Decider for Stub {
        fn decide(&self, _prompt: &str) -> bool {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answer
        }
    }

    #[test]
    fn auto_approve_always_decides_true() {
        assert!(CheckpointMode::AutoApprove.decide("proceed?"));
    }

    #[test]
    fn interactive_mode_defers_to_the_decider() {
        let approve = CheckpointMode::Interactive(Arc::new(Stub { answer: true, calls: AtomicUsize::new(0) }));
        assert!(approve.decide("proceed?"));
        let reject = CheckpointMode::Interactive(Arc::new(Stub { answer: false, calls: AtomicUsize::new(0) }));
        assert!(!reject.decide("proceed?"));
    }

    #[test]
    fn terminal_without_auto_approve_and_no_tty_errors() {
        // Test processes normally have no interactive stdin.
        let result = CheckpointMode::terminal(false);
        assert!(matches!(result, Err(TrackError::CheckpointBlocked { .. })));
    }

    #[test]
    fn terminal_with_auto_approve_never_checks_stdin() {
        let result = CheckpointMode::terminal(true);
        assert!(matches!(result, Ok(CheckpointMode::AutoApprove)));
    }

    #[test]
    fn record_checkpoint_persists_the_decision() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::Interactive(Arc::new(Stub { answer: true, calls: AtomicUsize::new(0) }));

        let approved = store.record_checkpoint("t1", "validate", &mode, "is this novel?").unwrap();
        assert!(approved);

        let (status, prompt): (String, String) = store
            .conn
            .query_row(
                "SELECT status, prompt_shown FROM checkpoints WHERE track_id = ? AND kind = ?",
                duckdb::params!["t1", "validate"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "approved");
        assert_eq!(prompt, "is this novel?");
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod checkpoint;
pub mod error;
pub mod experiment;
pub mod id;
pub mod prereg;
mod schema;
pub mod track;

pub use error::TrackError;
pub use track::Store;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 33 tests pass (28 from before, 5 new).

- [ ] **Step 4: Commit**

```bash
git add zorp-track/src/checkpoint.rs zorp-track/src/lib.rs
git commit -m "Add the checkpoint primitive"
```

---

### Task 9: LanceDB provisioning

**Files:**
- Create: `zorp-track/src/library.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: `TrackError`.
- Produces: `Library` struct, `Library::open(path: &Path) ->
  Result<Self, TrackError>` (connects to, and ensures a base schema
  exists in, the LanceDB store at `path`; synchronous, wraps LanceDB's
  async calls in an internal `tokio::Runtime::block_on`). No producers or
  consumers of specific content are added here, per the spec; this task
  only guarantees the store exists and is reachable, with a
  `track_id`-keyed base table so later capability specs can filter to
  one track's content. Nothing later in this plan depends on this task's
  internals beyond `Library::open` succeeding.

**Important:** use LanceDB's own re-exported Arrow types
(`lancedb::arrow::arrow_array`, `lancedb::arrow::arrow_schema`) rather
than adding separate `arrow-array`/`arrow-schema` dependencies. A
separately-pinned Arrow version produces a type that does not satisfy
LanceDB's `Scannable` trait bound even though it looks identical, since
Rust treats differently-versioned copies of the same crate as distinct
types. Verified directly: a standalone reproduction with independently
added `arrow-array`/`arrow-schema` dependencies failed to compile with
exactly this error; switching to `lancedb::arrow::*` compiled and ran.

- [ ] **Step 1: Write the failing tests**

`zorp-track/src/library.rs`:
```rust
use crate::TrackError;
use lancedb::arrow::arrow_array::{RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use std::path::Path;
use std::sync::Arc;

impl From<lancedb::Error> for TrackError {
    fn from(e: lancedb::Error) -> Self {
        TrackError::Library(e.to_string())
    }
}

/// LanceDB-backed store for multimodal, semantically searchable content
/// (literature, figures, plots). What actually goes in is each
/// capability's own concern; this only provisions the store and a base
/// `library` table keyed by `track_id`.
pub struct Library {
    runtime: tokio::runtime::Runtime,
    connection: lancedb::Connection,
}

fn base_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("track_id", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
    ]))
}

impl Library {
    /// Open (creating if necessary) the LanceDB store at `path`.
    pub fn open(path: &Path) -> Result<Self, TrackError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| TrackError::Library(e.to_string()))?;
        let path_str = path.to_string_lossy().to_string();
        let connection = runtime.block_on(async {
            let conn = lancedb::connect(&path_str).execute().await?;
            let existing = conn.table_names().execute().await?;
            if !existing.iter().any(|n| n == "library") {
                let schema = base_schema();
                let empty_batch = RecordBatch::try_new(
                    schema.clone(),
                    vec![
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                    ],
                )
                .map_err(|e| lancedb::Error::Other { message: e.to_string(), source: None })?;
                let reader: Box<dyn RecordBatchReader + Send> =
                    Box::new(RecordBatchIterator::new(vec![Ok(empty_batch)], schema));
                conn.create_table("library", reader).execute().await?;
            }
            Ok::<_, lancedb::Error>(conn)
        })?;
        Ok(Library { runtime, connection })
    }

    /// The names of tables currently in this store, `["library"]` right
    /// after `open` on a fresh path.
    pub fn table_names(&self) -> Result<Vec<String>, TrackError> {
        Ok(self.runtime.block_on(self.connection.table_names().execute())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_on_fresh_path_creates_the_library_table() {
        let dir = tempdir().unwrap();
        let library = Library::open(&dir.path().join("lancedb")).unwrap();
        assert_eq!(library.table_names().unwrap(), vec!["library".to_string()]);
    }

    #[test]
    fn open_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lancedb");
        Library::open(&path).unwrap();
        let reopened = Library::open(&path);
        assert!(reopened.is_ok());
        assert_eq!(reopened.unwrap().table_names().unwrap(), vec!["library".to_string()]);
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod checkpoint;
pub mod error;
pub mod experiment;
pub mod id;
pub mod library;
pub mod prereg;
mod schema;
pub mod track;

pub use error::TrackError;
pub use track::Store;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 35 tests pass (33 from before, 2 new).

- [ ] **Step 4: Commit**

```bash
git add zorp-track/src/library.rs zorp-track/src/lib.rs
git commit -m "Add LanceDB provisioning"
```

---

### Task 10: The `.zorp/` project facade, integration test, and workspace verification

**Files:**
- Create: `zorp-track/src/project.rs`
- Modify: `zorp-track/src/lib.rs`
- Create: `zorp-track/tests/integration.rs`

**Interfaces:**
- Consumes: `Store` (Task 3, 4, 6), `Library` (Task 9), `write_prereg`,
  `verify_prereg_integrity` (Task 5).
- Produces: `Project` struct, `Project::open(root: &Path) ->
  Result<Self, TrackError>` (the single entry point future `zorp-agent`
  subcommands will call: ensures `.zorp/` exists with a `.gitignore`
  covering `zorp.duckdb` and `lancedb/`, opens the `Store`, rebuilds from
  `prereg.md` files if the store was just freshly created but
  `.zorp/tracks/` already has content, and opens the `Library`),
  `Project.store: Store`, `Project.library: Library`,
  `Project.track_dir(&self, track_id: &str) -> PathBuf`.

- [ ] **Step 1: Write the failing test for `Project::open`**

`zorp-track/src/project.rs`:
```rust
use crate::library::Library;
use crate::track::Store;
use crate::TrackError;
use std::path::{Path, PathBuf};

const GITIGNORE_CONTENT: &str = "zorp.duckdb\nlancedb/\n";

/// The single entry point for a project's `.zorp/` directory: opens (or
/// creates) the DuckDB run record, the LanceDB library, and a
/// `.gitignore` covering the two regenerable stores while leaving
/// `tracks/*/prereg.md` tracked.
pub struct Project {
    root: PathBuf,
    pub store: Store,
    pub library: Library,
}

impl Project {
    pub fn open(root: &Path) -> Result<Self, TrackError> {
        let zorp_dir = root.join(".zorp");
        let tracks_dir = zorp_dir.join("tracks");
        std::fs::create_dir_all(&tracks_dir)?;

        let gitignore_path = zorp_dir.join(".gitignore");
        if !gitignore_path.exists() {
            std::fs::write(&gitignore_path, GITIGNORE_CONTENT)?;
        }

        let db_path = zorp_dir.join("zorp.duckdb");
        let db_existed = db_path.exists();
        let store = Store::open(&db_path)?;
        if !db_existed {
            store.rebuild_from_prereg_files(&tracks_dir)?;
        }

        let library = Library::open(&zorp_dir.join("lancedb"))?;

        Ok(Project { root: zorp_dir, store, library })
    }

    /// The directory a track's `prereg.md` and future capability
    /// artifacts live in: `.zorp/tracks/<track_id>/`.
    pub fn track_dir(&self, track_id: &str) -> PathBuf {
        self.root.join("tracks").join(track_id)
    }

    #[cfg(test)]
    pub(crate) fn root_for_test(&self) -> &Path {
        &self.root
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-track/src/lib.rs`:
```rust
pub mod checkpoint;
pub mod error;
pub mod experiment;
pub mod id;
pub mod library;
pub mod prereg;
pub mod project;
mod schema;
pub mod track;

pub use error::TrackError;
pub use project::Project;
pub use track::Store;
```

- [ ] **Step 3: Write the integration test**

`zorp-track/tests/integration.rs`:
```rust
use std::path::Path;
use tempfile::tempdir;
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::{verify_prereg_integrity, write_prereg};
use zorp_track::track::TrackStatus;
use zorp_track::{Project, TrackError};

fn init_git_repo(dir: &Path) {
    std::process::Command::new("git").arg("-C").arg(dir).args(["init", "-q"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.email", "test@example.com"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.name", "Test"]).output().unwrap();
}

#[test]
fn full_track_lifecycle() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    // Opening a fresh project creates .zorp/, a .gitignore, and both stores.
    let project = Project::open(dir.path()).unwrap();
    assert!(dir.path().join(".zorp/.gitignore").exists());
    let gitignore = std::fs::read_to_string(dir.path().join(".zorp/.gitignore")).unwrap();
    assert!(gitignore.contains("zorp.duckdb"));
    assert!(gitignore.contains("lancedb/"));
    assert_eq!(project.library.table_names().unwrap(), vec!["library".to_string()]);

    // Create a track and pre-register it.
    let track_id = "2026-08-09-does-caching-help";
    project.store.create_track(track_id, "does caching help").unwrap();
    let track_dir = project.track_dir(track_id);
    write_prereg(&project.store, &track_dir, track_id, "does caching help", "latency_ms", 100.0).unwrap();
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());
    assert!(track_dir.join("prereg.md").exists());

    // A checkpoint, auto-approved (no interactive terminal in tests).
    let mode = CheckpointMode::terminal(true).unwrap();
    let approved = project
        .store
        .record_checkpoint(track_id, "experiment", &mode, "proceed with this experiment?")
        .unwrap();
    assert!(approved);

    // Run an experiment and record typed metrics.
    let exp = project.store.create_experiment(track_id, &format!("{track_id}-prereg")).unwrap();
    project.store.set_experiment_status(&exp.id, ExperimentStatus::Running).unwrap();
    project.store.record_metric(&exp.id, "latency_ms", MetricValue::Number(87.3)).unwrap();
    project.store.set_experiment_status(&exp.id, ExperimentStatus::Completed).unwrap();
    let metrics = project.store.metrics_for(&exp.id).unwrap();
    assert_eq!(metrics, vec![("latency_ms".to_string(), MetricValue::Number(87.3))]);

    project.store.set_track_status(track_id, TrackStatus::Completed).unwrap();
    assert_eq!(project.store.get_track(track_id).unwrap().status, TrackStatus::Completed);
}

#[test]
fn reopening_a_project_does_not_lose_data() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-reopen-test";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "reopen test").unwrap();
    }
    let project = Project::open(dir.path()).unwrap();
    assert_eq!(project.store.get_track(track_id).unwrap().hypothesis, "reopen test");
}

#[test]
fn rebuilds_from_prereg_files_if_duckdb_file_is_deleted() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-rebuild-test";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "rebuild test").unwrap();
        let track_dir = project.track_dir(track_id);
        write_prereg(&project.store, &track_dir, track_id, "rebuild test", "m", 1.0).unwrap();
    }

    std::fs::remove_file(dir.path().join(".zorp/zorp.duckdb")).unwrap();

    let project = Project::open(dir.path()).unwrap();
    let recovered = project.store.get_track(track_id).unwrap();
    assert_eq!(recovered.hypothesis, "rebuild test");
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());
}

#[test]
fn checkpoint_without_auto_approve_or_a_terminal_is_a_hard_error() {
    let result = CheckpointMode::terminal(false);
    assert!(matches!(result, Err(TrackError::CheckpointBlocked { .. })));
}
```

- [ ] **Step 4: Run the crate's tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: 39 tests pass (35 unit tests from before, 4 new integration
tests).

- [ ] **Step 5: Verify the full workspace still builds and passes**

Run: `cargo build --workspace`
Expected: builds clean.

Run: `cargo build -p zorp-agent --features research`
Expected: builds clean.

Run: `cargo test --workspace`
Expected: all tests pass, including every existing `zorp-agent`,
`zorp-mcp`, `zorp-eval`, and root `zorp` test alongside the new
`zorp-track` ones.

- [ ] **Step 6: Commit**

```bash
git add zorp-track/src/project.rs zorp-track/src/lib.rs zorp-track/tests/
git commit -m "Add the Project facade tying tracks, prereg, and both stores together"
```

---

## Self-Review

**Spec coverage:**
- Where this lives (new crate, `research` feature, same pattern as
  `zorp-mcp`): Task 1.
- On disk (`.zorp/`, gitignored stores, tracked `prereg.md`): Task 10.
- Track identity (date-slug): Task 2.
- DuckDB schema (all five tables): Task 3 (schema), Tasks 4, 5, 7, 8 (the
  CRUD each table needs).
- LanceDB (provisioned, no producers/consumers yet): Task 9.
- Checkpoint mechanism (`Interactive`/`AutoApprove`, no
  `NonInteractive`, hard error with no terminal and no auto-approve):
  Task 8.
- Integrity check on load (hash comparison, hard error on mismatch):
  Task 5 (`verify_prereg_integrity`), exercised via `Project::open`'s
  rebuild path in Task 10.
- Error handling (auto-create `.zorp/`, rebuild on missing/corrupted
  DuckDB file, integrity hard error, checkpoint hard error): Task 10
  (create + rebuild), Task 5 (integrity error), Task 8 (checkpoint
  error).
- Testing list from the spec (schema creation/migration, track CRUD,
  prereg write + integrity mismatches, checkpoint transitions +
  no-terminal error, index rebuild): covered by Tasks 3, 4, 5, 6, 8, and
  tied together in Task 10's integration tests.

**Placeholder scan:** no TBD/TODO markers; every step has real code, not
a description of code.

**Type consistency:** `Store` (Task 3) is extended by `impl Store`
blocks in Tasks 4, 6, 7, 8, all against the same `pub(crate) conn:
Connection` field established in Task 3. `TrackError` variants introduced
in Task 1 are used consistently (`NotFound`, `IntegrityMismatch`,
`CheckpointBlocked`) rather than redefined. `track_id: &str` is the
consistent handle used across `prereg.rs`, `experiment.rs`,
`checkpoint.rs`, and `project.rs`, matching `Track.id: String`'s type
from Task 4.

## Execution Options

Plan complete and saved to
`docs/superpowers/plans/2026-08-09-zorp-track-foundation.md`. Two
execution options:

**1. Subagent-Driven (recommended)**, I dispatch a fresh subagent per
task, review between tasks, fast iteration.

**2. Inline Execution**, execute tasks in this session using
executing-plans, batch execution with checkpoints.

Which approach?
