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
//! A background worker sweeps once at startup and periodically after that.
//! A finished turn queues its own session on the same worker, so an active
//! conversation does not wait for the next sweep. Neither path waits in a
//! turn or in server startup. The existing fingerprint is the change check:
//! an unchanged sweep reads the store and issues no embedding calls.
//!
//! # Where
//!
//! Beside the conversations it indexes, in zorp's state directory, in a
//! separate file. Separate because the session store is the user's real
//! history and this code has no business writing to it: a derived index is
//! rebuildable and the thing it was derived from is not.

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use zorp_recall::{
    Chunk, Conversation, EmbedError, Embedder, Index, IndexError, LoopbackUrl, OllamaEmbedder,
};

/// Below this a message is a "yes" or an "ok" and its vector is noise.
const MIN_CHUNK_CHARS: usize = 24;

/// Embedding models have a context limit and a long message is about
/// several things anyway. The head of a message is the part that says what
/// it is about.
const MAX_CHUNK_CHARS: usize = 2000;

/// How many conversations a search answers with unless asked otherwise.
pub const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 50;

/// How often a running server checks the whole store by default.
pub const DEFAULT_SWEEP_SECS: u64 = 300;
pub const SWEEP_SECS_VAR: &str = "ZORP_RECALL_SWEEP_SECS";

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
    /// Successful model calls made by this pass. This is runtime evidence
    /// that a previously unreachable embedder has recovered.
    embeddings: usize,
}

impl Report {
    fn add(&mut self, other: &Report) {
        self.indexed += other.indexed;
        self.skipped += other.skipped;
        self.removed += other.removed;
        self.chunks += other.chunks;
        self.embeddings += other.embeddings;
    }
}

