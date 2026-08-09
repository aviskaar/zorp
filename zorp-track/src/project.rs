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
