//! Helpers shared by the aryabhatta modules' tests.
//!
//! Only compiled under `cfg(test)`. It exists so the integrity checks
//! that weigh the whole database are written once: four modules assert
//! "this reads and never writes", and four copies of the census would
//! drift apart the first time a table was added.

use crate::track::Store;

/// Every table in the store and how many rows it holds, so a test can
/// compare the whole database before and after an operation.
///
/// Read from `information_schema` rather than from a list in the test,
/// so a table added later is covered without anyone remembering to add
/// it here.
pub(crate) fn table_counts(store: &Store) -> Vec<(String, i64)> {
    let mut stmt = store
        .conn
        .prepare(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'main' ORDER BY table_name",
        )
        .unwrap();
    let names: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(
        !names.is_empty(),
        "the census found no tables, so comparing it before and after would pass vacuously"
    );
    names
        .into_iter()
        .map(|name| {
            let count: i64 = store
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |r| r.get(0))
                .unwrap();
            (name, count)
        })
        .collect()
}
