use crate::track::Store;
use crate::TrackError;
use duckdb::OptionalExt;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub struct Preregistration {
    pub id: String,
    pub track_id: String,
    pub hypothesis_snapshot: String,
    pub metric_name: String,
    pub kill_threshold: f64,
    pub file_path: PathBuf,
    pub file_hash: String,
    pub git_commit_hash: Option<String>,
    pub committed_at: i64,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn render_prereg_md(track_id: &str, hypothesis: &str, metric_name: &str, kill_threshold: f64) -> String {
    format!(
        "# Pre-registration: {track_id}\n\n\
         Hypothesis: {hypothesis}\n\
         Metric: {metric_name}\n\
         Kill threshold: {kill_threshold}\n"
    )
}

/// Parse a `prereg.md` written by `render_prereg_md` back into its
/// fields. Used both to verify integrity and, in Task 6, to rebuild the
/// DuckDB index from files alone.
pub(crate) fn parse_prereg_md(content: &str) -> Result<(String, String, f64), TrackError> {
    let mut hypothesis = None;
    let mut metric_name = None;
    let mut kill_threshold = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("Hypothesis: ") {
            hypothesis = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Metric: ") {
            metric_name = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Kill threshold: ") {
            kill_threshold = v.parse::<f64>().ok();
        }
    }
    match (hypothesis, metric_name, kill_threshold) {
        (Some(h), Some(m), Some(k)) => Ok((h, m, k)),
        _ => Err(TrackError::Io(
            "prereg.md missing a required field".to_string(),
        )),
    }
}

