use std::path::PathBuf;
use std::process::Command;
use zorp_eval::{contracts, snapshot};

fn agent_binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // workspace root
    path.push("target/release/zorp-agent");
    path
}

// This test drives a real zorp-agent run and needs a release build of the
// agent plus working model credentials. It is ignored by default and must
// be run explicitly:
//
//     cargo build --release -p zorp-agent
//     cargo test -p zorp-eval --test instrumentation_validation -- --ignored
//
// When run, any missing prerequisite is a hard failure, not a silent pass.
#[test]
#[ignore = "requires a release zorp-agent build and model credentials; run explicitly with --ignored"]
fn instrumentation_validation_gate() {
    let agent = agent_binary();
    assert!(
        agent.exists(),
        "{} not built. Run `cargo build --release -p zorp-agent` first.",
        agent.display()
    );

    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "hello").unwrap();

    let before_hash = snapshot::snapshot_hash(workspace.path()).unwrap();
    let backup = tempfile::tempdir().unwrap();
    snapshot::snapshot_copy(workspace.path(), &backup.path().join("snap")).unwrap();

    let trace_path = workspace.path().join("trace.jsonl");
    let status = Command::new(&agent)
        .current_dir(workspace.path())
        .arg("--yes")
        .arg("append 'world' to notes.txt, then run `cat notes.txt` to confirm, then finish")
        .env("ZORP_TRACE_FILE", &trace_path)
        .env("ZORP_EXPERIMENT_ID", "validation")
        .env("ZORP_TASK_ID", "notes-append")
        .env("ZORP_RUNTIME_ID", "reference-high")
        .env("ZORP_RUN_ID", "validation-run-0")
        .env("ZORP_REPETITION", "0")
        .env("ZORP_REASONING_MODE", "high")
        .env("ZORP_MODEL", "qwen3.6:35b-mlx")
        .status()
        .expect("could not spawn zorp-agent");

    assert!(
        status.success(),
        "zorp-agent exited non-zero (check model credentials): {status}"
    );

    let trace = contracts::load_trace(&trace_path).unwrap();
    assert_eq!(trace.malformed_lines, 0, "trace contains malformed lines");
    let events = &trace.events;
    let has = |t: &str| {
        events
            .iter()
            .any(|e| e.get("event_type").and_then(|v| v.as_str()) == Some(t))
    };
    assert!(has("run.start"), "missing run.start");
    assert!(has("run.end"), "missing run.end");
    assert!(has("termination"), "missing termination");
    assert!(has("tool.call"), "missing tool.call");
    assert!(has("tool.result"), "missing tool.result");

    snapshot::restore(&backup.path().join("snap"), workspace.path()).unwrap();
    let after_hash = snapshot::snapshot_hash(workspace.path()).unwrap();
    assert_eq!(
        before_hash, after_hash,
        "snapshot restore did not reproduce identical hash"
    );
}
