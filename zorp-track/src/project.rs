#[cfg(feature = "library")]
use crate::library::Library;
use crate::track::Store;
use crate::TrackError;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GITIGNORE_CONTENT: &str = "zorp.duckdb\nlancedb/\n";

/// Substrings that show up in DuckDB's error message when `Store::open`
/// fails because another connection already holds the file's lock (a
/// second concurrent `Project::open` on the same path, whether from a
/// second process or a second `Project` in the current process), as
/// opposed to the file's contents actually being corrupt/unreadable.
/// DuckDB does not currently expose a dedicated error variant for "file
/// is locked" (see `duckdb::Error::DuckDBFailure`, which wraps a bare
/// error code plus a free-form message), so this inspects the message
/// text. Keyed off the specific phrasing DuckDB has used historically
/// ("Could not set lock on file", "Conflicting lock is held") plus
/// "being used by another process", deliberately not the bare word
/// "lock": DuckDB's checksum-corruption message contains the substring
/// "in block" (from "...checksum M in block at location..."), and a
/// project path can itself contain "lock" (e.g. `unlock-utils/`), so a
/// bare substring match would misclassify real corruption as lock
/// contention and skip recovery. A wording change in a future DuckDB
/// version degrades to a false negative (treated as corruption) rather
/// than a false positive silently destroying a healthy, in-use
/// database.
fn is_lock_error(err: &TrackError) -> bool {
    let TrackError::Db(msg) = err else {
        return false;
    };
    let msg = msg.to_lowercase();
    [
        "could not set lock on file",
        "conflicting lock is held",
        "being used by another process",
    ]
    .iter()
    .any(|kw| msg.contains(kw))
}

/// Open the DuckDB store at `db_path`, recovering from a corrupted file:
/// if the first open fails, the bad file is renamed aside (so it is not
/// silently lost) and a fresh, empty store is opened in its place. The
/// caller is responsible for repopulating it, e.g. via
/// `rebuild_from_prereg_files`, since prereg.md files remain the source
/// of truth.
///
/// A lock error (another connection already has `db_path` open, e.g. a
/// concurrent `Project::open`) is deliberately excluded from recovery:
/// it is not corruption, and quarantining the file out from under a
/// healthy, currently-in-use database would silently lose
/// `experiments`/`metrics`/`checkpoints` data that has no file-backed
/// source of truth to rebuild from. See `is_lock_error`.
fn open_store_recovering_from_corruption(db_path: &Path) -> Result<Store, TrackError> {
    match Store::open(db_path) {
        Ok(store) => Ok(store),
        Err(e) if db_path.exists() && !is_lock_error(&e) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let quarantined = db_path.with_extension(format!("duckdb.corrupted-{now}"));
            std::fs::rename(db_path, &quarantined)?;
            Store::open(db_path)
        }
        Err(e) => Err(e),
    }
}

/// The single entry point for a project's `.zorp/` directory: opens (or
/// creates) the DuckDB run record and a `.gitignore` covering the
/// regenerable stores while leaving `tracks/*/prereg.md` tracked. The
/// LanceDB library (behind the `library` feature) is opened lazily on
/// first use via `Project::library`, so nothing touches it otherwise.
pub struct Project {
    root: PathBuf,
    pub store: Store,
    #[cfg(feature = "library")]
    library: std::cell::OnceCell<Library>,
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
        let store = open_store_recovering_from_corruption(&db_path)?;
        // rebuild_from_prereg_files skips any track that already has a
        // matching row, so this is safe (and cheap) to run on every
        // open, not just when the DB file was previously absent. That
        // is what picks up a prereg.md added to tracks/ after the DB
        // already existed, e.g. pulled in by a teammate via git.
        store.rebuild_from_prereg_files(&tracks_dir)?;
        store.verify_all_prereg_integrity(&tracks_dir)?;