/// What the page needs to decide whether to show a search box.
pub struct Status {
    pub available: bool,
    pub reason: Option<String>,
    pub endpoint: String,
    pub model: String,
    /// Conversations represented in the derived index.
    pub conversations: i64,
    /// Conversations in the source store.
    pub store_conversations: i64,
    pub chunks: i64,
    pub running: bool,
    pub ready: bool,
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

pub fn status(indexer: Option<&IndexerHandle>) -> Status {
    // Configuration and local counts, not a network probe. The worker's
    // last real attempt is what says whether Ollama answered. Refreshing
    // this endpoint therefore never adds an embedding call of its own.
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
    let store = zorp_agent::Store::open_default()
        .map_err(|e| RecallError::Store(e.to_string()))
        .and_then(|store| {
            store
                .sessions()
                .map(|sessions| sessions.len() as i64)
                .map_err(|e| RecallError::Store(e.to_string()))
        });
    let stats = Index::open_at(&index_path()).and_then(|i| i.stats());
    let (indexed_conversations, chunks) = match &stats {
        Ok(s) => (s.conversations, s.chunks),
        Err(_) => (0, 0),
    };
    let store_conversations = store.as_ref().copied().unwrap_or(0);
    let runtime = indexer
        .map(IndexerHandle::snapshot)
        .unwrap_or_else(|| IndexerSnapshot {
            available: false,
            reason: Some("automatic recall indexing is not running".to_string()),
            running: false,
            ready: false,
        });
    let reason = unavailable
        .or_else(|| store.err().map(|e| e.to_string()))
        .or_else(|| stats.err().map(|e| e.to_string()))
        .or_else(|| runtime.reason.clone());
    let running = runtime.running;
    let caught_up = store_conversations == indexed_conversations;
    let ready = reason.is_none() && caught_up && runtime.ready && !runtime.running;
    Status {
        available: reason.is_none(),
        reason,
        endpoint,
        model,
        conversations: indexed_conversations,
        store_conversations,
        chunks,
        running,
        ready,
    }
}

/// Bring the index up to date with the store. Blocking; call it off the
/// async runtime.
pub fn reindex() -> Result<Report, RecallError> {
    let Ok(_guard) = REINDEXING.try_lock() else {
        return Err(RecallError::Busy);
    };
    reindex_unlocked()
}

fn reindex_waiting() -> Result<Report, RecallError> {
    let _guard = REINDEXING.lock().unwrap();
    reindex_unlocked()
}

fn reindex_unlocked() -> Result<Report, RecallError> {
    let embedder = embedder()?;
    reindex_paths(&zorp_agent::Store::default_path(), &index_path(), &embedder)
}

fn reindex_paths(
    store_path: &Path,
    index_path: &Path,
    embedder: &dyn Embedder,
) -> Result<Report, RecallError> {
    let mut index = Index::open_at(index_path)?;
    // A different model means every vector in there is meaningless, so this
    // may empty the index before filling it again. That is the only honest
    // answer, and it is why the model name is recorded.
    index.prepare(&embedder.identity())?;

    let store =
        zorp_agent::Store::open_at(store_path).map_err(|e| RecallError::Store(e.to_string()))?;
    let sessions = store
        .sessions()
        .map_err(|e| RecallError::Store(e.to_string()))?;

    let mut report = Report::default();
    let mut seen: Vec<String> = Vec::with_capacity(sessions.len());
    let mut unreadable = None;
    for session in &sessions {
        seen.push(session.id.clone());
        match index_one(&store, &mut index, embedder, session) {
            Ok(one) => report.add(&one),
            // One unreadable conversation does not stop the rest. Skipping
            // it silently would make a partial index look ready, so retain
            // the first failure and return it after the remaining work.
            Err(RecallError::Store(e)) => {
                unreadable
                    .get_or_insert_with(|| format!("skipping {} in the index: {e}", session.id));
            }
            Err(e) => return Err(e),
        }
    }
    report.removed = index.retain(&seen)?;
    match unreadable {
        Some(error) => Err(RecallError::Store(error)),
        None => Ok(report),
    }
}

/// Bring one conversation up to date, and nothing else.
///
/// This is what a finished turn calls, which is what makes every
/// conversation feed the memory without anybody pressing a button. It is
/// the same work `reindex` does per session and deliberately not a second
/// implementation of it: one chunker, one fingerprint, one place that
/// decides what a stored message is worth embedding.
///
/// It does not call `retain`. Indexing the session that just finished is
/// not the moment to decide what the store no longer has, and reading every
/// session header to find out would make a per turn call cost what a full
/// reindex costs.
///
/// Blocking; call it off the async runtime.
pub fn feed_session(session_id: &str) -> Result<Report, RecallError> {
    let Ok(_guard) = REINDEXING.try_lock() else {
        return Err(RecallError::Busy);
    };
    feed_session_unlocked(session_id)
}

fn feed_session_waiting(session_id: &str) -> Result<Report, RecallError> {
    let _guard = REINDEXING.lock().unwrap();
    feed_session_unlocked(session_id)
}

fn feed_session_unlocked(session_id: &str) -> Result<Report, RecallError> {
    let embedder = embedder()?;
    let mut index = Index::open_at(&index_path())?;
    index.prepare(&embedder.identity())?;

    let store = zorp_agent::Store::open_default().map_err(|e| RecallError::Store(e.to_string()))?;
    let sessions = store
        .sessions()
        .map_err(|e| RecallError::Store(e.to_string()))?;
    let Some(session) = sessions.iter().find(|s| s.id == session_id) else {
        // Not an error. A turn can end before anything about it reached the
        // store, and there is nothing to index in that case.
        return Ok(Report::default());
    };
    index_one(&store, &mut index, &embedder, session)
}

/// Embed and write one conversation, or skip it if its text has not moved.
fn index_one(
    store: &zorp_agent::Store,
    index: &mut Index,
    embedder: &dyn Embedder,
    session: &zorp_agent::SessionRow,
) -> Result<Report, RecallError> {
    let messages = store
        .load_messages(&session.id)
        .map_err(|e| RecallError::Store(e.to_string()))?;
    let chunks = chunks_for(&messages);
    let print = fingerprint(&session.task, &chunks);
    if index.fingerprint(&session.id)?.as_deref() == Some(print.as_str()) {
        return Ok(Report {
            skipped: 1,
            chunks: chunks.len(),
            ..Report::default()
        });
    }
    // A conversation is rewritten whole, because that is the only way an
    // edited message stops being in the index twice. Whole does not have to
    // mean re-embedded, though: a message whose text has not changed has a
    // vector that has not changed either, and the old one is right there.
    //
    // That distinction is the difference between a feed that costs one
    // model call per new message and one that costs the length of the
    // conversation every turn. This runs after every turn now, so the
    // second shape would make a long thread quadratically expensive to
    // keep in the memory.
    let known = index.vectors_by_text(&session.id)?;
    let mut embedded = Vec::with_capacity(chunks.len());
    let mut embeddings = 0;
    for chunk in chunks {
        let vector = match known.get(&chunk.text) {
            Some(cached) => cached.clone(),
            None => {
                let vector = embedder.embed(&chunk.text)?;
                embeddings += 1;
                vector
            }
        };
        embedded.push((chunk, vector));
    }
    index.replace(
        Conversation {
            id: session.id.clone(),
            title: session.task.clone(),
            updated: session.updated,
            fingerprint: print,
        },
        &embedder.identity(),
        &embedded,
    )?;
    Ok(Report {
        indexed: 1,
        chunks: embedded.len(),
        embeddings,
        ..Report::default()
    })
}

trait PassRunner: Send + Sync + 'static {
    fn sweep(&self) -> Result<Report, RecallError>;
    fn session(&self, session_id: &str) -> Result<Report, RecallError>;
}

