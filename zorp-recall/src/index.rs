//! Where the vectors live, and how a query finds them.
//!
//! A SQLite file, and a brute-force scan.
//!
//! Both halves of that are deliberate. `zorp-track` already has a LanceDB
//! vector library, and reusing it would have been the reflex, but it is
//! opt-in precisely because it pulls the whole Arrow tree in, it is keyed
//! by track id because it holds an investigation's evidence, and chat
//! history is not evidence. SQLite is already in this workspace, already
//! linked by the crate that holds the conversations, and adds nothing.
//!
//! The scan is brute force because the corpus is a person's chat history.
//! A few hundred conversations is a few thousand vectors, which is a few
//! megabytes, and a dot product over that finishes in well under a
//! millisecond. An approximate index would be a data structure, a build
//! step, and a recall/latency tradeoff bought for a problem nobody has. If
//! this ever holds a hundred thousand conversations it will want one, and
//! swapping it in touches this file and nothing else.
//!
//! Vectors are stored normalized, so cosine similarity is a dot product and
//! nothing has to remember to divide.

use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;
use std::fmt;
use std::path::Path;

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conversations (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    fingerprint TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chunks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL,
    text            TEXT NOT NULL,
    vector          BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chunks_conversation ON chunks(conversation_id);";

const EMBEDDER_KEY: &str = "embedder";
const DIMENSIONS_KEY: &str = "dimensions";

/// One indexable piece of a conversation. A message, in practice: it is the
/// smallest unit that is still a thing a person remembers saying, and it
/// gives a result something to show besides a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Position in the conversation, so a hit can be pointed at.
    pub seq: i64,
    /// `user` or `assistant`.
    pub role: String,
    /// The text that was embedded.
    pub text: String,
}

/// One conversation that matched, and the message in it that matched best.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub conversation_id: String,
    pub title: String,
    pub seq: i64,
    pub role: String,
    pub snippet: String,
    /// Cosine similarity, in -1..=1. Comparable within one search and
    /// meaningless across two different models.
    pub score: f32,
}

/// What the index currently holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub conversations: i64,
    pub chunks: i64,
    /// The model every vector in here came from, if there is one.
    pub embedder: Option<String>,
    pub dimensions: Option<usize>,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum IndexError {
    Sqlite(rusqlite::Error),
    /// A write named a different model than the index was prepared with.
    EmbedderMismatch {
        index: Option<String>,
        given: String,
    },
    /// A query vector of a different width than the stored ones.
    Dimensions {
        stored: usize,
        query: usize,
    },
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Sqlite(e) => write!(f, "conversation index: {e}"),
            IndexError::EmbedderMismatch { index, given } => match index {
                Some(had) => write!(
                    f,
                    "the index was built with {had} and this write says {given}; \
                     call prepare first"
                ),
                None => write!(f, "the index has not been prepared for {given}"),
            },
            IndexError::Dimensions { stored, query } => write!(
                f,
                "the index holds {stored}-dimensional vectors and the query is {query}-dimensional; \
                 reindex to rebuild it"
            ),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> IndexError {
        IndexError::Sqlite(e)
    }
}

