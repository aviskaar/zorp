use crate::schema::SCHEMA;
use crate::TrackError;
use duckdb::{Connection, OptionalExt};
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

impl Store {
    /// Returns true if a `preregistrations` row exists for `track_id`.
    /// Used by `rebuild_from_prereg_files` instead of `get_track` because
    /// a `tracks` row can exist without a matching `preregistrations`
    /// row (e.g. `write_prereg` wrote `prereg.md` but crashed or failed
    /// before the git commit / row insert), and that half-written state
    /// must still be treated as needing a rebuild.
    fn has_prereg_row(&self, track_id: &str) -> Result<bool, TrackError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM preregistrations WHERE track_id = ?",
                duckdb::params![track_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Re-derive `tracks` and `preregistrations` rows by reading every
    /// `<tracks_dir>/<id>/prereg.md` on disk. Used to recover after
    /// `zorp.duckdb` is lost or deleted; the files, checked against
    /// their git-committed content, are the source of truth. Also
    /// self-heals a half-written pre-registration: if
    /// `write_prereg` wrote `prereg.md` to disk but failed before
    /// inserting the `preregistrations` row (e.g. the git commit step
    /// failed), a `tracks` row may already exist but the
    /// `preregistrations` row will not. Skips a track directory if it
    /// has no `prereg.md` (nothing to rebuild from) or already has a
    /// matching `preregistrations` row; otherwise backfills the
    /// `tracks` row (if missing) and the `preregistrations` row.
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
            if self.has_prereg_row(track_id)? {
                continue; // already present, nothing to rebuild
            }

            let content = std::fs::read_to_string(&prereg_path)?;
            let (hypothesis, metric_name, kill_threshold) = crate::prereg::parse_prereg_md(&content)?;
            let file_hash = crate::prereg::sha256_hex(content.as_bytes());
            let git_commit_hash = std::process::Command::new("git")
                .arg("-C")
                .arg(&track_dir)
                .args(["log", "-1", "--format=%H", "--", "prereg.md"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty());

            // Git is the root of trust on rebuild. Re-blessing whatever
            // is on disk would let anyone launder a tampered prereg.md
            // by deleting or corrupting the DuckDB store: the committed
            // blob must hash to the same value as the working tree file
            // before its hash is stored as authoritative. A file with no
            // commit backing it gets a distinct unverified marker
            // instead of being presented as equivalent to a committed
            // one.
            let stored_hash = match git_commit_hash.as_deref() {
                Some(commit) => {
                    let blob_hash = crate::prereg::git_blob_hash(&track_dir, commit).ok_or_else(|| {
                        TrackError::IntegrityMismatch {
                            track_id: track_id.to_string(),
                            detail: format!(
                                "prereg.md could not be read from its last git commit {commit} during rebuild"
                            ),
                        }
                    })?;
                    if blob_hash != file_hash {
                        return Err(TrackError::IntegrityMismatch {
                            track_id: track_id.to_string(),
                            detail: format!(
                                "prereg.md on disk does not match its last git-committed content ({commit}); refusing to rebuild from the tampered file"
                            ),
                        });
                    }
                    file_hash
                }
                None => format!("{}{file_hash}", crate::prereg::UNVERIFIED_HASH_PREFIX),
            };

            if self.get_track(track_id).is_err() {
                self.create_track(track_id, &hypothesis)?;
            }
            let committed_at_ms = std::fs::metadata(&prereg_path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            crate::prereg::insert_preregistration_row(
                self,
                track_id,
                &hypothesis,
                &metric_name,
                kill_threshold,
                &prereg_path,
                &stored_hash,
                git_commit_hash.as_deref(),
                committed_at_ms,
            )?;
            rebuilt += 1;
        }
        Ok(rebuilt)
    }
}

impl Store {
    /// Verify prereg integrity in both directions across every track:
    /// every `preregistrations` row must have a matching, hash-correct
    /// `prereg.md` file on disk (the existing per-track check in
    /// `crate::prereg::verify_prereg_integrity`), and every
    /// `<tracks_dir>/<id>/prereg.md` on disk must have a corresponding
    /// row (otherwise it is an orphan file that would never be
    /// surfaced). Returns `TrackError::IntegrityMismatch` on the first
    /// mismatch found in either direction.
    pub fn verify_all_prereg_integrity(&self, tracks_dir: &Path) -> Result<(), TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, file_path, file_hash, git_commit_hash, file_mtime_ms, file_len FROM preregistrations",
        )?;
        let rows: Vec<crate::prereg::PreregIntegrityRow> = stmt
            .query_map([], |r| {
                Ok(crate::prereg::PreregIntegrityRow {
                    track_id: r.get(0)?,
                    file_path: r.get(1)?,
                    file_hash: r.get(2)?,
                    git_commit_hash: r.get(3)?,
                    file_mtime_ms: r.get(4)?,
                    file_len: r.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        drop(stmt);

        for row in &rows {
            // Fast path: an unchanged (mtime, len) means the file was not
            // touched since the last full check, so skip re-reading and
            // re-hashing it. This is only a change detector; any change,
            // or a row with no cached stamp yet, falls through to the
            // full check.
            if let Some(stamp) = crate::prereg::file_stamp(Path::new(&row.file_path)) {
                if row.file_mtime_ms == Some(stamp.0) && row.file_len == Some(stamp.1) {
                    continue;
                }
            }
            crate::prereg::full_verify_row(self, row)?;
        }
        let track_ids: Vec<&str> = rows.iter().map(|r| r.track_id.as_str()).collect();

        let Ok(entries) = std::fs::read_dir(tracks_dir) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let track_dir = entry.path();
            if !track_dir.is_dir() {
                continue;
            }
            let prereg_path = track_dir.join("prereg.md");
            if !prereg_path.exists() {
                continue;
            }
            let Some(track_id) = track_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !track_ids.iter().any(|t| *t == track_id) {
                return Err(TrackError::IntegrityMismatch {
                    track_id: track_id.to_string(),
                    detail: format!(
                        "prereg.md exists on disk at {} but no preregistrations row was found",
                        prereg_path.display()
                    ),
                });
            }
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
                "tracks",
                "validations"
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

    fn init_git_repo(dir: &Path) {
        std::process::Command::new("git").arg("-C").arg(dir).args(["init", "-q"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.email", "test@example.com"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.name", "Test"]).output().unwrap();
    }

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

        // No git repo here, so the rebuilt hash cannot be verified
        // against a committed blob: it must carry the unverified marker
        // rather than posing as a committed, tamper-evident hash.
        let prereg = crate::prereg::get_preregistration(&fresh_store, "t1").unwrap().unwrap();
        assert!(
            prereg.file_hash.starts_with(crate::prereg::UNVERIFIED_HASH_PREFIX),
            "expected an unverified marker, got: {}",
            prereg.file_hash
        );
    }

    #[test]
    fn rebuild_verifies_a_committed_prereg_and_stores_a_plain_hash() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let db_path = dir.path().join("zorp.duckdb");
        let tracks_dir = dir.path().join("tracks");
        {
            let store = Store::open(&db_path).unwrap();
            store.create_track("t1", "does caching help").unwrap();
            crate::prereg::write_prereg(&store, &tracks_dir.join("t1"), "t1", "does caching help", "latency_ms", 100.0).unwrap();
        }

        std::fs::remove_file(&db_path).unwrap();

        let fresh_store = Store::open(&db_path).unwrap();
        assert_eq!(fresh_store.rebuild_from_prereg_files(&tracks_dir).unwrap(), 1);
        assert!(crate::prereg::verify_prereg_integrity(&fresh_store, "t1").is_ok());
        let prereg = crate::prereg::get_preregistration(&fresh_store, "t1").unwrap().unwrap();
        assert!(prereg.git_commit_hash.is_some());
        assert!(!prereg.file_hash.starts_with(crate::prereg::UNVERIFIED_HASH_PREFIX));
    }

    #[test]
    fn rebuild_refuses_a_prereg_md_tampered_after_its_commit() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let db_path = dir.path().join("zorp.duckdb");
        let tracks_dir = dir.path().join("tracks");
        {
            let store = Store::open(&db_path).unwrap();
            store.create_track("t1", "does caching help").unwrap();
            crate::prereg::write_prereg(&store, &tracks_dir.join("t1"), "t1", "does caching help", "latency_ms", 100.0).unwrap();
        }

        // Tamper with the committed file, then destroy the DuckDB store.
        // A rebuild must compare the file against its committed blob and
        // refuse, not re-bless the tampered file as authoritative.
        std::fs::write(
            tracks_dir.join("t1").join("prereg.md"),
            "# Pre-registration: t1\n\nHypothesis: does caching help\nMetric: latency_ms\nKill threshold: 999999\n",
        )
        .unwrap();
        std::fs::remove_file(&db_path).unwrap();

        let fresh_store = Store::open(&db_path).unwrap();
        let err = fresh_store.rebuild_from_prereg_files(&tracks_dir).unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
        // The tampered track must not have been inserted.
        assert!(crate::prereg::get_preregistration(&fresh_store, "t1").unwrap().is_none());
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

    #[test]
    fn verify_all_passes_for_a_clean_store() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("zorp.duckdb");
        let tracks_dir = dir.path().join("tracks");
        let store = Store::open(&db_path).unwrap();
        store.create_track("t1", "does caching help").unwrap();
        let track_dir = tracks_dir.join("t1");
        crate::prereg::write_prereg(&store, &track_dir, "t1", "does caching help", "m", 1.0).unwrap();

        assert!(store.verify_all_prereg_integrity(&tracks_dir).is_ok());
    }

    #[test]
    fn verify_all_detects_an_orphan_prereg_file_with_no_row() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("zorp.duckdb");
        let tracks_dir = dir.path().join("tracks");
        let store = Store::open(&db_path).unwrap();
        // A track row exists, but no preregistrations row was ever
        // inserted for it, and `rebuild_from_prereg_files` was not run
        // first: this exercises `verify_all_prereg_integrity` in
        // isolation. `rebuild_from_prereg_files` (tested separately)
        // does repair this scenario.
        store.create_track("t1", "does caching help").unwrap();
        let track_dir = tracks_dir.join("t1");
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(
            track_dir.join("prereg.md"),
            "# Pre-registration: t1\n\nHypothesis: does caching help\nMetric: m\nKill threshold: 1\n",
        )
        .unwrap();

        let err = store.verify_all_prereg_integrity(&tracks_dir).unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }
}
