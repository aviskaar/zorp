use crate::library::Library;
use crate::track::Store;
use crate::TrackError;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GITIGNORE_CONTENT: &str = "zorp.duckdb\nlancedb/\n";

/// Open the DuckDB store at `db_path`, recovering from a corrupted file:
/// if the first open fails, the bad file is renamed aside (so it is not
/// silently lost) and a fresh, empty store is opened in its place. The
/// caller is responsible for repopulating it, e.g. via
/// `rebuild_from_prereg_files`, since prereg.md files remain the source
/// of truth.
fn open_store_recovering_from_corruption(db_path: &Path) -> Result<Store, TrackError> {
    match Store::open(db_path) {
        Ok(store) => Ok(store),
        Err(_) if db_path.exists() => {
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
        let store = open_store_recovering_from_corruption(&db_path)?;
        // rebuild_from_prereg_files skips any track that already has a
        // matching row, so this is safe (and cheap) to run on every
        // open, not just when the DB file was previously absent. That
        // is what picks up a prereg.md added to tracks/ after the DB
        // already existed, e.g. pulled in by a teammate via git.
        store.rebuild_from_prereg_files(&tracks_dir)?;
        store.verify_all_prereg_integrity(&tracks_dir)?;

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
