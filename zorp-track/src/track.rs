use crate::schema::SCHEMA;
use crate::TrackError;
use duckdb::Connection;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
}