struct StorePasses;

impl PassRunner for StorePasses {
    fn sweep(&self) -> Result<Report, RecallError> {
        reindex_waiting()
    }

    fn session(&self, session_id: &str) -> Result<Report, RecallError> {
        feed_session_waiting(session_id)
    }
}

#[derive(Default)]
struct RuntimeState {
    running: bool,
    pending: usize,
    swept: bool,
    last_failure: Option<String>,
    last_failure_kind: Option<FailureKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Embedder,
    Other,
}

/// The part of the background worker's state that the status route exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexerSnapshot {
    pub available: bool,
    pub reason: Option<String>,
    pub running: bool,
    pub ready: bool,
}

enum Command {
    Session(String),
    Sweep(mpsc::Sender<Result<Report, RecallError>>),
}

type Logger = Arc<dyn Fn(&str) + Send + Sync>;

/// A non-blocking handle to the one thread allowed to update the index.
///
/// Session updates only send a small message. A forced sweep waits for its
/// answer, which is why the HTTP route calls it through `spawn_blocking`.
#[derive(Clone)]
pub struct IndexerHandle {
    tx: mpsc::Sender<Command>,
    state: Arc<Mutex<RuntimeState>>,
    pending_sessions: Arc<Mutex<HashSet<String>>>,
}

impl IndexerHandle {
    /// Start the real worker. This returns before its startup sweep begins.
    pub fn start_from_env() -> Self {
        let interval = sweep_interval_from_env();
        match Self::try_start_with(
            interval,
            Arc::new(StorePasses),
            Arc::new(|line| eprintln!("{line}")),
        ) {
            Ok(indexer) => indexer,
            Err(error) => {
                let reason = format!("cannot start the background indexer: {error}");
                eprintln!("zorp-web: recall indexing paused: {reason}");
                Self::stopped(reason)
            }
        }
    }

    #[cfg(test)]
    fn start_with(interval: Option<Duration>, runner: Arc<dyn PassRunner>, logger: Logger) -> Self {
        Self::try_start_with(interval, runner, logger).expect("the recall worker starts")
    }

    fn try_start_with(
        interval: Option<Duration>,
        runner: Arc<dyn PassRunner>,
        logger: Logger,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let pending_sessions = Arc::new(Mutex::new(HashSet::new()));
        let worker_state = Arc::clone(&state);
        let worker_sessions = Arc::clone(&pending_sessions);
        std::thread::Builder::new()
            .name("zorp-recall-indexer".to_string())
            .spawn(move || {
                worker_loop(rx, worker_state, worker_sessions, runner, interval, logger)
            })?;
        Ok(Self {
            tx,
            state,
            pending_sessions,
        })
    }

