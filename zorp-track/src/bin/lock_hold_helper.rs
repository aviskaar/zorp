//! Test-only helper binary: opens a `Store` at the given path and holds
//! the connection open until stdin is closed, so integration tests can
//! reproduce DuckDB's real cross-process file lock (a second connection
//! to the same file from another process, which DuckDB refuses).
//!
//! Not part of the crate's public API; used only by
//! `tests/integration.rs` via `env!("CARGO_BIN_EXE_lock_hold_helper")`.

use std::io::Read;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: lock_hold_helper <db-path>");
    let path = std::path::PathBuf::from(path);
    match zorp_track::Store::open(&path) {
        Ok(_store) => {
            // Signal the parent that the lock is held, then block until
            // the parent closes our stdin (or kills us).
            println!("locked");
            let mut buf = [0u8; 1];
            let _ = std::io::stdin().read(&mut buf);
        }
        Err(e) => {
            println!("open-failed: {e}");
        }
    }
}
