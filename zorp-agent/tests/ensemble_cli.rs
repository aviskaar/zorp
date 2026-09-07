//! `zorp-agent ensemble` against the scripted provider: the roles file is
//! read, the main model and each reviewer reach the endpoint, reviewer
//! transcripts and the record land in the log directory, and the count of
//! connections is what the loop says it made. Counted, not matched, for
//! the reason `sse_stub` gives.

mod sse_stub;

use std::net::SocketAddr;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use sse_stub::{scripted_server, Ending, Framing, Reply};

/// A finished answer with no tool calls. The stub frames it; the content
/// is what `parse_verdict` will read.
fn answer(text: &str) -> Reply {
    let payload = serde_json::json!({
        "choices": [{"delta": {"content": text}, "finish_reason": "stop"}]
    })
    .to_string();
    Reply::Scripted {
        events: Arc::from(vec![payload]),
        ending: Ending::Done,
    }
}

fn run_ensemble(address: SocketAddr, dir: &Path) -> Output {
    let roster = dir.join("roster.toml");
    std::fs::write(
        &roster,
        "rounds = 1\n[main]\nmodel = \"m\"\n[[reviewer]]\nmodel = \"r0\"\n",
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_zorp-agent"))
        .current_dir(dir)
        .args([
            "ensemble",
            "--yes",
            "--no-verify",
            "--base-url",
            &format!("http://{address}/v1"),
            "say something",
        ])
        .env("ZORP_STATE_DB", dir.join("s.db"))
        .env("ZORP_ENSEMBLE", &roster)
        .env("ZORP_ENSEMBLE_LOG_DIR", dir.join("log"))
        .env(zorp::RETRY_ATTEMPTS_VAR, "1")
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .env_remove("ZORP_MODEL")
        .output()
        .unwrap()
}

#[test]
fn the_subcommand_runs_main_then_each_reviewer_and_writes_the_record() {
    // Main answers "done", the reviewer answers an empty verdict, and the
    // script repeats its last entry for anything past the end.
    let (address, connections) = scripted_server(
        Framing::Chunked,
        vec![answer("done"), answer("```json\n{\"findings\":[]}\n```")],
    );
    let dir = tempfile::tempdir().unwrap();
    let out = run_ensemble(address, dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(connections.load(Ordering::SeqCst), 2, "{stderr}");
    let record: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("log").join("ensemble.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["roster"]["main"], "m");
    assert_eq!(record["stopped"], "nothing corroborated");
    assert_eq!(record["requests"]["main"], 1);
    assert!(dir
        .path()
        .join("log")
        .join("reviewer-0-contract-round-1.txt")
        .is_file());
}

#[test]
fn a_missing_roster_is_a_configuration_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_zorp-agent"))
        .current_dir(dir.path())
        .args(["ensemble", "--yes", "say something"])
        .env("ZORP_STATE_DB", dir.path().join("s.db"))
        .env_remove("ZORP_ENSEMBLE")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ZORP_ENSEMBLE"));
}
