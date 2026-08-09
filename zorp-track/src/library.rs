use crate::TrackError;
use lancedb::arrow::arrow_array::{RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use std::path::Path;
use std::sync::Arc;

impl From<lancedb::Error> for TrackError {
    fn from(e: lancedb::Error) -> Self {
        TrackError::Library(e.to_string())
    }
}

/// LanceDB-backed store for multimodal, semantically searchable content
/// (literature, figures, plots). What actually goes in is each
/// capability's own concern; this only provisions the store and a base
/// `library` table keyed by `track_id`.
pub struct Library {
    runtime: tokio::runtime::Runtime,
    connection: lancedb::Connection,
}

fn base_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("track_id", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
    ]))
}

impl Library {
    /// Open (creating if necessary) the LanceDB store at `path`.
    pub fn open(path: &Path) -> Result<Self, TrackError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| TrackError::Library(e.to_string()))?;
        let path_str = path.to_string_lossy().to_string();
        let connection = runtime.block_on(async {
            let conn = lancedb::connect(&path_str).execute().await?;
            let existing = conn.table_names().execute().await?;
            if !existing.iter().any(|n| n == "library") {
                let schema = base_schema();
                let empty_batch = RecordBatch::try_new(
                    schema.clone(),
                    vec![
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                    ],
                )
                .map_err(|e| lancedb::Error::Other { message: e.to_string(), source: None })?;
                let reader: Box<dyn RecordBatchReader + Send> =
                    Box::new(RecordBatchIterator::new(vec![Ok(empty_batch)], schema));
                conn.create_table("library", reader).execute().await?;
            }
            Ok::<_, lancedb::Error>(conn)
        })?;
        Ok(Library { runtime, connection })
    }

    /// The names of tables currently in this store, `["library"]` right
    /// after `open` on a fresh path.
    pub fn table_names(&self) -> Result<Vec<String>, TrackError> {
        Ok(self.runtime.block_on(self.connection.table_names().execute())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_on_fresh_path_creates_the_library_table() {
        let dir = tempdir().unwrap();
        let library = Library::open(&dir.path().join("lancedb")).unwrap();
        assert_eq!(library.table_names().unwrap(), vec!["library".to_string()]);
    }

    #[test]
    fn open_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lancedb");
        Library::open(&path).unwrap();
        let reopened = Library::open(&path);
        assert!(reopened.is_ok());
        assert_eq!(reopened.unwrap().table_names().unwrap(), vec!["library".to_string()]);
    }
}
