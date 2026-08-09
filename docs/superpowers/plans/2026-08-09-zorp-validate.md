# validate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `validate`, the first of zorp's four capabilities: given a
question, search using whatever tools are available (MCP-provided search
included, via the existing `Agent`/`attach_mcp_tools` machinery), score
redundancy and feasibility with required citations, embed cited sources
into LanceDB, and checkpoint before the track proceeds or gets killed.

**Architecture:** Two new `zorp-track` methods (`Store::record_validation`,
`Library::insert_source`) extend the foundation to hold validate's output.
A new `zorp-agent` library module, `validate`, holds the orchestration
logic and is unit-testable against a stub `Model` (the `Model` trait
already used by `Agent::new`, no real HTTP needed for tests). A new
`zorp-agent validate "<question>"` subcommand in `main.rs` wires a real
`Agent` (with MCP tools attached, same as `chat` already does) to
`validate::run`.

**Tech Stack:** Rust, `duckdb`, `lancedb` (both already zorp-track
dependencies), `serde_json` (already a dependency everywhere), no new
crates.

## Global Constraints

- Rust edition 2021, no em dashes or en dashes in doc comments, commit
  messages, or prose.
- `cargo build --workspace`, `cargo build -p zorp-agent --features
  research`, and `cargo test --workspace` must pass after every task's
  commit.
- `zorp-agent` already has its own `Store` (session persistence,
  `zorp-agent/src/session.rs`) distinct from `zorp_track::Store` (track
  persistence). Any file that uses both must alias one, e.g. `use
  zorp_track::Store as TrackStore` or `use zorp_agent::Store as
  SessionStore`. Never import both unaliased in the same file.
- Reuse existing primitives; do not duplicate `attach_mcp_tools`,
  `extract_fenced_block`, `Agent`, `Outcome`, or `zorp::zorp_raw`/`join_url`.
- The `validate` module and its new `zorp-agent` code live behind the
  existing `research` feature (the same feature `zorp-track` is already
  gated behind).

---

### Task 1: `validations` table and `Store::record_validation`

**Files:**
- Modify: `zorp-track/src/schema.rs`
- Modify: `zorp-track/src/experiment.rs` (no changes; referenced for
  pattern only)
- Create: `zorp-track/src/validation.rs`
- Modify: `zorp-track/src/lib.rs`

**Interfaces:**
- Consumes: `Store` (`zorp-track/src/track.rs`), `TrackError`.
- Produces: `Citation { text: String, source: String }`,
  `Validation { id, track_id, redundancy_score: f64,
  redundancy_citations: Vec<Citation>, feasibility_score: f64,
  feasibility_citations: Vec<Citation>, verdict: String, created_at: i64
  }`, `Store::record_validation(&self, track_id: &str, redundancy_score:
  f64, redundancy_citations: &[Citation], feasibility_score: f64,
  feasibility_citations: &[Citation], verdict: &str) -> Result<Validation,
  TrackError>`, `Store::get_validation(&self, track_id: &str) ->
  Result<Validation, TrackError>`. Task 7 (the orchestration function in
  `zorp-agent`) calls both.

- [ ] **Step 1: Add the table to the schema**

Modify `zorp-track/src/schema.rs`. Append to the `SCHEMA` constant
(after the `checkpoints` table, still inside the same string, before the
closing `";`):

```rust
CREATE TABLE IF NOT EXISTS validations (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    redundancy_score DOUBLE NOT NULL,
    redundancy_citations TEXT NOT NULL,
    feasibility_score DOUBLE NOT NULL,
    feasibility_citations TEXT NOT NULL,
    verdict TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
```

Citations are stored as a JSON-encoded `TEXT` column (a list of
`Citation`), not a joined table; see the design spec's "Out of scope."

- [ ] **Step 2: Write the failing tests**

`zorp-track/src/validation.rs`:

