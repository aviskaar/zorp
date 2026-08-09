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
