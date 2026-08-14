use crate::track::Store;
use crate::TrackError;
use duckdb::OptionalExt;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Which way the pre-registered metric is supposed to move, and
/// therefore which side of the kill threshold kills the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdDirection {
    /// Lower metric values are better (latency, error rate); the track
    /// is killed when the metric goes above the threshold.
    LowerIsBetter,
    /// Higher metric values are better (accuracy, throughput); the
    /// track is killed when the metric goes below the threshold.
    HigherIsBetter,
}

impl ThresholdDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThresholdDirection::LowerIsBetter => "lower-is-better",
            ThresholdDirection::HigherIsBetter => "higher-is-better",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lower-is-better" => Some(ThresholdDirection::LowerIsBetter),
            "higher-is-better" => Some(ThresholdDirection::HigherIsBetter),
            _ => None,
        }
    }

    /// True when `value` crosses the kill threshold in the killing
    /// direction. Landing exactly on the threshold is not a breach.
    pub fn breached(&self, value: f64, threshold: f64) -> bool {
        match self {
            ThresholdDirection::LowerIsBetter => value > threshold,
            ThresholdDirection::HigherIsBetter => value < threshold,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preregistration {
    pub id: String,
    pub track_id: String,
    pub hypothesis_snapshot: String,
    pub metric_name: String,
    pub kill_threshold: f64,
    /// `None` only on rows rebuilt from a legacy prereg.md written
    /// before directions existed; new registrations always record one.
    pub threshold_direction: Option<ThresholdDirection>,
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

fn render_prereg_md(
    track_id: &str,
    hypothesis: &str,
    metric_name: &str,
    kill_threshold: f64,
    direction: ThresholdDirection,
) -> String {
    format!(
        "# Pre-registration: {track_id}\n\n\
         Hypothesis: {hypothesis}\n\
         Metric: {metric_name}\n\
         Kill threshold: {kill_threshold}\n\
         Threshold direction: {}\n",
        direction.as_str()
    )
}

/// Parse a `prereg.md` written by `render_prereg_md` back into its
/// fields. Used both to verify integrity and, in Task 6, to rebuild the
/// DuckDB index from files alone. The direction is `None` for legacy
/// files written before directions existed; the other fields are
/// required.
pub(crate) fn parse_prereg_md(
    content: &str,
) -> Result<(String, String, f64, Option<ThresholdDirection>), TrackError> {
    let mut hypothesis = None;
    let mut metric_name = None;
    let mut kill_threshold = None;
    let mut direction = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("Hypothesis: ") {
            hypothesis = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Metric: ") {
            metric_name = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Kill threshold: ") {
            kill_threshold = v.parse::<f64>().ok();
        } else if let Some(v) = line.strip_prefix("Threshold direction: ") {
            direction = ThresholdDirection::parse(v.trim());
        }
    }
    match (hypothesis, metric_name, kill_threshold) {
        (Some(h), Some(m), Some(k)) => Ok((h, m, k, direction)),
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

/// The prereg.md file's (mtime in millis, length in bytes), used only as
/// a change-detection fast path by `verify_all_prereg_integrity`, never
/// as integrity evidence on its own.
pub(crate) fn file_stamp(path: &Path) -> Option<(i64, i64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)?;
    Some((mtime_ms, meta.len() as i64))
}

/// Insert a preregistration row into the database. This is the single
/// authoritative place where the preregistration INSERT statement is defined.
pub(crate) fn insert_preregistration_row(
    store: &Store,
    track_id: &str,
    hypothesis: &str,
    metric_name: &str,
    kill_threshold: f64,
    threshold_direction: Option<ThresholdDirection>,
    file_path: &Path,
    file_hash: &str,
    git_commit_hash: Option<&str>,
    committed_at: i64,
) -> Result<(), TrackError> {
    let id = format!("{track_id}-prereg");
    let (file_mtime_ms, file_len) = match file_stamp(file_path) {
        Some((m, l)) => (Some(m), Some(l)),
        None => (None, None),
    };
    store.conn.execute(
        "INSERT INTO preregistrations \
         (id, track_id, hypothesis_snapshot, metric_name, kill_threshold, threshold_direction, file_path, file_hash, git_commit_hash, committed_at, file_mtime_ms, file_len) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        duckdb::params![
            id,
            track_id,
            hypothesis,
            metric_name,
            kill_threshold,
            threshold_direction.map(|d| d.as_str()),
            file_path.to_string_lossy().to_string(),
            file_hash,
            git_commit_hash,
            committed_at,
            file_mtime_ms,
            file_len
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
    threshold_direction: ThresholdDirection,
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
    let content = render_prereg_md(track_id, hypothesis, metric_name, kill_threshold, threshold_direction);
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
        Some(threshold_direction),
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
        threshold_direction: Some(threshold_direction),
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
            "SELECT id, track_id, hypothesis_snapshot, metric_name, kill_threshold, threshold_direction, file_path, file_hash, git_commit_hash, committed_at \
             FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |r| {
                let direction: Option<String> = r.get(5)?;
                let file_path: String = r.get(6)?;
                Ok(Preregistration {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    hypothesis_snapshot: r.get(2)?,
                    metric_name: r.get(3)?,
                    kill_threshold: r.get(4)?,
                    threshold_direction: direction.as_deref().and_then(ThresholdDirection::parse),
                    file_path: PathBuf::from(file_path),
                    file_hash: r.get(7)?,
                    git_commit_hash: r.get(8)?,
                    committed_at: r.get(9)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Marker prefixed to a stored file hash when the row was rebuilt from a
/// prereg.md that has no git commit backing it: the hash was
/// self-attested at rebuild time, not tamper-evident, so it must not be
/// presented as equivalent to a committed one.
pub const UNVERIFIED_HASH_PREFIX: &str = "unverified:";

/// The plain SHA-256 a stored file hash asserts, with the unverified
/// marker (if any) stripped.
pub(crate) fn asserted_hash(stored: &str) -> &str {
    stored.strip_prefix(UNVERIFIED_HASH_PREFIX).unwrap_or(stored)
}

/// The columns `full_verify_row` needs, read either one row at a time
/// (`verify_prereg_integrity`) or all at once
/// (`verify_all_prereg_integrity`).
pub(crate) struct PreregIntegrityRow {
    pub(crate) track_id: String,
    pub(crate) file_path: String,
    pub(crate) file_hash: String,
    pub(crate) git_commit_hash: Option<String>,
    pub(crate) file_mtime_ms: Option<i64>,
    pub(crate) file_len: Option<i64>,
}

/// The full integrity check for one preregistration row: the file must
/// exist, its current SHA-256 must match what was recorded at commit
/// time, and when a git commit was recorded, that commit must still
/// exist and its prereg.md blob must hash to the same recorded value
/// (so tampering with the row's stored hash cannot be laundered by also
/// rewriting history). On success, the row's cached (mtime, len) is
/// refreshed so the next `verify_all_prereg_integrity` can skip
/// re-hashing an unchanged file.
pub(crate) fn full_verify_row(store: &Store, row: &PreregIntegrityRow) -> Result<(), TrackError> {
    let path = Path::new(&row.file_path);
    if !path.exists() {
        return Err(TrackError::IntegrityMismatch {
            track_id: row.track_id.clone(),
            detail: format!("prereg.md missing at {}", row.file_path),
        });
    }
    let current_content = fs::read(path)?;
    let current_hash = sha256_hex(&current_content);
    if current_hash != asserted_hash(&row.file_hash) {
        return Err(TrackError::IntegrityMismatch {
            track_id: row.track_id.clone(),
            detail: "prereg.md content does not match the hash recorded at commit time".to_string(),
        });
    }

    if let Some(commit) = row.git_commit_hash.as_deref() {
        let track_dir = path.parent().unwrap_or(Path::new("."));
        let commit_exists = std::process::Command::new("git")
            .arg("-C")
            .arg(track_dir)
            .args(["cat-file", "-e", commit])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !commit_exists {
            return Err(TrackError::IntegrityMismatch {
                track_id: row.track_id.clone(),
                detail: format!("recorded git commit {commit} for prereg.md no longer exists"),
            });
        }
        let blob_hash = git_blob_hash(track_dir, commit).ok_or_else(|| TrackError::IntegrityMismatch {
            track_id: row.track_id.clone(),
            detail: format!("prereg.md could not be read from recorded git commit {commit}"),
        })?;
        if blob_hash != asserted_hash(&row.file_hash) {
            return Err(TrackError::IntegrityMismatch {
                track_id: row.track_id.clone(),
                detail: format!(
                    "prereg.md content in recorded git commit {commit} does not match the hash recorded at commit time"
                ),
            });
        }
    }

    if let Some((mtime_ms, len)) = file_stamp(path) {
        store.conn.execute(
            "UPDATE preregistrations SET file_mtime_ms = ?, file_len = ? WHERE track_id = ?",
            duckdb::params![mtime_ms, len, row.track_id],
        )?;
    }
    Ok(())
}

/// SHA-256 of the prereg.md blob as committed at `commit`, or `None` if
/// git cannot produce it. `:./prereg.md` is resolved relative to
/// `track_dir`, which is where every prereg.md lives.
pub(crate) fn git_blob_hash(track_dir: &Path, commit: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(track_dir)
        .args(["show", &format!("{commit}:./prereg.md")])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(sha256_hex(&out.stdout))
}

pub(crate) fn get_integrity_row(store: &Store, track_id: &str) -> Result<PreregIntegrityRow, TrackError> {
    store
        .conn
        .query_row(
            "SELECT track_id, file_path, file_hash, git_commit_hash, file_mtime_ms, file_len FROM preregistrations WHERE track_id = ?",
            duckdb::params![track_id],
            |r| {
                Ok(PreregIntegrityRow {
                    track_id: r.get(0)?,
                    file_path: r.get(1)?,
                    file_hash: r.get(2)?,
                    git_commit_hash: r.get(3)?,
                    file_mtime_ms: r.get(4)?,
                    file_len: r.get(5)?,
                })
            },
        )
        .map_err(|e| match e {
            duckdb::Error::QueryReturnedNoRows => TrackError::IntegrityMismatch {
                track_id: track_id.to_string(),
                detail: "no preregistration row found".to_string(),
            },
            other => TrackError::from(other),
        })
}

/// Verify that the `preregistrations` row for `track_id` matches the
/// `prereg.md` file on disk: the file must exist, and its current
/// SHA-256 must match what was recorded at commit time.
pub fn verify_prereg_integrity(store: &Store, track_id: &str) -> Result<(), TrackError> {
    let row = get_integrity_row(store, track_id)?;
    full_verify_row(store, &row)
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

        let prereg = write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0, ThresholdDirection::LowerIsBetter).unwrap();
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

        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9, ThresholdDirection::LowerIsBetter).unwrap();
        assert_eq!(prereg.git_commit_hash, None);
        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn verify_fails_when_file_is_edited_after_commit() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9, ThresholdDirection::LowerIsBetter).unwrap();

        fs::write(&prereg.file_path, "tampered content").unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn verify_fails_when_the_prereg_commit_is_rewritten_to_match_a_tampered_file() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9, ThresholdDirection::LowerIsBetter).unwrap();

        // Tamper with the file, rewrite history to cover it, and update
        // the row's stored hash to match, the way an attacker with disk
        // access would. The file-only check now passes; only the
        // recorded commit hash still points at the original
        // registration, whose blob no longer matches the stored hash.
        let tampered = "tampered content";
        fs::write(&prereg.file_path, tampered).unwrap();
        std::process::Command::new("git").arg("-C").arg(&track_dir).args(["add", "--", "prereg.md"]).output().unwrap();
        std::process::Command::new("git").arg("-C").arg(&track_dir).args(["commit", "-q", "--amend", "--no-edit"]).output().unwrap();
        store
            .conn
            .execute(
                "UPDATE preregistrations SET file_hash = ? WHERE track_id = ?",
                duckdb::params![sha256_hex(tampered.as_bytes()), "t1"],
            )
            .unwrap();

        let err = verify_prereg_integrity(&store, "t1").unwrap_err();
        assert!(matches!(err, TrackError::IntegrityMismatch { .. }));
    }

    #[test]
    fn verify_fails_when_file_is_missing() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let track_dir = dir.path().join("tracks").join("t1");
        let prereg = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9, ThresholdDirection::LowerIsBetter).unwrap();

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

        let first = write_prereg(&store, &track_dir, "t1", "hyp", "accuracy", 0.9, ThresholdDirection::LowerIsBetter).unwrap();

        let err = write_prereg(&store, &track_dir, "t1", "different hypothesis", "other_metric", 0.5, ThresholdDirection::LowerIsBetter)
            .unwrap_err();
        assert!(matches!(err, TrackError::AlreadyRegistered { track_id } if track_id == "t1"));

        // The first registration must be untouched: the file on disk
        // still matches what the first call wrote and recorded.
        let content = fs::read_to_string(&first.file_path).unwrap();
        assert_eq!(content, render_prereg_md("t1", "hyp", "accuracy", 0.9, ThresholdDirection::LowerIsBetter));
        assert!(verify_prereg_integrity(&store, "t1").is_ok());
    }

    #[test]
    fn parse_prereg_md_round_trips_render_prereg_md() {
        let content = render_prereg_md("t1", "does caching help", "latency_ms", 42.5, ThresholdDirection::HigherIsBetter);
        let (hypothesis, metric, threshold, direction) = parse_prereg_md(&content).unwrap();
        assert_eq!(hypothesis, "does caching help");
        assert_eq!(metric, "latency_ms");
        assert_eq!(threshold, 42.5);
        assert_eq!(direction, Some(ThresholdDirection::HigherIsBetter));
    }

    #[test]
    fn parse_prereg_md_without_a_direction_line_is_legacy_not_an_error() {
        let content = "# Pre-registration: t1\n\nHypothesis: h\nMetric: m\nKill threshold: 1\n";
        let (_, _, _, direction) = parse_prereg_md(content).unwrap();
        assert_eq!(direction, None);
    }

    #[test]
    fn breached_respects_the_direction() {
        let lower = ThresholdDirection::LowerIsBetter;
        assert!(lower.breached(101.0, 100.0));
        assert!(!lower.breached(100.0, 100.0));
        assert!(!lower.breached(99.0, 100.0));

        let higher = ThresholdDirection::HigherIsBetter;
        assert!(higher.breached(0.4, 0.5));
        assert!(!higher.breached(0.5, 0.5));
        assert!(!higher.breached(0.6, 0.5));
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
        let written = write_prereg(&store, &track_dir, "t1", "does caching help", "latency_ms", 100.0, ThresholdDirection::LowerIsBetter).unwrap();

        let read_back = get_preregistration(&store, "t1").unwrap().unwrap();
        assert_eq!(read_back, written);
    }
}
