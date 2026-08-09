use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::tempdir;
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::{verify_prereg_integrity, write_prereg};
use zorp_track::track::TrackStatus;
use zorp_track::{Project, TrackError};

fn init_git_repo(dir: &Path) {
    std::process::Command::new("git").arg("-C").arg(dir).args(["init", "-q"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.email", "test@example.com"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir).args(["config", "user.name", "Test"]).output().unwrap();
}

#[test]
fn full_track_lifecycle() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    // Opening a fresh project creates .zorp/, a .gitignore, and both stores.
    let project = Project::open(dir.path()).unwrap();
    assert!(dir.path().join(".zorp/.gitignore").exists());
    let gitignore = std::fs::read_to_string(dir.path().join(".zorp/.gitignore")).unwrap();
    assert!(gitignore.contains("zorp.duckdb"));
    assert!(gitignore.contains("lancedb/"));
    assert_eq!(project.library.table_names().unwrap(), vec!["library".to_string()]);

    // Create a track and pre-register it.
    let track_id = "2026-08-09-does-caching-help";
    project.store.create_track(track_id, "does caching help").unwrap();
    let track_dir = project.track_dir(track_id);
    write_prereg(&project.store, &track_dir, track_id, "does caching help", "latency_ms", 100.0).unwrap();
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());
    assert!(track_dir.join("prereg.md").exists());

    // A checkpoint, auto-approved (no interactive terminal in tests).
    let mode = CheckpointMode::terminal(true).unwrap();
    let approved = project
        .store
        .record_checkpoint(track_id, "experiment", &mode, "proceed with this experiment?")
        .unwrap();
    assert!(approved);

    // Run an experiment and record typed metrics.
    let exp = project.store.create_experiment(track_id, &format!("{track_id}-prereg")).unwrap();
    project.store.set_experiment_status(&exp.id, ExperimentStatus::Running).unwrap();
    project.store.record_metric(&exp.id, "latency_ms", MetricValue::Number(87.3)).unwrap();
    project.store.set_experiment_status(&exp.id, ExperimentStatus::Completed).unwrap();
    let metrics = project.store.metrics_for(&exp.id).unwrap();
    assert_eq!(metrics, vec![("latency_ms".to_string(), MetricValue::Number(87.3))]);

    project.store.set_track_status(track_id, TrackStatus::Completed).unwrap();
    assert_eq!(project.store.get_track(track_id).unwrap().status, TrackStatus::Completed);
}

#[test]
fn reopening_a_project_does_not_lose_data() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-reopen-test";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "reopen test").unwrap();
    }
    let project = Project::open(dir.path()).unwrap();
    assert_eq!(project.store.get_track(track_id).unwrap().hypothesis, "reopen test");
}

#[test]
fn rebuilds_from_prereg_files_if_duckdb_file_is_deleted() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-rebuild-test";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "rebuild test").unwrap();
        let track_dir = project.track_dir(track_id);
        write_prereg(&project.store, &track_dir, track_id, "rebuild test", "m", 1.0).unwrap();
    }

    std::fs::remove_file(dir.path().join(".zorp/zorp.duckdb")).unwrap();

    let project = Project::open(dir.path()).unwrap();
    let recovered = project.store.get_track(track_id).unwrap();
    assert_eq!(recovered.hypothesis, "rebuild test");
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());
}

#[test]
fn prereg_md_added_after_the_db_already_exists_is_indexed_on_next_open() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-added-later-test";

    // First open creates the DB. It has no knowledge of this track yet.
    {
        let project = Project::open(dir.path()).unwrap();
        assert!(project.store.get_track(track_id).is_err());
    }

    // Simulate a teammate's prereg.md landing on disk (e.g. via git
    // pull) after the DB already existed: write both the track and its
    // prereg row's file the same way write_prereg would, but do it
    // without going through this project's Store, the way a git pull
    // would just drop files on disk with no DB involved at all.
    let track_dir = dir.path().join(".zorp/tracks").join(track_id);
    std::fs::create_dir_all(&track_dir).unwrap();
    std::fs::write(
        track_dir.join("prereg.md"),
        format!(
            "# Pre-registration: {track_id}\n\nHypothesis: added later\nMetric: m\nKill threshold: 1\n"
        ),
    )
    .unwrap();

    // Reopening the same project, with the DB file still present, must
    // pick up and index the new prereg.md rather than only doing so
    // when the DB file was entirely absent.
    let project = Project::open(dir.path()).unwrap();
    let recovered = project.store.get_track(track_id).unwrap();
    assert_eq!(recovered.hypothesis, "added later");
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());
}

#[test]
fn project_open_recovers_from_a_corrupted_duckdb_file() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-corruption-test";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "corruption test").unwrap();
        let track_dir = project.track_dir(track_id);
        write_prereg(&project.store, &track_dir, track_id, "corruption test", "m", 1.0).unwrap();
    }

    // Corrupt the DuckDB file so a fresh Store::open on it fails outright.
    let db_path = dir.path().join(".zorp/zorp.duckdb");
    std::fs::write(&db_path, b"not a duckdb file, definitely corrupted").unwrap();

    // Project::open must recover: quarantine the bad file and start a
    // fresh store, which the unconditional rebuild then repopulates
    // from tracks/*/prereg.md, the source of truth.
    let project = Project::open(dir.path()).unwrap();
    let recovered = project.store.get_track(track_id).unwrap();
    assert_eq!(recovered.hypothesis, "corruption test");
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());

    // The corrupted file must not simply be deleted: it should still
    // exist somewhere under .zorp, quarantined under a different name.
    let zorp_dir = dir.path().join(".zorp");
    let quarantined_exists = std::fs::read_dir(&zorp_dir)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("corrupted"));
    assert!(quarantined_exists, "expected a quarantined copy of the corrupted db file");
}