    fn stopped(reason: String) -> Self {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let state = RuntimeState {
            last_failure: Some(reason),
            last_failure_kind: Some(FailureKind::Other),
            ..RuntimeState::default()
        };
        Self {
            tx,
            state: Arc::new(Mutex::new(state)),
            pending_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Queue one changed conversation. Sending never waits for embeddings.
    pub fn index_session(&self, session_id: impl Into<String>) {
        let session_id = session_id.into();
        if !self
            .pending_sessions
            .lock()
            .unwrap()
            .insert(session_id.clone())
        {
            return;
        }
        self.state.lock().unwrap().pending += 1;
        if self.tx.send(Command::Session(session_id.clone())).is_err() {
            self.pending_sessions.lock().unwrap().remove(&session_id);
            let mut state = self.state.lock().unwrap();
            state.pending = state.pending.saturating_sub(1);
            state.last_failure = Some("the background indexer stopped".to_string());
            state.last_failure_kind = Some(FailureKind::Other);
        }
    }

    /// Force a full pass and wait for its report.
    pub fn sweep(&self) -> Result<Report, RecallError> {
        let (tx, rx) = mpsc::channel();
        self.state.lock().unwrap().pending += 1;
        if self.tx.send(Command::Sweep(tx)).is_err() {
            let mut state = self.state.lock().unwrap();
            state.pending = state.pending.saturating_sub(1);
            return Err(RecallError::Store(
                "the background indexer stopped".to_string(),
            ));
        }
        rx.recv().unwrap_or_else(|_| {
            Err(RecallError::Store(
                "the background indexer stopped".to_string(),
            ))
        })
    }

    pub fn snapshot(&self) -> IndexerSnapshot {
        let state = self.state.lock().unwrap();
        let available = state.last_failure.is_none();
        IndexerSnapshot {
            available,
            reason: state.last_failure.clone(),
            running: state.running,
            ready: state.swept && available && !state.running && state.pending == 0,
        }
    }
}

fn sweep_interval_from_env() -> Option<Duration> {
    match std::env::var(SWEEP_SECS_VAR) {
        Ok(raw) if !raw.trim().is_empty() => match raw.trim().parse::<u64>() {
            Ok(_) => sweep_interval(Some(raw.trim())),
            Err(_) => {
                eprintln!(
                    "zorp-web: ignoring invalid {SWEEP_SECS_VAR}={raw:?}; using {DEFAULT_SWEEP_SECS}"
                );
                sweep_interval(None)
            }
        },
        _ => sweep_interval(None),
    }
}

fn sweep_interval(raw: Option<&str>) -> Option<Duration> {
    let seconds = raw
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SWEEP_SECS);
    (seconds != 0).then(|| Duration::from_secs(seconds))
}

fn worker_loop(
    rx: mpsc::Receiver<Command>,
    state: Arc<Mutex<RuntimeState>>,
    pending_sessions: Arc<Mutex<HashSet<String>>>,
    runner: Arc<dyn PassRunner>,
    interval: Option<Duration>,
    logger: Logger,
) {
    let mut next_sweep = interval.map(|period| {
        let _ = run_pass(&state, runner.as_ref(), None, false, logger.as_ref());
        Instant::now() + period
    });

    loop {
        let command = match (interval, next_sweep) {
            (Some(period), Some(deadline)) if Instant::now() >= deadline => {
                let _ = run_pass(&state, runner.as_ref(), None, false, logger.as_ref());
                next_sweep = Some(Instant::now() + period);
                continue;
            }
            (Some(_), Some(deadline)) => {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            (None, None) => match rx.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            },
            _ => unreachable!("the sweep interval and deadline move together"),
        };

        match command {
            Some(Command::Session(session_id)) => {
                // Remove it before the pass. A later turn for this session
                // can then queue one follow-up while this snapshot is read.
                pending_sessions.lock().unwrap().remove(&session_id);
                let _ = run_pass(
                    &state,
                    runner.as_ref(),
                    Some(&session_id),
                    true,
                    logger.as_ref(),
                );
            }
            Some(Command::Sweep(reply)) => {
                let result = run_pass(&state, runner.as_ref(), None, true, logger.as_ref());
                let _ = reply.send(result);
            }
            None => {
                let _ = run_pass(&state, runner.as_ref(), None, false, logger.as_ref());
                next_sweep = interval.map(|period| Instant::now() + period);
            }
        }
    }
}

fn run_pass(
    state: &Mutex<RuntimeState>,
    runner: &dyn PassRunner,
    session_id: Option<&str>,
    queued: bool,
    logger: &dyn Fn(&str),
) -> Result<Report, RecallError> {
    state.lock().unwrap().running = true;
    let result = match session_id {
        Some(session_id) => runner.session(session_id),
        None => runner.sweep(),
    };

    let mut log = None;
    {
        let mut state = state.lock().unwrap();
        state.running = false;
        if queued {
            state.pending = state.pending.saturating_sub(1);
        }
        match &result {
            Ok(report) => {
                if session_id.is_none() {
                    state.swept = true;
                }
                let recovered = session_id.is_none()
                    && match state.last_failure_kind {
                        Some(FailureKind::Embedder) => report.embeddings > 0,
                        Some(FailureKind::Other) => true,
                        None => false,
                    };
                if recovered {
                    state.last_failure = None;
                    state.last_failure_kind = None;
                    log = Some("zorp-web: recall indexing recovered".to_string());
                }
            }
            Err(error) => {
                let kind = if matches!(error, RecallError::Embed(_)) {
                    FailureKind::Embedder
                } else {
                    FailureKind::Other
                };
                let error = error.to_string();
                if state.last_failure.is_none() {
                    log = Some(format!("zorp-web: recall indexing paused: {error}"));
                }
                state.last_failure = Some(error);
                state.last_failure_kind = Some(kind);
            }
        }
    }
    if let Some(line) = log {
        logger(&line);
    }
    result
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

/// Search, answering messages rather than conversations.
///
/// What `crate::memory` reads. Same index and same embedder as `search`,
/// different unit: a recall into a live thread wants the lines themselves,
/// with their provenance attached, and it wants two of them from one
/// conversation when that is where the answer is.
///
/// Blocking; call it off the async runtime.
pub fn passages(query: &str, limit: usize) -> Result<Vec<zorp_recall::Passage>, RecallError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(RecallError::EmptyQuery);
    }
    let embedder = embedder()?;
    let index = Index::open_at(&index_path())?;
    let vector = embedder.embed(query)?;
    Ok(index.search_passages(&vector, limit.clamp(1, MAX_LIMIT))?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    struct CountingEmbedder {
        calls: AtomicUsize,
    }

    impl CountingEmbedder {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Embedder for CountingEmbedder {
        fn identity(&self) -> String {
            "test/counting".to_string()
        }

        fn embed(&self, _text: &str) -> Result<Vec<f32>, EmbedError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![1.0, 0.0])
        }
    }

