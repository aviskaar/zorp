use tempfile::tempdir;
use zorp_train::environment::{EnvironmentStatus, TrainingEnvironment};

#[test]
fn test_uninitialized_environment_reports_missing() {
    let tmp = tempdir().unwrap();
    let env = TrainingEnvironment::new(tmp.path().join("training-env"));
    assert_eq!(env.status(), EnvironmentStatus::Missing);
}

#[test]
fn test_status_serde() {
    let json = serde_json::to_string(&EnvironmentStatus::Ready).unwrap();
    assert_eq!(json, "\"ready\"");
    let deserialized: EnvironmentStatus = serde_json::from_str("\"ready\"").unwrap();
    assert_eq!(deserialized, EnvironmentStatus::Ready);

    let json_missing = serde_json::to_string(&EnvironmentStatus::Missing).unwrap();
    assert_eq!(json_missing, "\"missing\"");
}

#[test]
fn test_python_path() {
    let env = TrainingEnvironment::new(std::path::PathBuf::from("/tmp/my-env"));
    assert_eq!(
        env.python_path(),
        std::path::PathBuf::from("/tmp/my-env/bin/python3")
    );
}

#[test]
fn test_default_dir_custom() {
    std::env::set_var("ZORP_TRAINING_ENV_DIR", "/custom/training/env");
    assert_eq!(
        TrainingEnvironment::default_dir(),
        std::path::PathBuf::from("/custom/training/env")
    );
    std::env::remove_var("ZORP_TRAINING_ENV_DIR");
}

#[cfg(unix)]
#[test]
fn test_corrupted_environment_reports_corrupted() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("training-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let py = bin_dir.join("python3");
    fs::write(&py, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = fs::metadata(&py).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);
    assert_eq!(env.status(), EnvironmentStatus::Corrupted);
}