```rust
use crate::track::Store;
use crate::TrackError;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub id: String,
    pub track_id: String,
    pub redundancy_score: f64,
    pub redundancy_citations: Vec<Citation>,
    pub feasibility_score: f64,
    pub feasibility_citations: Vec<Citation>,
    pub verdict: String,
    pub created_at: i64,
}

fn citations_to_json(citations: &[Citation]) -> String {
    serde_json::to_string(citations).unwrap_or_else(|_| "[]".to_string())
}

fn citations_from_json(raw: &str) -> Vec<Citation> {
    serde_json::from_str(raw).unwrap_or_default()
}

impl Store {
    pub fn record_validation(
        &self,
        track_id: &str,
        redundancy_score: f64,
        redundancy_citations: &[Citation],
        feasibility_score: f64,
        feasibility_citations: &[Citation],
        verdict: &str,
    ) -> Result<Validation, TrackError> {
        let id = format!("{track_id}-validation");
        let created_at = now_millis();
        let redundancy_json = citations_to_json(redundancy_citations);
        let feasibility_json = citations_to_json(feasibility_citations);
        self.conn.execute(
            "INSERT INTO validations \
             (id, track_id, redundancy_score, redundancy_citations, feasibility_score, feasibility_citations, verdict, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                id,
                track_id,
                redundancy_score,
                redundancy_json,
                feasibility_score,
                feasibility_json,
                verdict,
                created_at
            ],
        )?;
        Ok(Validation {
            id,
            track_id: track_id.to_string(),
            redundancy_score,
            redundancy_citations: redundancy_citations.to_vec(),
            feasibility_score,
            feasibility_citations: feasibility_citations.to_vec(),
            verdict: verdict.to_string(),
            created_at,
        })
    }

    pub fn get_validation(&self, track_id: &str) -> Result<Validation, TrackError> {
        self.conn
            .query_row(
                "SELECT id, track_id, redundancy_score, redundancy_citations, feasibility_score, feasibility_citations, verdict, created_at \
                 FROM validations WHERE track_id = ?",
                duckdb::params![track_id],
                |r| {
                    let redundancy_raw: String = r.get(3)?;
                    let feasibility_raw: String = r.get(5)?;
                    Ok(Validation {
                        id: r.get(0)?,
                        track_id: r.get(1)?,
                        redundancy_score: r.get(2)?,
                        redundancy_citations: citations_from_json(&redundancy_raw),
                        feasibility_score: r.get(4)?,
                        feasibility_citations: citations_from_json(&feasibility_raw),
                        verdict: r.get(6)?,
                        created_at: r.get(7)?,
                    })
                },
            )
            .map_err(|e| match e {
                duckdb::Error::QueryReturnedNoRows => TrackError::NotFound {
                    kind: "validation",
                    id: track_id.to_string(),
                },
                other => TrackError::from(other),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn citation(text: &str, source: &str) -> Citation {
        Citation { text: text.to_string(), source: source.to_string() }
    }

    #[test]
    fn record_and_get_round_trip() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();

        let red = vec![citation("no prior benchmark found", "search result 1")];
        let feas = vec![citation("a benchmark harness already exists", "repo README")];
        let recorded = store
            .record_validation("t1", 20.0, &red, 85.0, &feas, "worth investigating")
            .unwrap();

        let fetched = store.get_validation("t1").unwrap();
        assert_eq!(recorded, fetched);
        assert_eq!(fetched.redundancy_citations, red);
        assert_eq!(fetched.feasibility_citations, feas);
    }

    #[test]
    fn get_missing_validation_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let err = store.get_validation("t1").unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "validation", .. }));
    }

    #[test]
    fn empty_citations_round_trip_as_empty() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        store.record_validation("t1", 0.0, &[], 0.0, &[], "no evidence found").unwrap();
        let fetched = store.get_validation("t1").unwrap();
        assert!(fetched.redundancy_citations.is_empty());
        assert!(fetched.feasibility_citations.is_empty());
    }
}
```

Add `serde = { version = "1", features = ["derive"] }` and
`serde_json = "1"` to `zorp-track/Cargo.toml`'s `[dependencies]` if not
already present (check first; `zorp-track` may not have `serde_json`
yet even though `serde` was added in an earlier task for a different
reason. Add whichever is missing).

- [ ] **Step 2: Register the module**

Modify `zorp-track/src/lib.rs`, add `pub mod validation;` in
alphabetical position (after `track`, so: ..., `pub mod track;`,
`pub mod validation;`), and `pub use validation::{Citation, Validation};`
near the other `pub use` lines.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: existing tests still pass, plus 3 new (`record_and_get_round_trip`,
`get_missing_validation_errors`, `empty_citations_round_trip_as_empty`).

- [ ] **Step 4: Commit**

```bash
git add zorp-track/src/schema.rs zorp-track/src/validation.rs zorp-track/src/lib.rs zorp-track/Cargo.toml
git commit -m "Add validations table and Store::record_validation"
```

---

### Task 2: `Library::insert_source`

**Files:**
- Modify: `zorp-track/src/library.rs`

**Interfaces:**
- Consumes: `Library` (Task 9 of the foundation plan), `TrackError`.
- Produces: `Library::insert_source(&self, track_id: &str, kind: &str,
  text: &str, embedding: &[f32]) -> Result<(), TrackError>`. Task 7 (the
  `zorp-agent` orchestration function) calls this once per cited source.

- [ ] **Step 1: Write the failing tests**

Add to `zorp-track/src/library.rs`, above the existing `#[cfg(test)]`
block:

```rust
use lancedb::arrow::arrow_array::FixedSizeListArray;
use lancedb::arrow::arrow_schema::Field as ArrowField;

fn source_schema(dim: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("track_id", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(ArrowField::new("item", DataType::Float32, true)), dim),
            false,
        ),
    ]))
}

fn source_batch(schema: Arc<Schema>, track_id: &str, kind: &str, text: &str, embedding: &[f32]) -> Result<RecordBatch, TrackError> {
    let dim = embedding.len() as i32;
    let values = lancedb::arrow::arrow_array::Float32Array::from(embedding.to_vec());
    let vector_array = FixedSizeListArray::try_new(
        Arc::new(ArrowField::new("item", DataType::Float32, true)),
        dim,
        Arc::new(values),
        None,
    )
    .map_err(|e| TrackError::Library(e.to_string()))?;
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![track_id])),
            Arc::new(StringArray::from(vec![kind])),
            Arc::new(StringArray::from(vec![text])),
            Arc::new(vector_array),
        ],
    )
    .map_err(|e| TrackError::Library(e.to_string()))
}

impl Library {
    /// Embed and store one source. Lazily creates the `sources` table on
    /// the first call, with its vector column's dimension inferred from
    /// that first `embedding`'s length. Later calls append; passing an
    /// embedding of a different length than the table's fixed dimension
    /// is a `TrackError::Library` error, not a silent failure.
    pub fn insert_source(&self, track_id: &str, kind: &str, text: &str, embedding: &[f32]) -> Result<(), TrackError> {
        self.runtime.block_on(async {
            let existing = self.connection.table_names().execute().await?;
            let schema = source_schema(embedding.len() as i32);
            let batch = source_batch(schema.clone(), track_id, kind, text, embedding)?;
            let reader: Box<dyn RecordBatchReader + Send> =
                Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema));
            if existing.iter().any(|n| n == "sources") {
                let tbl = self.connection.open_table("sources").execute().await?;
                tbl.add(reader).execute().await?;
            } else {
                self.connection.create_table("sources", reader).execute().await?;
            }
            Ok::<(), TrackError>(())
        })
    }
}
```