    fn seed(store_path: &Path) {
        let mut store = zorp_agent::Store::open_at(store_path).unwrap();
        store
            .create_session("conv-1", "A verbatim first question", "repo", "model")
            .unwrap();
        store
            .record_message(
                "conv-1",
                0,
                &zorp_agent::Message::user("A long enough first message to be embedded"),
            )
            .unwrap();
        store
            .record_message(
                "conv-1",
                1,
                &zorp_agent::Message::assistant("A long enough answer to be embedded too"),
            )
            .unwrap();
    }

    #[test]
    fn an_unchanged_sweep_issues_no_embedding_calls() {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("sessions.db");
        let index_path = dir.path().join("recall.db");
        seed(&store_path);
        let embedder = CountingEmbedder::new();

        let first = reindex_paths(&store_path, &index_path, &embedder).unwrap();
        assert_eq!(first.indexed, 1);
        let calls_after_first = embedder.calls();

        let second = reindex_paths(&store_path, &index_path, &embedder).unwrap();
        assert_eq!(second.skipped, 1);
        assert_eq!(embedder.calls(), calls_after_first);
    }

    #[test]
    fn a_changed_conversation_is_picked_up_by_the_next_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("sessions.db");
        let index_path = dir.path().join("recall.db");
        seed(&store_path);
        let embedder = CountingEmbedder::new();
        reindex_paths(&store_path, &index_path, &embedder).unwrap();
        let calls_after_first = embedder.calls();

        zorp_agent::Store::open_at(&store_path)
            .unwrap()
            .record_message(
                "conv-1",
                2,
                &zorp_agent::Message::user("A later correction changes this conversation"),
            )
            .unwrap();

