//! Semantic search over the conversations already in the store.
//!
//! `zorp-recall` holds the parts that could be reused: the loopback guard,
//! the embedder, the vector index. This module is the part that could not
//! be, because it is the only place that knows a conversation is a row in
//! `zorp-agent`'s SQLite store and a message is a `Message`.
//!
//! # What gets embedded
//!
//! One vector per message, for the `user` and `assistant` messages that
//! carry text. The same set the transcript replays, which is not a
//! coincidence: a result you cannot open and read is not a result.
//!
//! Tool results are left out. They are the biggest thing in most sessions
//! and the least like something a person would search for, being mostly
//! file contents the agent read on its way to an answer. Indexing them
//! would multiply the cost of a reindex by a large number and fill the
//! results with the same file appearing in nine conversations.
//!
//! A message rather than a whole conversation, because a whole conversation
//! averaged into one vector is a vector about nothing in particular, and
//! because a per-message hit gives the result list a line to show. Results
//! are rolled back up to one row per conversation by the index.
//!
//! # When
//!
//! On request, and never on its own. Embedding on every write would put a
//! model call in the path of sending a message and make the chat depend on
//! Ollama being up. Embedding on the first search would put a several
//! minute wait behind a text box. Neither is worth it for a corpus that
//! changes a few times a day, so it is a button, and the button is
//! incremental: a conversation whose text has not changed is skipped by
//! fingerprint.
//!
//! # Where
//!
//! Beside the conversations it indexes, in zorp's state directory, in a
//! separate file. Separate because the session store is the user's real
//! history and this code has no business writing to it: a derived index is
//! rebuildable and the thing it was derived from is not.

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use zorp_recall::{Chunk, EmbedError, Embedder, Index, IndexError, LoopbackUrl, OllamaEmbedder};

/// Below this a message is a "yes" or an "ok" and its vector is noise.
const MIN_CHUNK_CHARS: usize = 24;

/// Embedding models have a context limit and a long message is about
/// several things anyway. The head of a message is the part that says what
/// it is about.
const MAX_CHUNK_CHARS: usize = 2000;

/// How many conversations a search answers with unless asked otherwise.
pub const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 50;

/// Two reindexes at once would fight over one SQLite file and ask the model
/// for the same vectors twice. The second one is told to wait, the same way
/// a second turn on a busy session is.
static REINDEXING: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum RecallError {
    /// No vector could be produced. Includes the case where the configured
    /// endpoint is not on this machine.
    Embed(EmbedError),
    /// The index could not be read or written.
    Index(IndexError),
    /// The conversation store could not be opened.
    Store(String),
    /// A reindex is already running.
    Busy,
    /// The request did not ask for anything.
    EmptyQuery,
}

impl std::fmt::Display for RecallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecallError::Embed(e) => write!(f, "{e}"),
            RecallError::Index(e) => write!(f, "{e}"),
            RecallError::Store(e) => write!(f, "cannot read the conversation store: {e}"),
            RecallError::Busy => write!(f, "an index is already running"),
            RecallError::EmptyQuery => write!(f, "a search needs something to search for"),
        }
    }
}

impl From<EmbedError> for RecallError {
    fn from(e: EmbedError) -> RecallError {
        RecallError::Embed(e)
    }
}

impl From<IndexError> for RecallError {
    fn from(e: IndexError) -> RecallError {
        RecallError::Index(e)
    }
}

/// What one reindex did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
    pub chunks: usize,
}

/// What the page needs to decide whether to show a search box.
pub struct Status {
    pub available: bool,
    pub reason: Option<String>,
    pub endpoint: String,
    pub model: String,
    pub conversations: i64,
    pub chunks: i64,
}

/// Where the index lives.
///
/// `ZORP_RECALL_DB` names it outright, the same escape hatch
/// `ZORP_STATE_DB` gives the session store, and the tests use it. Otherwise
/// it sits next to the sessions database, wherever that resolved to, so
/// moving zorp's state directory moves both.
///
/// Nothing gitignores it, because nothing needs to: it is in the state
/// directory, not in a repository. `.zorp/`'s own `.gitignore` covers
/// `zorp.duckdb*` and `lancedb/` because those are written inside a project
/// tree. This is not.
pub fn index_path() -> PathBuf {
    if let Ok(p) = std::env::var("ZORP_RECALL_DB") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let sessions = zorp_agent::Store::default_path();
    match sessions.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("recall.db"),
        _ => PathBuf::from("recall.db"),
    }
}

fn embedder() -> Result<OllamaEmbedder, EmbedError> {
    OllamaEmbedder::from_env()
}