Add to the `tests` module in the same file:

```rust
    #[test]
    fn insert_source_creates_the_table_on_first_call() {
        let dir = tempdir().unwrap();
        let library = Library::open(&dir.path().join("lancedb")).unwrap();
        library.insert_source("t1", "validate-source", "a snippet", &[0.1, 0.2, 0.3]).unwrap();
        let names = library.table_names().unwrap();
        assert!(names.contains(&"sources".to_string()));
    }

    #[test]
    fn insert_source_appends_on_second_call() {
        let dir = tempdir().unwrap();
        let library = Library::open(&dir.path().join("lancedb")).unwrap();
        library.insert_source("t1", "validate-source", "first", &[0.1, 0.2]).unwrap();
        library.insert_source("t1", "validate-source", "second", &[0.3, 0.4]).unwrap();
        let count = library.runtime.block_on(async {
            library.connection.open_table("sources").execute().await.unwrap().count_rows(None).await.unwrap()
        });
        assert_eq!(count, 2);
    }
```

The second test reaches into `library.runtime`/`library.connection`
directly; both are private fields on the same struct, and this test is
in the same module (`#[cfg(test)] mod tests` inside `library.rs`), so
this compiles without changing either field's visibility.

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p zorp-track`
Expected: existing tests pass, plus 2 new.

- [ ] **Step 3: Commit**

```bash
git add zorp-track/src/library.rs
git commit -m "Add Library::insert_source with lazy table creation"
```

---

### Task 3: Embeddings

**Files:**
- Create: `zorp-agent/src/embed.rs`
- Modify: `zorp-agent/src/lib.rs`

**Interfaces:**
- Consumes: `zorp::join_url`, `zorp::zorp_raw` (already `pub`, `zorp-agent`
  already depends on the `zorp` core crate).
- Produces: `embed_request_body(model: &str, texts: &[String]) -> Value`
  (pure), `parse_embedding_response(resp: &Value, index: usize) ->
  Result<Vec<f32>, String>` (pure), `embed_texts(texts: &[String]) ->
  Result<Vec<Vec<f32>>, String>` (the thin I/O wrapper gluing both via
  `zorp::zorp_raw`, reading `ZORP_BASE_URL`, `ZORP_API_KEY`, and the new
  `ZORP_EMBEDDING_MODEL` env vars; not itself unit tested, the same
  posture `zorp::zorp_raw` already has elsewhere in this codebase, real
  HTTP is exercised by the manual/ignored integration test in Task 8,
  not a unit test). Task 6 (`validate::run`) calls `embed_texts`.

- [ ] **Step 1: Write the failing tests**

`zorp-agent/src/embed.rs`:

```rust
use serde_json::{json, Value};

/// Build an OpenAI-style embeddings request body.
pub fn embed_request_body(model: &str, texts: &[String]) -> Value {
    json!({ "model": model, "input": texts })
}

/// Extract the `index`-th embedding vector from an embeddings response.
pub fn parse_embedding_response(resp: &Value, index: usize) -> Result<Vec<f32>, String> {
    let data = resp
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "embeddings response missing data array".to_string())?;
    let entry = data
        .get(index)
        .ok_or_else(|| format!("embeddings response has no entry at index {index}"))?;
    let embedding = entry
        .get("embedding")
        .and_then(|e| e.as_array())
        .ok_or_else(|| format!("embeddings response entry {index} missing embedding array"))?;
    embedding
        .iter()
        .map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| format!("non-numeric value in embedding at index {index}")))
        .collect()
}