        let report = reindex_paths(&store_path, &index_path, &embedder).unwrap();
        assert_eq!(report.indexed, 1);
        assert_eq!(embedder.calls(), calls_after_first + 1);
    }

    struct ConcurrentRunner {
        active: AtomicUsize,
        max_active: AtomicUsize,
        calls: AtomicUsize,
    }

    struct TrafficRunner {
        sweeps: AtomicUsize,
        sessions: AtomicUsize,
    }

    impl PassRunner for TrafficRunner {
        fn sweep(&self) -> Result<Report, RecallError> {
            self.sweeps.fetch_add(1, Ordering::SeqCst);
            Ok(Report::default())
        }

        fn session(&self, _session_id: &str) -> Result<Report, RecallError> {
            self.sessions.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(2));
            Ok(Report::default())
        }
    }

    impl ConcurrentRunner {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                calls: AtomicUsize::new(0),
            }
        }

        fn run(&self) -> Result<Report, RecallError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(Report::default())
        }
    }

    impl PassRunner for ConcurrentRunner {
        fn sweep(&self) -> Result<Report, RecallError> {
            self.run()
        }

        fn session(&self, _session_id: &str) -> Result<Report, RecallError> {
            self.run()
        }
    }

    fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while !predicate() {
            assert!(Instant::now() < deadline, "condition did not become true");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn silent_logger() -> Arc<dyn Fn(&str) + Send + Sync> {
        Arc::new(|_| {})
    }

    #[test]
    fn queued_session_and_sweep_passes_never_run_concurrently() {
        let runner = Arc::new(ConcurrentRunner::new());
        let indexer = IndexerHandle::start_with(None, runner.clone(), silent_logger());

        indexer.index_session("conv-1");
        let forced = {
            let indexer = indexer.clone();
            std::thread::spawn(move || indexer.sweep())
        };
        forced.join().unwrap().unwrap();

        assert_eq!(runner.calls.load(Ordering::SeqCst), 2);
        assert_eq!(runner.max_active.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn duplicate_session_requests_are_coalesced() {
        let runner = Arc::new(TrafficRunner {
            sweeps: AtomicUsize::new(0),
            sessions: AtomicUsize::new(0),
        });
        let indexer = IndexerHandle::start_with(None, runner.clone(), silent_logger());

        for _ in 0..100 {
            indexer.index_session("conv-1");
        }
        indexer.sweep().unwrap();

        assert!(
            runner.sessions.load(Ordering::SeqCst) <= 2,
            "duplicate work accumulated without a bound"
        );
    }

    #[test]
    fn queued_sessions_cannot_postpone_a_periodic_sweep() {
        let runner = Arc::new(TrafficRunner {
            sweeps: AtomicUsize::new(0),
            sessions: AtomicUsize::new(0),
        });
        let indexer = IndexerHandle::start_with(
            Some(Duration::from_millis(20)),
            runner.clone(),
            silent_logger(),
        );
        wait_until(Duration::from_secs(1), || {
            runner.sweeps.load(Ordering::SeqCst) == 1
        });

        for seq in 0..100 {
            indexer.index_session(format!("conv-{seq}"));
        }
        std::thread::sleep(Duration::from_millis(45));

        assert!(
            runner.sweeps.load(Ordering::SeqCst) >= 2,
            "the queued session backlog starved the periodic sweep"
        );
    }

    #[test]
    fn zero_interval_starts_no_automatic_sweep() {
        let runner = Arc::new(ConcurrentRunner::new());
        let _indexer = IndexerHandle::start_with(None, runner.clone(), silent_logger());

        std::thread::sleep(Duration::from_millis(80));

        assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn zero_seconds_disables_the_configured_sweep() {
        assert_eq!(sweep_interval(Some("0")), None);
    }

    #[test]
    fn the_default_sweep_is_five_minutes() {
        assert_eq!(sweep_interval(None), Some(Duration::from_secs(300)));
    }

    #[test]
    fn a_stopped_worker_stays_explicitly_unavailable() {
        let indexer = IndexerHandle::stopped("thread spawn failed".to_string());

        let snapshot = indexer.snapshot();
        assert!(!snapshot.available);
        assert_eq!(snapshot.reason.as_deref(), Some("thread spawn failed"));
        assert!(!snapshot.ready);
        assert!(indexer.sweep().is_err());

        indexer.index_session("conv-1");
        assert!(!indexer.snapshot().available);
    }

    #[test]
    fn a_pass_is_reported_running_until_it_finishes() {
        let runner = Arc::new(ConcurrentRunner::new());
        let indexer = IndexerHandle::start_with(None, runner.clone(), silent_logger());

        indexer.index_session("conv-1");
        wait_until(Duration::from_secs(1), || indexer.snapshot().running);
        assert!(indexer.snapshot().running);
        wait_until(Duration::from_secs(1), || {
            runner.calls.load(Ordering::SeqCst) == 1 && !indexer.snapshot().running
        });
    }

    struct FailingRunner {
        fail: AtomicBool,
        calls: AtomicUsize,
        successful_embeddings: AtomicUsize,
    }

    impl FailingRunner {
        fn result(&self) -> Result<Report, RecallError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail.load(Ordering::SeqCst) {
                Err(RecallError::Embed(EmbedError::Unreachable {
                    url: "http://127.0.0.1:11434".to_string(),
                    message: format!("connection refused on attempt {call}"),
                }))
            } else {
                Ok(Report {
                    embeddings: self.successful_embeddings.load(Ordering::SeqCst),
                    ..Report::default()
                })
            }
        }
    }

    impl PassRunner for FailingRunner {
        fn sweep(&self) -> Result<Report, RecallError> {
            self.result()
        }

        fn session(&self, _session_id: &str) -> Result<Report, RecallError> {
            self.result()
        }
    }

    #[test]
    fn a_session_success_cannot_hide_a_failed_full_sweep() {
        let runner = Arc::new(FailingRunner {
            fail: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
            successful_embeddings: AtomicUsize::new(1),
        });
        let indexer = IndexerHandle::start_with(None, runner.clone(), silent_logger());

        assert!(indexer.sweep().is_err());
        runner.fail.store(false, Ordering::SeqCst);
        indexer.index_session("conv-1");
        wait_until(Duration::from_secs(1), || {
            runner.calls.load(Ordering::SeqCst) == 2 && !indexer.snapshot().running
        });
        assert!(
            !indexer.snapshot().available,
            "unrelated session work hid the failed full sweep"
        );

        indexer.sweep().unwrap();
        assert!(indexer.snapshot().available);
    }

    #[test]
    fn an_unreachable_embedder_is_retried_without_logging_every_tick() {
        let runner = Arc::new(FailingRunner {
            fail: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
            successful_embeddings: AtomicUsize::new(0),
        });
        let logs = Arc::new(Mutex::new(Vec::<String>::new()));
        let logger = {
            let logs = Arc::clone(&logs);
            Arc::new(move |line: &str| logs.lock().unwrap().push(line.to_string()))
                as Arc<dyn Fn(&str) + Send + Sync>
        };
        let indexer =
            IndexerHandle::start_with(Some(Duration::from_millis(15)), runner.clone(), logger);

        wait_until(Duration::from_secs(1), || {
            runner.calls.load(Ordering::SeqCst) >= 3
        });
        assert_eq!(logs.lock().unwrap().len(), 1);
        assert!(!indexer.snapshot().available);

        runner.fail.store(false, Ordering::SeqCst);
        indexer.sweep().unwrap();
        assert!(
            !indexer.snapshot().available,
            "a no-op pass cannot prove that the embedder recovered"
        );
        assert_eq!(logs.lock().unwrap().len(), 1);

        runner.successful_embeddings.store(1, Ordering::SeqCst);
        indexer.sweep().unwrap();
        assert!(indexer.snapshot().available);
        assert_eq!(
            logs.lock().unwrap().len(),
            2,
            "recovery was not logged once"
        );

        runner.fail.store(true, Ordering::SeqCst);
        let _ = indexer.sweep();
        assert_eq!(
            logs.lock().unwrap().len(),
            3,
            "the new failure was not logged"
        );
    }
}