pub fn status() -> Status {
    // Configuration and index size, not a probe. Whether the model is
    // running right now is answered by trying, and trying is what the
    // index button and the search box do. A status endpoint that opened a
    // socket every time the page loaded would be a background poll of
    // somebody's model server.
    let (endpoint, model, unavailable) = match embedder() {
        Ok(e) => (e.endpoint().to_string(), e.model().to_string(), None),
        Err(e) => (
            std::env::var(zorp_recall::EMBED_URL_VAR)
                .unwrap_or_else(|_| zorp_recall::DEFAULT_EMBED_URL.to_string()),
            std::env::var(zorp_recall::EMBED_MODEL_VAR)
                .unwrap_or_else(|_| zorp_recall::DEFAULT_EMBED_MODEL.to_string()),
            Some(e.to_string()),
        ),
    };
    let stats = Index::open_at(&index_path()).and_then(|i| i.stats());
    let (conversations, chunks) = match &stats {
        Ok(s) => (s.conversations, s.chunks),
        Err(_) => (0, 0),
    };
    let reason = unavailable.or_else(|| stats.err().map(|e| e.to_string()));
    Status {
        available: reason.is_none(),
        reason,
        endpoint,
        model,
        conversations,
        chunks,
    }
}

/// Bring the index up to date with the store. Blocking; call it off the
/// async runtime.
pub fn reindex() -> Result<Report, RecallError> {
    let Ok(_guard) = REINDEXING.try_lock() else {
        return Err(RecallError::Busy);
    };
    let embedder = embedder()?;
    let mut index = Index::open_at(&index_path())?;
    // A different model means every vector in there is meaningless, so this
    // may empty the index before filling it again. That is the only honest
    // answer, and it is why the model name is recorded.
    index.prepare(&embedder.identity())?;

    let store = zorp_agent::Store::open_default().map_err(|e| RecallError::Store(e.to_string()))?;
    let sessions = store
        .sessions()
        .map_err(|e| RecallError::Store(e.to_string()))?;

    let mut report = Report::default();
    let mut seen: Vec<String> = Vec::with_capacity(sessions.len());
    for session in &sessions {
        seen.push(session.id.clone());
        let messages = match store.load_messages(&session.id) {
            Ok(m) => m,
            // One unreadable conversation does not stop the rest. Skipping
            // it silently would be worse than the warning, but failing the
            // whole reindex over it would be worse than both.
            Err(e) => {
                eprintln!("zorp-web: skipping {} in the index: {e}", session.id);
                continue;
            }
        };
        let chunks = chunks_for(&messages);
        let print = fingerprint(&session.task, &chunks);
        if index.fingerprint(&session.id)?.as_deref() == Some(print.as_str()) {
            report.skipped += 1;
            report.chunks += chunks.len();
            continue;
        }
        let mut embedded = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let vector = embedder.embed(&chunk.text)?;
            embedded.push((chunk, vector));
        }
        index.replace(
            &session.id,
            &session.task,
            &print,
            &embedder.identity(),
            &embedded,
        )?;
        report.indexed += 1;
        report.chunks += embedded.len();
    }
    report.removed = index.retain(&seen)?;
    Ok(report)
}

/// Search. Blocking; call it off the async runtime.
pub fn search(query: &str, limit: usize) -> Result<Vec<zorp_recall::Hit>, RecallError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(RecallError::EmptyQuery);
    }
    let embedder = embedder()?;
    let index = Index::open_at(&index_path())?;
    let vector = embedder.embed(query)?;
    Ok(index.search(&vector, limit.clamp(1, MAX_LIMIT))?)
}

/// The messages worth embedding, in order.
fn chunks_for(messages: &[zorp_agent::Message]) -> Vec<Chunk> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user" || m.role == "assistant")
        .filter_map(|(seq, m)| {
            let text = m.text();
            let text = text.trim();
            if text.chars().count() < MIN_CHUNK_CHARS {
                return None;
            }
            Some(Chunk {
                seq: seq as i64,
                role: m.role.clone(),
                text: text.chars().take(MAX_CHUNK_CHARS).collect(),
            })
        })
        .collect()
}

/// What this conversation looked like, so the next reindex can tell whether
/// it moved. Over the title and the exact text of every chunk, so a message
/// edited in place counts as a change and a conversation that only grew
/// still gets re-embedded whole. Re-embedding a grown conversation whole is
/// wasteful and it is also correct, and correct is the one that matters at
/// this size.
fn fingerprint(title: &str, chunks: &[Chunk]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    for chunk in chunks {
        hasher.update([0u8]);
        hasher.update(chunk.seq.to_le_bytes());
        hasher.update(chunk.role.as_bytes());
        hasher.update(chunk.text.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// The configured endpoint, checked, for anything that wants to report it
/// without building an embedder.
pub fn configured_endpoint() -> Result<LoopbackUrl, zorp_recall::LoopbackError> {
    let raw = std::env::var(zorp_recall::EMBED_URL_VAR)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| zorp_recall::DEFAULT_EMBED_URL.to_string());
    LoopbackUrl::parse(&raw)
}