/// The conversation index.
pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open_at(path: &Path) -> Result<Index, IndexError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    IndexError::Sqlite(rusqlite::Error::InvalidPath(
                        parent.join(e.to_string()).to_path_buf(),
                    ))
                })?;
            }
        }
        Index::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Index, IndexError> {
        Index::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Index, IndexError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Index { conn })
    }

    /// Declare which model is about to write, and clear the index if it is
    /// not the one already in there. Returns whether anything was cleared.
    ///
    /// Clearing is the only correct answer. Two models put the same idea in
    /// different places, so a query embedded by one and scored against
    /// vectors from the other returns a confident ranking of nothing. There
    /// is no way to convert between them and no way to tell from a row that
    /// it is stale, which leaves throwing it away.
    pub fn prepare(&mut self, embedder: &str) -> Result<bool, IndexError> {
        let current = self.meta(EMBEDDER_KEY)?;
        if current.as_deref() == Some(embedder) {
            return Ok(false);
        }
        let cleared = current.is_some();
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM chunks", [])?;
        tx.execute("DELETE FROM conversations", [])?;
        tx.execute("DELETE FROM meta WHERE key = ?1", [DIMENSIONS_KEY])?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [EMBEDDER_KEY, embedder],
        )?;
        tx.commit()?;
        Ok(cleared)
    }

    /// What this conversation looked like the last time it was indexed, or
    /// `None` if it never has been. A reindex compares this against the
    /// store and skips what has not moved.
    pub fn fingerprint(&self, id: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT fingerprint FROM conversations WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Write one conversation, replacing whatever was there.
    ///
    /// Replacing rather than appending, in one transaction, because a
    /// conversation that grew by three messages must not end up in the
    /// index twice. A conversation with no indexable text is still
    /// recorded, so the next reindex skips it instead of asking the model
    /// about nothing again.
    pub fn replace(
        &mut self,
        id: &str,
        title: &str,
        fingerprint: &str,
        embedder: &str,
        chunks: &[(Chunk, Vec<f32>)],
    ) -> Result<(), IndexError> {
        let current = self.meta(EMBEDDER_KEY)?;
        if current.as_deref() != Some(embedder) {
            return Err(IndexError::EmbedderMismatch {
                index: current,
                given: embedder.to_string(),
            });
        }
        let stored_dim = self.dimensions()?;
        if let Some((_, first)) = chunks.first() {
            if let Some(stored) = stored_dim {
                if stored != first.len() {
                    return Err(IndexError::Dimensions {
                        stored,
                        query: first.len(),
                    });
                }
            }
        }

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM chunks WHERE conversation_id = ?1", [id])?;
        tx.execute(
            "INSERT INTO conversations (id, title, fingerprint) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, \
             fingerprint = excluded.fingerprint",
            [id, title, fingerprint],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO chunks (conversation_id, seq, role, text, vector) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (chunk, vector) in chunks {
                insert.execute(rusqlite::params![
                    id,
                    chunk.seq,
                    chunk.role,
                    chunk.text,
                    to_blob(&normalized(vector)),
                ])?;
            }
        }
        if stored_dim.is_none() {
            if let Some((_, first)) = chunks.first() {
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [DIMENSIONS_KEY, &first.len().to_string()],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drop everything except the conversations named. Returns how many
    /// went. A conversation deleted from the store must stop being a search
    /// result, or search starts offering things that cannot be opened.
    pub fn retain(&mut self, keep: &[String]) -> Result<usize, IndexError> {
        let keep: HashSet<&str> = keep.iter().map(String::as_str).collect();
        let mut stmt = self.conn.prepare("SELECT id FROM conversations")?;
        let existing: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let gone: Vec<&String> = existing
            .iter()
            .filter(|id| !keep.contains(id.as_str()))
            .collect();
        if gone.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        for id in &gone {
            tx.execute("DELETE FROM chunks WHERE conversation_id = ?1", [id])?;
            tx.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(gone.len())
    }

    /// The best `limit` conversations for this query vector, best first.
    ///
    /// One row per conversation, carrying its best-scoring message. Scores
    /// are not filtered here: what counts as too weak to show is a question
    /// about a user interface, and this is not one.
    pub fn search(&self, query: &[f32], limit: usize) -> Result<Vec<Hit>, IndexError> {
        let Some(stored) = self.dimensions()? else {
            return Ok(Vec::new());
        };
        if stored != query.len() {
            return Err(IndexError::Dimensions {
                stored,
                query: query.len(),
            });
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query = normalized(query);

        let mut stmt = self.conn.prepare(
            "SELECT c.conversation_id, v.title, c.seq, c.role, c.text, c.vector \
             FROM chunks c JOIN conversations v ON v.id = c.conversation_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Vec<u8>>(5)?,
            ))
        })?;

        // Best chunk per conversation, ties going to the earlier message.
        // An arbitrary tie-break would make the same search show a
        // different snippet on a reload.
        let mut best: Vec<Hit> = Vec::new();
        for row in rows {
            let (conversation_id, title, seq, role, text, blob) = row?;
            let score = dot(&query, &from_blob(&blob));
            match best
                .iter_mut()
                .find(|h| h.conversation_id == conversation_id)
            {
                Some(existing) => {
                    if score > existing.score || (score == existing.score && seq < existing.seq) {
                        existing.seq = seq;
                        existing.role = role;
                        existing.snippet = text;
                        existing.score = score;
                    }
                }
                None => best.push(Hit {
                    conversation_id,
                    title,
                    seq,
                    role,
                    snippet: text,
                    score,
                }),
            }
        }
        best.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.conversation_id.cmp(&b.conversation_id))
        });
        best.truncate(limit);
        Ok(best)
    }

    pub fn stats(&self) -> Result<Stats, IndexError> {
        Ok(Stats {
            conversations: self
                .conn
                .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?,
            chunks: self
                .conn
                .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?,
            embedder: self.meta(EMBEDDER_KEY)?,
            dimensions: self.dimensions()?,
        })
    }

    fn meta(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    fn dimensions(&self) -> Result<Option<usize>, IndexError> {
        Ok(self
            .meta(DIMENSIONS_KEY)?
            .and_then(|v| v.parse::<usize>().ok()))
    }
}

/// Unit length, so similarity is a dot product. A vector with no length has
/// no direction either; it is left as it is and scores zero against
/// everything, which is the honest answer for text the model had nothing to
/// say about.
fn normalized(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn from_blob(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