fn is_git_repo(dir: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String, TrackError> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| TrackError::Io(format!("git: {e}")))?;
    if !out.status.success() {
        return Err(TrackError::Io(format!(
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Insert a preregistration row into the database. This is the single
/// authoritative place where the preregistration INSERT statement is defined.
pub(crate) fn insert_preregistration_row(
    store: &Store,
    track_id: &str,
    hypothesis: &str,
    metric_name: &str,
    kill_threshold: f64,
    file_path: &Path,
    file_hash: &str,
    git_commit_hash: Option<&str>,
    committed_at: i64,
) -> Result<(), TrackError> {
    let id = format!("{track_id}-prereg");
    store.conn.execute(
        "INSERT INTO preregistrations \
         (id, track_id, hypothesis_snapshot, metric_name, kill_threshold, file_path, file_hash, git_commit_hash, committed_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        duckdb::params![
            id,
            track_id,
            hypothesis,
            metric_name,
            kill_threshold,
            file_path.to_string_lossy().to_string(),
            file_hash,
            git_commit_hash,
            committed_at
        ],
    )?;
    Ok(())
}

/// Write a pre-registration: the `prereg.md` file, a git commit of just
/// that file (if `track_dir` is inside a git repository), and the
/// corresponding `preregistrations` row.
///
/// Not idempotent by design: a `track_id` may only be pre-registered
/// once. A second call for the same `track_id` returns
/// `TrackError::AlreadyRegistered` before touching disk or git, so a
/// caller that mistakenly re-registers a track cannot overwrite the
/// file (and permanently wedge its integrity check) out from under the
/// first, already-committed registration.
pub fn write_prereg(
    store: &Store,
    track_dir: &Path,
    track_id: &str,
    hypothesis: &str,
    metric_name: &str,
    kill_threshold: f64,
) -> Result<Preregistration, TrackError> {
    let already_registered: bool = store
        .conn
        .query_row(
            "SELECT 1 FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if already_registered {
        return Err(TrackError::AlreadyRegistered {
            track_id: track_id.to_string(),
        });
    }

    fs::create_dir_all(track_dir)?;
    let file_path = track_dir.join("prereg.md");
    let content = render_prereg_md(track_id, hypothesis, metric_name, kill_threshold);
    fs::write(&file_path, &content)?;
    let file_hash = sha256_hex(content.as_bytes());

    let git_commit_hash = if is_git_repo(track_dir) {
        run_git(track_dir, &["add", "--", file_path.to_str().unwrap_or("")])?;
        run_git(
            track_dir,
            &[
                "commit",
                "-m",
                &format!("prereg({track_id}): pre-registration"),
                "--",
                file_path.to_str().unwrap_or(""),
            ],
        )?;
        Some(run_git(track_dir, &["rev-parse", "HEAD"])?)
    } else {
        None
    };

    let id = format!("{track_id}-prereg");
    let committed_at = now_millis();
    insert_preregistration_row(
        store,
        track_id,
        hypothesis,
        metric_name,
        kill_threshold,
        &file_path,
        &file_hash,
        git_commit_hash.as_deref(),
        committed_at,
    )?;

    Ok(Preregistration {
        id,
        track_id: track_id.to_string(),
        hypothesis_snapshot: hypothesis.to_string(),
        metric_name: metric_name.to_string(),
        kill_threshold,
        file_path,
        file_hash,
        git_commit_hash,
        committed_at,
    })
}

/// Read back the `preregistrations` row for `track_id`, if one exists.
/// `None` means no pre-registration has been written yet for this track
/// (a normal state for a fresh track, not an error); any other failure
/// to read is a real `TrackError`.
pub fn get_preregistration(store: &Store, track_id: &str) -> Result<Option<Preregistration>, TrackError> {
    let row = store
        .conn
        .query_row(
            "SELECT id, track_id, hypothesis_snapshot, metric_name, kill_threshold, file_path, file_hash, git_commit_hash, committed_at \
             FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |r| {
                let file_path: String = r.get(5)?;
                Ok(Preregistration {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    hypothesis_snapshot: r.get(2)?,
                    metric_name: r.get(3)?,
                    kill_threshold: r.get(4)?,
                    file_path: PathBuf::from(file_path),
                    file_hash: r.get(6)?,
                    git_commit_hash: r.get(7)?,
                    committed_at: r.get(8)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Verify that the `preregistrations` row for `track_id` matches the
/// `prereg.md` file on disk: the file must exist, and its current
/// SHA-256 must match what was recorded at commit time.
pub fn verify_prereg_integrity(store: &Store, track_id: &str) -> Result<(), TrackError> {
    let (file_path, file_hash): (String, String) = store
        .conn
        .query_row(
            "SELECT file_path, file_hash FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            duckdb::Error::QueryReturnedNoRows => TrackError::IntegrityMismatch {
                track_id: track_id.to_string(),
                detail: "no preregistration row found".to_string(),
            },
            other => TrackError::from(other),
        })?;

    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(TrackError::IntegrityMismatch {
            track_id: track_id.to_string(),
            detail: format!("prereg.md missing at {file_path}"),
        });
    }
    let current_content = fs::read(path)?;
    let current_hash = sha256_hex(&current_content);
    if current_hash != file_hash {
        return Err(TrackError::IntegrityMismatch {
            track_id: track_id.to_string(),
            detail: "prereg.md content does not match the hash recorded at commit time".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Store;
    use tempfile::tempdir;

    fn init_git_repo(dir: &Path) {
        std::process::Command::new("git").arg("-C").arg(dir).args(["init", "-q"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.email", "test@example.com"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.name", "Test"]).output().unwrap();
    }

    #[test]
    fn write_then_verify_succeeds_in_a_git_repo() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");

        let prereg = write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0).unwrap();
        assert!(prereg.git_commit_hash.is_some());
        assert!(prereg.file_path.exists());

        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn write_without_a_git_repo_leaves_commit_hash_none_but_still_verifies() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");

        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();
        assert_eq!(prereg.git_commit_hash, None);
        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn verify_fails_when_file_is_edited_after_commit() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();

        fs::write(&prereg.file_path, "tampered content").unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn verify_fails_when_file_is_missing() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();

        fs::remove_file(&prereg.file_path).unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn verify_fails_when_no_prereg_row_exists() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn second_write_prereg_for_the_same_track_is_rejected_and_does_not_corrupt_the_first() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");

        let first = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9).unwrap();

        let err = write_prereg(&store, &track_dir, "t1", "different hypothesis", "other_metric", 0.5)
            .unwrap_err();
        assert!(matches!(err, TrackError::AlreadyRegistered { track_id } if track_id == "t1"));

        // The first registration must be untouched: the file on disk
        // still matches what the first call wrote and recorded.
        let content = fs::read_to_string(&first.file_path).unwrap();
        assert_eq!(content, render_prereg_md("t1", "hyp", "accuracy", 0.9));
        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn parse_prereg_md_round_trips_render_prereg_md() {
        let content = render_prereg_md("t1", "does caching help", "latency_ms", 42.5);
        let (hypothesis, metric, threshold) = parse_prereg_md(&content).unwrap();
        assert_eq!(hypothesis, "does caching help");
        assert_eq!(metric, "latency_ms");
        assert_eq!(threshold, 42.5);
    }

    #[test]
    fn get_preregistration_returns_none_when_absent() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        assert_eq!(get_preregistration(&store, "t1").unwrap(), None);
    }

    #[test]
    fn get_preregistration_returns_the_written_row() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let written = write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0).unwrap();

        let read_back = get_preregistration(&store, "t1").unwrap().unwrap();
        assert_eq!(read_back, written);
    }
}