#[test]
fn project_open_self_heals_a_track_with_a_half_written_preregistration() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let track_id = "2026-08-09-orphan-prereg-test";
    {
        let project = Project::open(dir.path()).unwrap();
        // Create the track row, but write prereg.md directly to disk
        // instead of going through `write_prereg`, so no
        // preregistrations row is ever inserted for it. This mirrors
        // `write_prereg` writing the file and then failing (e.g. a
        // failing git commit) before it can insert the row.
        project.store.create_track(track_id, "orphan prereg test").unwrap();
        let track_dir = project.track_dir(track_id);
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(
            track_dir.join("prereg.md"),
            "# Pre-registration: orphan\n\nHypothesis: orphan prereg test\nMetric: m\nKill threshold: 1\n",
        )
        .unwrap();
    }

    // `rebuild_from_prereg_files` runs unconditionally before the
    // integrity check on every `Project::open`, so this half-written
    // state self-heals rather than permanently locking the project out.
    let project = Project::open(dir.path()).unwrap();
    let track = project.store.get_track(track_id).unwrap();
    assert_eq!(track.hypothesis, "orphan prereg test");
    assert!(verify_prereg_integrity(&project.store, track_id).is_ok());
}

/// Reproduces DuckDB's real file lock: a second, concurrent
/// `Project::open` on the same path (here, a separate process, via the
/// `lock_hold_helper` test binary) must fail because DuckDB refuses a
/// second connection to a file another process already has open, but
/// must NOT be treated as corruption. Before the fix,
/// `open_store_recovering_from_corruption` matched on any `Store::open`
/// error when the db file exists, so this healthy, in-use database
/// would have been quarantined (renamed aside as "corrupted") and
/// silently replaced with an empty one, losing any experiments/metrics/
/// checkpoints data.
#[test]
fn project_open_does_not_quarantine_a_healthy_db_locked_by_another_process() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    // Open once to create .zorp/zorp.duckdb and populate it with data
    // that has no file-backed source of truth (so if this got
    // quarantined and rebuilt from prereg.md files, the loss would be
    // detectable).
    let track_id = "2026-08-09-lock-test";
    {
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track(track_id, "lock test").unwrap();
        let track_dir = project.track_dir(track_id);
        write_prereg(&project.store, &track_dir, track_id, "lock test", "m", 1.0).unwrap();
    }
    let db_path = dir.path().join(".zorp/zorp.duckdb");
    let original_contents = std::fs::read(&db_path).unwrap();

    // Hold DuckDB's file lock from a separate process.
    let helper_path = env!("CARGO_BIN_EXE_lock_hold_helper");
    let mut child = Command::new(helper_path)
        .arg(&db_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn lock_hold_helper");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "locked", "helper process did not report holding the lock");

    // A concurrent Project::open must fail (DuckDB genuinely cannot
    // open a second connection to a locked file) but must not quarantine
    // the file: the error is a lock error, not corruption.
    let result = Project::open(dir.path());
    assert!(result.is_err(), "expected Project::open to fail while the db file is locked");

    // Release the lock by closing the helper's stdin, which lets it exit.
    drop(child.stdin.take());
    let _ = child.wait();

    // The original db file must be untouched: same path, same bytes,
    // and no quarantined copy left behind.
    assert!(db_path.exists(), "the original zorp.duckdb must still exist at its original path");
    let contents_after = std::fs::read(&db_path).unwrap();
    assert_eq!(contents_after, original_contents, "the original zorp.duckdb must not have been rewritten");
    let zorp_dir = dir.path().join(".zorp");
    let quarantined_exists = std::fs::read_dir(&zorp_dir)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("corrupted"));
    assert!(!quarantined_exists, "a lock error must never quarantine the db file");

    // And a subsequent open, once the lock is released, must see the
    // original data intact.
    let project = Project::open(dir.path()).unwrap();
    let recovered = project.store.get_track(track_id).unwrap();
    assert_eq!(recovered.hypothesis, "lock test");
}

#[test]
fn checkpoint_hard_error_still_applies_inside_a_real_project() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    let project = Project::open(dir.path()).unwrap();
    let track_id = "2026-08-09-checkpoint-hard-error-test";
    project.store.create_track(track_id, "checkpoint hard error test").unwrap();

    // No auto-approve, and cargo test's stdin is never an interactive
    // terminal, so this must be a hard error even with a real project
    // and a real track behind it, not a silent skip.
    let result = CheckpointMode::terminal(false);
    assert!(matches!(result, Err(TrackError::CheckpointBlocked { .. })));

    // Auto-approve still works normally in the same project context.
    let mode = CheckpointMode::terminal(true).unwrap();
    let approved = project
        .store
        .record_checkpoint(track_id, "experiment", &mode, "proceed?")
        .unwrap();
    assert!(approved);
}