/// Embed each of `texts` in one request. Reads `ZORP_BASE_URL`,
/// `ZORP_API_KEY`, and `ZORP_EMBEDDING_MODEL` from the environment; a
/// missing `ZORP_EMBEDDING_MODEL` is an error, unlike `ZORP_BASE_URL`
/// and `ZORP_MODEL` (chat completions), which have defaults, embeddings
/// have no sensible default model to fall back to.
pub fn embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let base = std::env::var("ZORP_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let key = std::env::var("ZORP_API_KEY").ok();
    let model = std::env::var("ZORP_EMBEDDING_MODEL")
        .map_err(|_| "ZORP_EMBEDDING_MODEL is not set".to_string())?;

    let url = zorp::join_url(&base, "embeddings");
    let body = embed_request_body(&model, texts);
    let auth = key.map(|k| format!("Bearer {k}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    let resp = zorp::zorp_raw(&url, &headers, body).map_err(|e| e.to_string())?;

    (0..texts.len())
        .map(|i| parse_embedding_response(&resp, i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_request_body_shape() {
        let body = embed_request_body("text-embedding-3-small", &["a".to_string(), "b".to_string()]);
        assert_eq!(body["model"], "text-embedding-3-small");
        assert_eq!(body["input"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn parse_embedding_response_ok() {
        let resp = json!({ "data": [{ "embedding": [0.1, 0.2, 0.3] }] });
        let v = parse_embedding_response(&resp, 0).unwrap();
        assert_eq!(v.len(), 3);
        assert!((v[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn parse_embedding_response_missing_data_errs() {
        let resp = json!({ "error": "bad request" });
        assert!(parse_embedding_response(&resp, 0).is_err());
    }

    #[test]
    fn parse_embedding_response_index_out_of_range_errs() {
        let resp = json!({ "data": [{ "embedding": [0.1] }] });
        assert!(parse_embedding_response(&resp, 1).is_err());
    }

    #[test]
    fn parse_embedding_response_non_numeric_value_errs() {
        let resp = json!({ "data": [{ "embedding": ["not", "numbers"] }] });
        assert!(parse_embedding_response(&resp, 0).is_err());
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-agent/src/lib.rs`: add `mod embed;` in alphabetical
position (after `context`, before `flavor`), and
`pub use embed::{embed_request_body, embed_texts, parse_embedding_response};`
near the other `pub use` lines.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-agent embed::`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/embed.rs zorp-agent/src/lib.rs
git commit -m "Add embeddings request/response handling"
```

---

### Task 4: Validation result parsing

**Files:**
- Create: `zorp-agent/src/validate/result.rs`
- Create: `zorp-agent/src/validate/mod.rs` (stub, filled in by Task 6)
- Modify: `zorp-agent/src/lib.rs`

**Interfaces:**
- Consumes: `extract_fenced_block` (already in `zorp-agent`, via
  `capsule.rs`).
- Produces: `zorp_track::Citation` (reused directly, not redefined, see
  below), `ValidationResult { redundancy_score: f64,
  redundancy_citations: Vec<zorp_track::Citation>, feasibility_score:
  f64, feasibility_citations: Vec<zorp_track::Citation>, verdict: String
  }`, `parse_validation_result(agent_output: &str) ->
  Result<ValidationResult, ParseError>`, `ParseError` (a small enum:
  `NoFencedBlock`, `InvalidJson(String)`, `MissingCitation { dimension:
  &'static str }`). Task 6 (`validate::run`) calls
  `parse_validation_result` and converts a nonempty `ValidationResult`
  into the `zorp_track::Citation` values `Store::record_validation` and
  `Library::insert_source` both consume, so this task reuses
  `zorp_track::Citation` rather than defining a second, separate
  `Citation` type in `zorp-agent`.

**Note on `zorp_track::Citation` visibility:** this requires `zorp-agent`
depending on `zorp-track` for a type used outside the `Store`/`Library`
API surface, not just as an opaque handle. That's already the
relationship (`zorp-agent` depends on `zorp-track` behind the `research`
feature); this task's code is itself gated the same way (see Step 2).

- [ ] **Step 1: Write the failing tests**

`zorp-agent/src/validate/result.rs`:

```rust
use crate::capsule::extract_fenced_block;
use serde::Deserialize;
use std::fmt;
use zorp_track::Citation;

#[derive(Debug, Deserialize)]
struct RawValidationResult {
    redundancy_score: f64,
    redundancy_citations: Vec<Citation>,
    feasibility_score: f64,
    feasibility_citations: Vec<Citation>,
    verdict: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    pub redundancy_score: f64,
    pub redundancy_citations: Vec<Citation>,
    pub feasibility_score: f64,
    pub feasibility_citations: Vec<Citation>,
    pub verdict: String,
}

#[derive(Debug)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
    MissingCitation { dimension: &'static str },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => write!(f, "no fenced JSON block found in the agent's answer"),
            ParseError::InvalidJson(msg) => write!(f, "fenced block was not valid JSON: {msg}"),
            ParseError::MissingCitation { dimension } => {
                write!(f, "{dimension} has a nonzero score with no citation")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse the agent's final answer into a `ValidationResult`. Requires a
/// fenced block containing the expected JSON shape, and requires a
/// citation for any dimension scored above zero, the same "no citation,
/// no claim" discipline enforced here at parse time, not only prompted
/// for.
pub fn parse_validation_result(agent_output: &str) -> Result<ValidationResult, ParseError> {
    let block = extract_fenced_block(agent_output).map_err(|_| ParseError::NoFencedBlock)?;
    let raw: RawValidationResult =
        serde_json::from_str(&block).map_err(|e| ParseError::InvalidJson(e.to_string()))?;

    if raw.redundancy_score > 0.0 && raw.redundancy_citations.is_empty() {
        return Err(ParseError::MissingCitation { dimension: "redundancy" });
    }
    if raw.feasibility_score > 0.0 && raw.feasibility_citations.is_empty() {
        return Err(ParseError::MissingCitation { dimension: "feasibility" });
    }

    Ok(ValidationResult {
        redundancy_score: raw.redundancy_score,
        redundancy_citations: raw.redundancy_citations,
        feasibility_score: raw.feasibility_score,
        feasibility_citations: raw.feasibility_citations,
        verdict: raw.verdict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(json: &str) -> String {
        format!("Here is my finding.\n```json\n{json}\n```\n")
    }

    #[test]
    fn parses_a_well_formed_block() {
        let text = wrap(
            r#"{"redundancy_score": 20.0, "redundancy_citations": [{"text": "no prior work found", "source": "search 1"}], "feasibility_score": 85.0, "feasibility_citations": [{"text": "tooling exists", "source": "repo readme"}], "verdict": "worth investigating"}"#,
        );
        let result = parse_validation_result(&text).unwrap();
        assert_eq!(result.redundancy_score, 20.0);
        assert_eq!(result.feasibility_citations.len(), 1);
        assert_eq!(result.verdict, "worth investigating");
    }

    #[test]
    fn missing_block_errors() {
        let err = parse_validation_result("no block here at all").unwrap_err();
        assert!(matches!(err, ParseError::NoFencedBlock));
    }

    #[test]
    fn nonzero_score_with_no_citations_errors() {
        let text = wrap(
            r#"{"redundancy_score": 40.0, "redundancy_citations": [], "feasibility_score": 0.0, "feasibility_citations": [], "verdict": "unclear"}"#,
        );
        let err = parse_validation_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::MissingCitation { dimension: "redundancy" }));
    }

    #[test]
    fn zero_score_with_no_citations_is_fine() {
        let text = wrap(
            r#"{"redundancy_score": 0.0, "redundancy_citations": [], "feasibility_score": 0.0, "feasibility_citations": [], "verdict": "no evidence found"}"#,
        );
        assert!(parse_validation_result(&text).is_ok());
    }

    #[test]
    fn invalid_json_in_block_errors() {
        let text = wrap("{ not json");
        let err = parse_validation_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }
}
```

`zorp-agent/src/validate/mod.rs` (stub for this task; Task 6 adds
`run()` to it):

```rust
mod result;

pub use result::{parse_validation_result, ParseError, ValidationResult};
```

- [ ] **Step 2: Register the module behind the `research` feature**

Modify `zorp-agent/src/lib.rs`: add `#[cfg(feature = "research")] mod
validate;` in alphabetical position, and `#[cfg(feature = "research")]
pub use validate::{parse_validation_result, ParseError, ValidationResult};`
near the other `pub use` lines. `zorp_track` is already an optional,
`research`-feature-gated dependency (added when `zorp-track` itself was
scaffolded); this module is the first thing that actually uses it, so
this is also the first `#[cfg(feature = "research")]` gate on real code
in `zorp-agent`.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-agent --features research validate::`
Expected: 5 tests pass.

Run (confirm the default build, without `research`, still compiles):
`cargo build -p zorp-agent`
Expected: builds clean, no reference to the new module in a default
build.

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/validate/ zorp-agent/src/lib.rs
git commit -m "Add validation result parsing with required-citation checks"
```

---

### Task 5: `ValidateError`

**Files:**
- Modify: `zorp-agent/src/validate/mod.rs`
- Create: `zorp-agent/src/validate/error.rs`

**Interfaces:**
- Consumes: `Outcome` (already in `zorp-agent`), `ParseError` (Task 4),
  `zorp_track::TrackError`.
- Produces: `ValidateError` (`NoSearchTool`, `AgentOutcome(String)`
  built from a non-`Complete` `Outcome`'s debug representation,
  `Scoring(ParseError)`, `Track(zorp_track::TrackError)`, `Embedding(String)`),
  implementing `Display` and `std::error::Error`, with `From<ParseError>`,
  `From<zorp_track::TrackError>` conversions. Task 6 (`validate::run`)
  returns `Result<_, ValidateError>`.

- [ ] **Step 1: Write the failing tests**

`zorp-agent/src/validate/error.rs`:

```rust
use super::ParseError;
use std::fmt;

#[derive(Debug)]
pub enum ValidateError {
    NoSearchTool,
    AgentOutcome(String),
    Scoring(ParseError),
    Track(zorp_track::TrackError),
    Embedding(String),
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidateError::NoSearchTool => write!(
                f,
                "no search-capable tool is available; configure an MCP search server (--mcp or .zorp/mcp.toml)"
            ),
            ValidateError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            ValidateError::Scoring(e) => write!(f, "could not score the search results: {e}"),
            ValidateError::Track(e) => write!(f, "{e}"),
            ValidateError::Embedding(msg) => write!(f, "could not embed a cited source: {msg}"),
        }
    }
}

impl std::error::Error for ValidateError {}

impl From<ParseError> for ValidateError {
    fn from(e: ParseError) -> Self {
        ValidateError::Scoring(e)
    }
}

impl From<zorp_track::TrackError> for ValidateError {
    fn from(e: zorp_track::TrackError) -> Self {
        ValidateError::Track(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_no_search_tool_mentions_mcp() {
        let e = ValidateError::NoSearchTool;
        assert!(e.to_string().contains("MCP"));
    }

    #[test]
    fn display_agent_outcome_includes_the_outcome() {
        let e = ValidateError::AgentOutcome("StepLimit".to_string());
        assert!(e.to_string().contains("StepLimit"));
    }

    #[test]
    fn from_parse_error_wraps_correctly() {
        let e: ValidateError = ParseError::NoFencedBlock.into();
        assert!(matches!(e, ValidateError::Scoring(ParseError::NoFencedBlock)));
    }
}
```

- [ ] **Step 2: Register the module**

Modify `zorp-agent/src/validate/mod.rs`:

```rust
mod error;
mod result;

pub use error::ValidateError;
pub use result::{parse_validation_result, ParseError, ValidationResult};
```

Modify `zorp-agent/src/lib.rs`'s existing `#[cfg(feature = "research")]
pub use validate::{...}` line from Task 4 to also export `ValidateError`.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p zorp-agent --features research validate::`
Expected: 8 tests pass (5 from Task 4, 3 new).

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/validate/error.rs zorp-agent/src/validate/mod.rs zorp-agent/src/lib.rs
git commit -m "Add ValidateError"
```

---

### Task 6: `validate::run` orchestration

**Files:**
- Modify: `zorp-agent/src/validate/mod.rs`

**Interfaces:**
- Consumes: `Agent`, `Outcome` (`zorp-agent`), `zorp_track::Project`,
  `zorp_track::checkpoint::CheckpointMode`, `zorp_track::track::TrackStatus`,
  `embed_texts` (Task 3), `parse_validation_result` (Task 4),
  `ValidateError` (Task 5).
- Produces: `pub fn run(agent: &mut Agent, project: &zorp_track::Project,
  track_id: &str, question: &str, checkpoint_mode: &CheckpointMode) ->
  Result<bool, ValidateError>` (returns whether the checkpoint was
  approved). The caller (Task 7, `main.rs`'s new subcommand handler)
  builds `agent` with MCP tools already attached, and creates the track
  before calling this.

- [ ] **Step 1: Write the implementation**

Add to `zorp-agent/src/validate/mod.rs`:

```rust
mod error;
mod result;

pub use error::ValidateError;
pub use result::{parse_validation_result, ParseError, ValidationResult};

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::Project;

const TASK_PROMPT_PREFIX: &str = "\
Research the following question using whatever tools are available to you. \
Determine two things: (1) redundancy, has this question already been \
answered with enough confidence by something you found (a settled best \
practice, an existing analysis, a prior benchmark)? (2) feasibility, can \
this question actually be investigated further given what you found? \
Score each 0 to 100. Every score above 0 must be backed by at least one \
citation to something you actually found; a score with no citation is \
invalid. Cite the search result or source you're relying on for each \
claim.\n\n\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{\"redundancy_score\": <number>, \"redundancy_citations\": [{\"text\": \"<what it says>\", \"source\": \"<where it came from>\"}], \
\"feasibility_score\": <number>, \"feasibility_citations\": [...], \"verdict\": \"<one sentence>\"}\n\
```\n\n\
Question: ";

fn has_search_tool(agent: &Agent) -> bool {
    agent.tool_names().iter().any(|n| n.starts_with("mcp__"))
}

/// Run validate for an already-created track: search, score, embed
/// cited sources, record the validation, and checkpoint. Returns
/// whether the checkpoint was approved.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    question: &str,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, ValidateError> {
    if !has_search_tool(agent) {
        return Err(ValidateError::NoSearchTool);
    }

    let task = format!("{TASK_PROMPT_PREFIX}{question}");
    let outcome = agent.run(&task);
    let text = match outcome {
        Outcome::Complete(text) => text,
        other => return Err(ValidateError::AgentOutcome(format!("{other:?}"))),
    };

    let result = parse_validation_result(&text)?;

    for citation in result.redundancy_citations.iter().chain(result.feasibility_citations.iter()) {
        let embedding = crate::embed_texts(&[citation.text.clone()])
            .map_err(ValidateError::Embedding)?
            .into_iter()
            .next()
            .ok_or_else(|| ValidateError::Embedding("no embedding returned".to_string()))?;
        project
            .library
            .insert_source(track_id, "validate-source", &citation.text, &embedding)?;
    }

    project.store.record_validation(
        track_id,
        result.redundancy_score,
        &result.redundancy_citations,
        result.feasibility_score,
        &result.feasibility_citations,
        &result.verdict,
    )?;

    let prompt = format!(
        "validate: redundancy {:.0}/100, feasibility {:.0}/100. {}\nProceed to investigate?",
        result.redundancy_score, result.feasibility_score, result.verdict
    );
    let approved = project.store.record_checkpoint(track_id, "validate", checkpoint_mode, &prompt)?;
    if !approved {
        project.store.set_track_status(track_id, zorp_track::track::TrackStatus::Killed)?;
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
    }

    fn well_formed_response() -> String {
        "Findings below.\n```json\n{\"redundancy_score\": 10.0, \"redundancy_citations\": [{\"text\": \"nothing directly on point\", \"source\": \"search\"}], \"feasibility_score\": 90.0, \"feasibility_citations\": [{\"text\": \"tools are available\", \"source\": \"search\"}], \"verdict\": \"worth investigating\"}\n```\n".to_string()
    }

    #[test]
    fn no_search_tool_errors_before_calling_the_model() {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel { response: well_formed_response(), calls: calls.clone() };
        let mut agent = Agent::new(
            Box::new(model),
            "system",
            5,
            std::env::temp_dir(),
            crate::cancel_token(),
            crate::ApprovalMode::AutoApprove,
        )
        .register_builtins();
        // No MCP tools attached: only built-in local tools are present.

        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, ValidateError::NoSearchTool));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
```

Note: the one test in this task's own file only covers the
no-search-tool short-circuit, since a real search-capable tool requires
an MCP server (stub or real), which Task 8 sets up. This task's other
behavior (parsing, embedding, storing, checkpointing) is exercised in
Task 8's integration test, which has the fixtures this task's own tests
don't.

- [ ] **Step 2: Run the test to verify it passes**

Run: `cargo test -p zorp-agent --features research validate::`
Expected: 9 tests pass (8 from before, 1 new). Note: this requires
`AssistantMessage`, `Message`, `Model`, `BoxErr`, `Agent`, `cancel_token`,
`ApprovalMode` to be reachable via `crate::` paths inside `zorp-agent`;
if any of these are only available via the crate's public `pub use` and
not via their originating private module path, adjust the `use`
statements in the test to match whatever `zorp-agent/src/lib.rs`
actually exports (check `lib.rs` before writing this step if the exact
paths above don't compile).

- [ ] **Step 3: Commit**

```bash
git add zorp-agent/src/validate/mod.rs
git commit -m "Add validate::run orchestration"
```

---

### Task 7: The `validate` subcommand

**Files:**
- Modify: `zorp-agent/src/main.rs`

**Interfaces:**
- Consumes: `validate::run` (Task 6), the existing `attach_mcp_tools`,
  `Agent::new`, `register_builtins_filtered`, `HttpModel` construction
  pattern already used by the oneshot task path in `main.rs`.
- Produces: a new `Command::Validate { question: String }` variant and
  its handler. No new public interface; this is the CLI entry point.

- [ ] **Step 1: Add the subcommand variant**

Modify `zorp-agent/src/main.rs`'s `Command` enum (the existing one with
`Chat`, `Resume`, `Undo`, `Diff`, `New`), add:

```rust
    /// Validate whether a question is worth investigating.
    #[cfg(feature = "research")]
    Validate { question: String },
```

- [ ] **Step 2: Add the handler**

Find where `main.rs` dispatches on `cli.command` (a `match` over
`Command` variants) and add a `Command::Validate { question }` arm.
Follow the existing oneshot-task construction pattern in `main.rs`
(the block building `HttpModel`, then `Agent::new(...)
.register_builtins_filtered(...) .with_policy(...)`, then
`attach_mcp_tools(agent, overrides, false)`), reusing the same
`Overrides`/`merged` config resolution already in scope at that point
in `main.rs`, rather than duplicating env/flag parsing. After building
`agent`:

```rust
#[cfg(feature = "research")]
{
    let project = match zorp_agent::validate::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };
    let track_id = zorp_track::id::track_id(&question);
    if let Err(e) = project.store.create_track(&track_id, &question) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    }
    let checkpoint_mode = match zorp_track::checkpoint::CheckpointMode::terminal(cli.yes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };
    match zorp_agent::validate::run(&mut agent, &project, &track_id, &question, &checkpoint_mode) {
        Ok(true) => println!("validate: approved, track {track_id} ready for investigate"),
        Ok(false) => println!("validate: rejected, track {track_id} killed"),
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}
```

Adjust import paths (`zorp_track::Project` vs.
`zorp_agent::validate::Project`, whichever is actually exported where)
to match what Tasks 4 through 6 actually exported; the exact `use`
statements at the top of `main.rs` need the new symbols added to the
existing `use zorp_agent::{...}` block, following its existing
alphabetized style. `cli.yes` (the existing `--yes` flag) is reused
directly as the checkpoint's auto-approve flag, the same flag that
already controls tool-call auto-approval; introducing a second,
separate flag for checkpoint auto-approval is unnecessary scope for
this task.

- [ ] **Step 3: Build and manually verify the CLI surface**

Run: `cargo build -p zorp-agent --features research`
Expected: builds clean.

Run: `cargo run -p zorp-agent --features research -- validate --help`
Expected: shows the new subcommand (clap generates this from the doc
comment automatically).

- [ ] **Step 4: Commit**

```bash
git add zorp-agent/src/main.rs
git commit -m "Add the validate subcommand"
```

---

### Task 8: Integration test with a stub MCP server

**Files:**
- Create: `zorp-agent/tests/fixtures/stub_search_mcp_server.rs` (a
  second binary target for this test fixture)
- Modify: `zorp-agent/Cargo.toml` (register the fixture binary)
- Create: `zorp-agent/tests/validate_integration.rs`

**Interfaces:**
- Consumes: `zorp_mcp::{McpConfig, McpRegistry}` (real, already built),
  `validate::run` (Task 6), a stub `Model` (same shape as Task 6's
  `StubModel`, redefined locally since integration tests can't reach a
  unit test's private `#[cfg(test)]` items across the crate boundary).

- [ ] **Step 1: Write a minimal stub MCP server over stdio**

`zorp-agent/tests/fixtures/stub_search_mcp_server.rs`. This is a small,
standalone binary speaking the MCP stdio JSON-RPC protocol just well
enough to answer `initialize`, `tools/list` (advertising one tool,
`search`), and `tools/call` (returning one canned result), reading
requests from stdin and writing responses to stdout, line-delimited
JSON, matching the framing `zorp-mcp/src/transport/stdio.rs` already
expects (check that file's request/response framing before writing
this, so the fixture actually speaks the protocol `zorp-mcp` parses,
rather than assuming a shape).

```rust
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn respond(id: &Value, result: Value) {
    let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    println!("{}", resp);
    io::stdout().flush().ok();
}

fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        match req.get("method").and_then(|m| m.as_str()) {
            Some("initialize") => respond(&id, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": { "name": "stub-search", "version": "0.1.0" }
            })),
            Some("tools/list") => respond(&id, json!({
                "tools": [{
                    "name": "search",
                    "description": "search the web",
                    "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } } }
                }]
            })),
            Some("tools/call") => respond(&id, json!({
                "content": [{ "type": "text", "text": "Stub result: no prior work found on this exact question. A relevant benchmarking tool already exists in the target repo." }]
            })),
            _ => {}
        }
    }
}
```

If `zorp-mcp/src/transport/stdio.rs`'s actual framing differs from
plain line-delimited JSON (e.g. it expects `Content-Length` headers
like LSP), match that framing instead; read the file first, this
sketch assumes the simpler line-delimited shape until confirmed
otherwise.

- [ ] **Step 2: Register the fixture as a binary target**

Modify `zorp-agent/Cargo.toml`, add:

```toml
[[bin]]
name = "stub_search_mcp_server"
path = "tests/fixtures/stub_search_mcp_server.rs"
```

This makes `CARGO_BIN_EXE_stub_search_mcp_server` available to
integration tests, the same mechanism `tests/cli.rs` already uses for
the main binaries.

- [ ] **Step 3: Write the integration test**

`zorp-agent/tests/validate_integration.rs`:

```rust
use serde_json::json;
use std::path::PathBuf;
use tempfile::tempdir;
use zorp_agent::model::{AssistantMessage, Message, Model};
use zorp_agent::validate::{self, ValidateError};
use zorp_agent::{cancel_token, Agent, ApprovalMode, BoxErr};
use zorp_mcp::{McpConfig, McpRegistry};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::Project;

struct StubModel {
    response: String,
}

impl Model for StubModel {
    fn complete(&self, _messages: &[Message], _tools: &[serde_json::Value]) -> Result<AssistantMessage, BoxErr> {
        Ok(AssistantMessage {
            content: self.response.clone(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }
}

fn well_formed_response() -> String {
    "Based on the search: \n```json\n{\"redundancy_score\": 15.0, \"redundancy_citations\": [{\"text\": \"no prior work found on this exact question\", \"source\": \"stub search\"}], \"feasibility_score\": 88.0, \"feasibility_citations\": [{\"text\": \"a relevant benchmarking tool already exists\", \"source\": \"stub search\"}], \"verdict\": \"worth investigating\"}\n```".to_string()
}

fn stub_server_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stub_search_mcp_server"))
}

#[test]
fn validate_end_to_end_with_a_stub_search_server_and_stub_model() {
    let dir = tempdir().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init", "-q"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["config", "user.email", "t@example.com"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["config", "user.name", "T"]).output().unwrap();

    let config = McpConfig::from_toml_str(&format!(
        "[[server]]\nname = \"stub\"\ntransport = \"stdio\"\ncommand = \"{}\"\ntrust = \"sandbox\"\n",
        stub_server_binary().display()
    ))
    .unwrap();
    let mut registry = McpRegistry::new(config);
    let tools = registry.discover();
    assert!(!tools.is_empty(), "stub server should advertise at least one tool");

    let model = StubModel { response: well_formed_response() };
    let mut agent = Agent::new(Box::new(model), "system", 5, dir.path().to_path_buf(), cancel_token(), ApprovalMode::AutoApprove)
        .register_builtins();
    // Attach the stub MCP tool the same way attach_mcp_tools does, adapted
    // inline here since attach_mcp_tools itself lives in the binary crate,
    // not the library, and isn't reachable from an integration test.
    use std::sync::{Arc, Mutex};
    let registry = Arc::new(Mutex::new(registry));
    for tool in tools {
        agent = agent.register(Box::new(zorp_agent::mcp_adapter::McpToolAdapter { tool, registry: registry.clone() }));
    }

    let project = Project::open(dir.path()).unwrap();
    let track_id = "2026-08-09-validate-integration-test";
    project.store.create_track(track_id, "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = validate::run(&mut agent, &project, track_id, "does caching help", &mode).unwrap();
    assert!(approved);

    let validation = project.store.get_validation(track_id).unwrap();
    assert_eq!(validation.redundancy_score, 15.0);
    assert_eq!(validation.feasibility_score, 88.0);
    assert_eq!(validation.redundancy_citations.len(), 1);

    let track = project.store.get_track(track_id).unwrap();
    assert_eq!(track.status, zorp_track::track::TrackStatus::Active);
}
```

This test still calls the real `embed_texts` (Task 3), which needs a
real `ZORP_BASE_URL`/`ZORP_EMBEDDING_MODEL`. If no embeddings provider
is configured in the test environment, this test will fail at the
embedding step, not before. Guard it the same way `zorp-mcp`'s own
`#[ignore]`d tests guard on `has_npx()`: check for
`ZORP_EMBEDDING_MODEL` at the top of the test and skip with a clear
message if it's unset, rather than failing. If embedding truly cannot
run in CI, mark this `#[ignore]` with a reason, matching `zorp-mcp`'s
existing convention, and note that in the task's report; don't silently
weaken the assertions instead.

- [ ] **Step 4: Run the test**

Run: `cargo test -p zorp-agent --features research --test
validate_integration`
Expected: passes if `ZORP_EMBEDDING_MODEL` (and a reachable embeddings
endpoint) is configured in the environment; skips or is `#[ignore]`d
with a clear reason otherwise, per Step 3.

- [ ] **Step 5: Commit**

```bash
git add zorp-agent/tests/fixtures/stub_search_mcp_server.rs zorp-agent/tests/validate_integration.rs zorp-agent/Cargo.toml
git commit -m "Add end to end validate integration test with a stub MCP server"
```

---

### Task 9: Full workspace verification

**Files:** none (verification only).

- [ ] **Step 1: Build everything**

Run: `cargo build --workspace`
Expected: clean.

Run: `cargo build -p zorp-agent --features research`
Expected: clean.

- [ ] **Step 2: Run everything**

Run: `cargo test --workspace`
Expected: all tests pass (the Task 8 integration test may skip per its
own guard, that's expected, not a failure).

- [ ] **Step 3: Commit if Step 1 or 2 needed any fixes**

If everything already passed with no changes needed, there's nothing to
commit for this task; it exists to catch cross-task integration gaps
before calling this plan done, the same role Task 10 played in the
`zorp-track` foundation plan.

---

## Self-Review

**Spec coverage:**
- Where this lives, reusing `Agent`/`attach_mcp_tools` instead of new
  MCP code: Task 6, Task 7.
- Search via the agent's own tool loop, `tool_names()` gate: Task 6.
- Embeddings (`ZORP_EMBEDDING_MODEL`, `zorp::join_url`/`zorp_raw`):
  Task 3.
- `Library::insert_source`, lazy table creation: Task 2.
- Scoring, two dimensions, required citations, fenced JSON block:
  Task 4.
- Storage (`validations` table, `Store::record_validation`): Task 1.
- Checkpoint, kill on reject: Task 6.
- Error handling (no search tool, non-`Complete` outcome, parse/citation
  failures): Task 5, Task 6.
- Testing (unit tests per module, stub-MCP-server integration test):
  Tasks 1 through 8.

**Placeholder scan:** no TBD/TODO markers. Two spots explicitly ask the
implementer to check a real file before finalizing exact shape (the MCP
stdio framing in Task 8, the exact `use` paths in Task 6's test): both
are grounded, bounded instructions to verify against real source, not
vague placeholders, consistent with how the `zorp-track` plan handled
`duckdb`/`lancedb` API verification, checked directly rather than
assumed, just not fully pre-verified here for two narrower spots given
this plan's size.

**Type consistency:** `Citation` is defined once, in `zorp-track`
(Task 1), and reused by name everywhere else (Task 4's
`ValidationResult`, Task 6's `run`), never redefined. `ValidateError`
(Task 5) is the single error type `run` (Task 6) returns.

## Execution Options

Plan complete and saved to
`docs/superpowers/plans/2026-08-09-zorp-validate.md`. Two execution
options:

**1. Subagent-Driven (recommended)**, I dispatch a fresh subagent per
task, review between tasks, fast iteration.

**2. Inline Execution**, execute tasks in this session using
executing-plans, batch execution with checkpoints.

Which approach?