        Ok(Project {
            root: zorp_dir,
            store,
            #[cfg(feature = "library")]
            library: std::cell::OnceCell::new(),
        })
    }

    /// The LanceDB library for this project, opened (and created on
    /// disk) only on the first call.
    #[cfg(feature = "library")]
    pub fn library(&self) -> Result<&Library, TrackError> {
        if self.library.get().is_none() {
            let opened = Library::open(&self.root.join("lancedb"))?;
            let _ = self.library.set(opened);
        }
        Ok(self.library.get().expect("library cell was just filled"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Store;
    use tempfile::tempdir;

    /// Reproduces the half-written pre-registration left behind when
    /// `write_prereg` writes `prereg.md` but fails (or the process
    /// crashes) before it can insert the `preregistrations` row, e.g. a
    /// failing git commit. Before the fix, `rebuild_from_prereg_files`
    /// skipped this track because a `tracks` row already existed, so
    /// `verify_all_prereg_integrity` hard-errored on every subsequent
    /// `Project::open`, permanently locking the project out.
    #[test]
    fn project_open_self_heals_a_prereg_md_with_no_preregistrations_row() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let zorp_dir = root.join(".zorp");
        let tracks_dir = zorp_dir.join("tracks");
        let track_dir = tracks_dir.join("t1");
        let db_path = zorp_dir.join("zorp.duckdb");

        // Simulate a `tracks` row already existing (created before
        // `write_prereg` was called) and `prereg.md` written to disk,
        // but no `preregistrations` row: `write_prereg`'s git commit
        // step is assumed to have failed or the process crashed in that
        // window.
        std::fs::create_dir_all(&zorp_dir).unwrap();
        {
            let store = Store::open(&db_path).unwrap();
            store.create_track("t1", "does caching help").unwrap();
        }
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(
            track_dir.join("prereg.md"),
            "# Pre-registration: t1\n\nHypothesis: does caching help\nMetric: latency_ms\nKill threshold: 100\n",
        )
        .unwrap();

        // Before the fix this returned TrackError::IntegrityMismatch on
        // every call, forever, since there was no way to get a Project
        // handle to repair it.
        let project = Project::open(root).unwrap();

        // The track is now fully readable: both a `tracks` row and a
        // `preregistrations` row exist, and integrity checks pass.
        let track = project.store.get_track("t1").unwrap();
        assert_eq!(track.hypothesis, "does caching help");
        assert!(crate::prereg::verify_prereg_integrity(&project.store, "t1").is_ok());
        assert!(project
            .store
            .verify_all_prereg_integrity(&tracks_dir)
            .is_ok());

        // Reopening again must also succeed (idempotent, not just a
        // one-time repair).
        drop(project);
        assert!(Project::open(root).is_ok());
    }

    #[test]
    fn is_lock_error_classifies_duckdbs_actual_lock_message_as_a_lock_error() {
        // The real message DuckDB (1.x, bundled) returns when a second
        // connection is attempted on a file another process already
        // has open, observed directly from `duckdb::Connection::open`.
        let err = TrackError::Db(
            "IO Error: Could not set lock on file \"/tmp/zorp.duckdb\": \
             Conflicting lock is held in /usr/bin/other-process (PID 123) by user someone. \
             See also https://duckdb.org/docs/stable/connect/concurrency"
                .to_string(),
        );
        assert!(is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_classifies_a_permission_style_lock_message_as_a_lock_error() {
        let err = TrackError::Db("database is being used by another process".to_string());
        assert!(is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_does_not_classify_a_generic_corruption_message_as_a_lock_error() {
        let err = TrackError::Db(
            "IO Error: Failed to read file \"/tmp/zorp.duckdb\": file is not a valid DuckDB database file"
                .to_string(),
        );
        assert!(!is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_does_not_classify_non_db_errors_as_lock_errors() {
        let err = TrackError::Io("permission denied".to_string());
        assert!(!is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_does_not_classify_a_checksum_corruption_message_as_a_lock_error() {
        // The real message DuckDB returns for single-byte corruption of a
        // zorp.duckdb file: the substring "in block" (from "...checksum
        // M in block at location...") contains "lock", which a bare
        // substring check on "lock" would misclassify as lock
        // contention, silently skipping corruption recovery.
        let err = TrackError::Db(
            "IO Error: Corrupt database file: computed checksum 12345 does not match \
             stored checksum 67890 in block at location 0"
                .to_string(),
        );
        assert!(!is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_does_not_classify_a_corruption_message_with_lock_in_the_path_as_a_lock_error()
    {
        // A project directory whose path happens to contain the
        // substring "lock" (e.g. a repo named `blockchain/` or
        // `unlock-utils/`) embeds that substring in the error message
        // via the file path, independent of whether the failure is
        // actually lock contention.
        let err = TrackError::Db(
            "IO Error: Corrupt database file: computed checksum 111 does not match stored \
             checksum 222 in block at location 0 while reading \
             \"/home/user/unlock-utils/.zorp/zorp.duckdb\""
                .to_string(),
        );
        assert!(!is_lock_error(&err));
    }
}
