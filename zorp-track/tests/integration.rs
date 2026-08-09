use std::path::Path;
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
