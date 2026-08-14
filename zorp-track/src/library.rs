use crate::TrackError;
use lancedb::arrow::arrow_array::{
    FixedSizeListArray, RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray,
};
use lancedb::arrow::arrow_schema::{DataType, Field, Field as ArrowField, Schema};
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
                .map_err(|e| lancedb::Error::Other {
                    message: e.to_string(),
                    source: None,
                })?;
                let reader: Box<dyn RecordBatchReader + Send> =
                    Box::new(RecordBatchIterator::new(vec![Ok(empty_batch)], schema));
                conn.create_table("library", reader).execute().await?;
            }
            Ok::<_, lancedb::Error>(conn)
        })?;
        Ok(Library {
            runtime,
            connection,
        })
    }

    /// The names of tables currently in this store, `["library"]` right
    /// after `open` on a fresh path.
    pub fn table_names(&self) -> Result<Vec<String>, TrackError> {
        Ok(self
            .runtime
            .block_on(self.connection.table_names().execute())?)
    }
}

fn source_schema(dim: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("track_id", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("source", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(ArrowField::new("item", DataType::Float32, true)),
                dim,
            ),
            false,
        ),
    ]))
}

fn source_batch(
    schema: Arc<Schema>,
    track_id: &str,
    kind: &str,
    text: &str,
    source: &str,
    embedding: &[f32],
) -> Result<RecordBatch, TrackError> {
    let dim = embedding.len() as i32;
    let values = lancedb::arrow::arrow_array::Float32Array::from(embedding.to_vec());
    let vector_array = FixedSizeListArray::try_new(
        Arc::new(ArrowField::new("item", DataType::Float32, true)),
        dim,
        Arc::new(values),
        None,
    )
    .map_err(|e| TrackError::Library(e.to_string()))?;
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![track_id])),
            Arc::new(StringArray::from(vec![kind])),
            Arc::new(StringArray::from(vec![text])),
            Arc::new(StringArray::from(vec![source])),
            Arc::new(vector_array),
        ],
    )
    .map_err(|e| TrackError::Library(e.to_string()))
}

impl Library {
    /// Embed and store one source, including its provenance (`source`, the
    /// URL or citation it came from) alongside its `text`. Lazily creates
    /// the `sources` table on the first call, with its vector column's
    /// dimension inferred from that first `embedding`'s length. Later calls
    /// append; passing an embedding of a different length than the table's
    /// fixed dimension is a `TrackError::Library` error, not a silent
    /// failure.
    pub fn insert_source(
        &self,
        track_id: &str,
        kind: &str,
        text: &str,
        source: &str,
        embedding: &[f32],
    ) -> Result<(), TrackError> {
        self.runtime.block_on(async {
            let existing = self.connection.table_names().execute().await?;
            let schema = source_schema(embedding.len() as i32);
            let batch = source_batch(schema.clone(), track_id, kind, text, source, embedding)?;
            let reader: Box<dyn RecordBatchReader + Send> =
                Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema));
            if existing.iter().any(|n| n == "sources") {
                let tbl = self.connection.open_table("sources").execute().await?;
                tbl.add(reader).execute().await?;
            } else {
                self.connection
                    .create_table("sources", reader)
                    .execute()
                    .await?;
            }
            Ok::<(), TrackError>(())
        })
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
        assert_eq!(
            reopened.unwrap().table_names().unwrap(),
            vec!["library".to_string()]
        );
    }

    #[test]
    fn insert_source_creates_the_table_on_first_call() {
        let dir = tempdir().unwrap();
        let library = Library::open(&dir.path().join("lancedb")).unwrap();
        library
            .insert_source(
                "t1",
                "validate-source",
                "a snippet",
                "https://example.com/paper",
                &[0.1, 0.2, 0.3],
            )
            .unwrap();
        let names = library.table_names().unwrap();
        assert!(names.contains(&"sources".to_string()));
    }

    #[test]
    fn insert_source_appends_on_second_call() {
        let dir = tempdir().unwrap();
        let library = Library::open(&dir.path().join("lancedb")).unwrap();
        library
            .insert_source(
                "t1",
                "validate-source",
                "first",
                "https://example.com/1",
                &[0.1, 0.2],
            )
            .unwrap();
        library
            .insert_source(
                "t1",
                "validate-source",
                "second",
                "https://example.com/2",
                &[0.3, 0.4],
            )
            .unwrap();
        let count = library.runtime.block_on(async {
            library
                .connection
                .open_table("sources")
                .execute()
                .await
                .unwrap()
                .count_rows(None)
                .await
                .unwrap()
        });
        assert_eq!(count, 2);
    }
}
