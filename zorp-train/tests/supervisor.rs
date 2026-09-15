use std::fs;
use tempfile::tempdir;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::manifest::{TrainEvent, TrainingJobConfig};
use zorp_train::supervisor::TrainingSupervisor;

#[tokio::test]
async fn test_supervisor_channel() {
    let sup = TrainingSupervisor::new();
    let mut _rx = sup.subscribe();

    // Test parsing a JSON event string
    let json = r#"{"type":"step","step":10,"loss":3.2,"lr":0.0003,"tokens":10000,"tok_per_sec":5000.0,"memory_gb":12.5,"eta_seconds":120}"#;
    let event: TrainEvent = serde_json::from_str(json).unwrap();

    if let TrainEvent::Step { step, loss, .. } = event {
        assert_eq!(step, 10);
        assert_eq!(loss, 3.2);
    } else {
        panic!("unexpected event");
    }
}

#[test]
fn test_all_train_events_deserialization() {
    let init_json = r#"{"type":"init","parameters":250000000,"device":"Apple Metal","memory_total_gb":16.0}"#;
    let init_event: TrainEvent = serde_json::from_str(init_json).unwrap();
    assert_eq!(
        init_event,
        TrainEvent::Init {
            parameters: 250_000_000,
            device: "Apple Metal".to_string(),
            memory_total_gb: 16.0,
        }
    );

    let step_json = r#"{"type":"step","step":50,"loss":2.45,"lr":0.0005,"tokens":50000,"tok_per_sec":4200.5,"memory_gb":10.2,"eta_seconds":300}"#;
    let step_event: TrainEvent = serde_json::from_str(step_json).unwrap();
    assert_eq!(
        step_event,
        TrainEvent::Step {
            step: 50,
            loss: 2.45,
            lr: 0.0005,
            tokens: 50_000,
            tok_per_sec: 4200.5,
            memory_gb: 10.2,
            eta_seconds: 300,
        }
    );

    let step_zero_eta_json = r#"{"type":"step","step":100,"loss":1.50,"lr":0.0001,"tokens":100000,"tok_per_sec":5000.0,"memory_gb":10.2,"eta_seconds":0}"#;
    let step_zero_eta_event: TrainEvent = serde_json::from_str(step_zero_eta_json).unwrap();
    assert_eq!(
        step_zero_eta_event,
        TrainEvent::Step {
            step: 100,
            loss: 1.50,
            lr: 0.0001,
            tokens: 100_000,
            tok_per_sec: 5000.0,
            memory_gb: 10.2,
            eta_seconds: 0,
        }
    );

    let sample_json = r#"{"type":"sample","step":100,"prompt":"Hello","output":" world"}"#;
    let sample_event: TrainEvent = serde_json::from_str(sample_json).unwrap();
    assert_eq!(
        sample_event,
        TrainEvent::Sample {
            step: 100,
            prompt: "Hello".to_string(),
            output: " world".to_string(),
        }
    );

    let checkpoint_json = r#"{"type":"checkpoint","step":500,"loss":1.85,"path":"/tmp/checkpoints/step_500"}"#;
    let checkpoint_event: TrainEvent = serde_json::from_str(checkpoint_json).unwrap();
    assert_eq!(
        checkpoint_event,
        TrainEvent::Checkpoint {
            step: 500,
            loss: 1.85,
            path: "/tmp/checkpoints/step_500".to_string(),
        }
    );

    let error_json = r#"{"type":"error","message":"Out of memory on Metal device"}"#;
    let error_event: TrainEvent = serde_json::from_str(error_json).unwrap();
    assert_eq!(
        error_event,
        TrainEvent::Error {
            message: "Out of memory on Metal device".to_string(),
        }
    );
}

#[tokio::test]
async fn test_supervisor_idle_lifecycle() {
    let sup = TrainingSupervisor::new();
    assert!(!sup.is_running());
    assert_eq!(sup.active_pid(), None);

    assert!(sup.pause().is_err());
    assert!(sup.resume().is_err());
    assert!(sup.stop().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn test_supervisor_spawn_lifecycle_and_events() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("mock-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let py_path = bin_dir.join("python3");
    // Mock python script that outputs init, step, sample, and sleeps
    let mock_py = r#"#!/usr/bin/env python3
import sys
import time

print('{"type":"init","parameters":1000,"device":"Apple Metal","memory_total_gb":8.0}', flush=True)
time.sleep(0.05)
print('{"type":"step","step":1,"loss":4.2,"lr":0.001,"tokens":128,"tok_per_sec":100.0,"memory_gb":1.0,"eta_seconds":10}', flush=True)
time.sleep(0.05)
print('{"type":"sample","step":1,"prompt":"test","output":"out"}', flush=True)

# Loop to test pause/resume/stop
for _ in range(50):
    time.sleep(0.1)
"#;
    fs::write(&py_path, mock_py).unwrap();
    let mut perms = fs::metadata(&py_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py_path, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);
    let sup = TrainingSupervisor::new();
    let mut rx = sup.subscribe();

    let config = TrainingJobConfig {
        run_id: "test-run-1".to_string(),
        dataset_id: "ds1".to_string(),
        tokenizer_name: "tok1".to_string(),
        recipe_name: "rec1".to_string(),
        batch_size: 4,
        gradient_accumulation_steps: 1,
        learning_rate: 0.001,
        warmup_steps: 10,
        max_tokens: 10000,
        checkpoint_every_steps: 100,
        sample_every_steps: 50,
    };

    let run_dir = tmp.path().join("run");
    let recipe = serde_json::json!({
        "hidden_size": 64,
        "vocab_size": 100,
        "num_attention_heads": 2,
        "num_key_value_heads": 2,
        "intermediate_size": 128,
        "num_hidden_layers": 2,
        "max_position_embeddings": 128
    });

    let handle = sup.start_job(&env, &config, &run_dir, recipe).await;
    assert!(handle.is_ok(), "start_job failed: {:?}", handle.err());
    let _join_handle = handle.unwrap();

    assert!(sup.is_running());
    assert!(sup.active_pid().is_some());

    // Verify events received over broadcast
    let ev1 = rx.recv().await.expect("receive init");
    assert!(matches!(ev1, TrainEvent::Init { .. }));

    let ev2 = rx.recv().await.expect("receive step");
    assert!(matches!(ev2, TrainEvent::Step { step: 1, .. }));

    let ev3 = rx.recv().await.expect("receive sample");
    assert!(matches!(ev3, TrainEvent::Sample { .. }));

    // Test pause
    assert!(sup.pause().is_ok());
    // Repeated pause should fail or be rejected
    assert!(sup.pause().is_err());

    // Test resume
    assert!(sup.resume().is_ok());
    // Repeated resume should fail or be rejected
    assert!(sup.resume().is_err());

    // Test stop
    assert!(sup.stop().is_ok());
    assert!(!sup.is_running());
}

#[cfg(unix)]
#[tokio::test]
async fn test_supervisor_child_abnormal_termination_emits_error() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("mock-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let py_path = bin_dir.join("python3");
    let mock_py = r#"#!/usr/bin/env python3
import sys
sys.stderr.write("Fatal crash in MLX: out of memory on Metal device\n")
sys.exit(1)
"#;
    fs::write(&py_path, mock_py).unwrap();
    let mut perms = fs::metadata(&py_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py_path, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);
    let sup = TrainingSupervisor::new();
    let mut rx = sup.subscribe();

    let config = TrainingJobConfig {
        run_id: "test-crash-run".to_string(),
        dataset_id: "ds1".to_string(),
        tokenizer_name: "tok1".to_string(),
        recipe_name: "rec1".to_string(),
        batch_size: 4,
        gradient_accumulation_steps: 1,
        learning_rate: 0.001,
        warmup_steps: 10,
        max_tokens: 10000,
        checkpoint_every_steps: 100,
        sample_every_steps: 50,
    };

    let run_dir = tmp.path().join("run");
    let recipe = serde_json::json!({
        "hidden_size": 64,
        "vocab_size": 100,
        "num_attention_heads": 2,
        "num_key_value_heads": 2,
        "intermediate_size": 128,
        "num_hidden_layers": 2,
        "max_position_embeddings": 128
    });

    let handle = sup.start_job(&env, &config, &run_dir, recipe).await.unwrap();

    // Verify error event is broadcast with exit code and stderr details
    let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for error event")
        .expect("channel receive failed");

    if let TrainEvent::Error { message } = ev {
        assert!(
            message.contains("code 1"),
            "expected exit code in error message: {message}"
        );
        assert!(
            message.contains("Fatal crash in MLX: out of memory on Metal device"),
            "expected stderr in error message: {message}"
        );
    } else {
        panic!("expected TrainEvent::Error, got: {ev:?}");
    }

    let _ = handle.await;
    assert!(!sup.is_running());
}

#[cfg(unix)]
#[tokio::test]
async fn test_supervisor_stop_while_paused() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("mock-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let py_path = bin_dir.join("python3");
    let mock_py = r#"#!/usr/bin/env python3
import time
while True:
    time.sleep(0.1)
"#;
    fs::write(&py_path, mock_py).unwrap();
    let mut perms = fs::metadata(&py_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py_path, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);
    let sup = TrainingSupervisor::new();

    let config = TrainingJobConfig {
        run_id: "test-pause-stop-run".to_string(),
        dataset_id: "ds1".to_string(),
        tokenizer_name: "tok1".to_string(),
        recipe_name: "rec1".to_string(),
        batch_size: 4,
        gradient_accumulation_steps: 1,
        learning_rate: 0.001,
        warmup_steps: 10,
        max_tokens: 10000,
        checkpoint_every_steps: 100,
        sample_every_steps: 50,
    };

    let run_dir = tmp.path().join("run");
    let recipe = serde_json::json!({
        "hidden_size": 64,
        "vocab_size": 100,
        "num_attention_heads": 2,
        "num_key_value_heads": 2,
        "intermediate_size": 128,
        "num_hidden_layers": 2,
        "max_position_embeddings": 128
    });

    let handle = sup.start_job(&env, &config, &run_dir, recipe).await.unwrap();
    assert!(sup.is_running());

    assert!(sup.pause().is_ok());
    assert!(sup.stop().is_ok());
    assert!(!sup.is_running());

    // Joining handle must not deadlock or hang because SIGCONT resumes child to handle SIGTERM
    let join_res = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
    assert!(join_res.is_ok(), "handle hung after stop while paused");
}

