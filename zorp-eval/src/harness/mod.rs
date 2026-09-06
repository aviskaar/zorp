//! The deterministic half of `zorp-eval`.
//!
//! `compat` spawns the agent against a live provider and grades what it left
//! behind, which is the right shape for a question about models and the
//! wrong shape for a gate: a provider outage fails it for reasons that have
//! nothing to do with the code, and a recent nine task run lost four tasks
//! to 404s from upstream. This half answers a different question. It runs
//! the real `zorp-agent` binary against a scripted provider on loopback, so
//! a case is a fixed conversation with fixed transport behaviour, and the
//! only thing that can fail it is the code.
//!
//! The binary is driven rather than the library on purpose. `agent.rs` has
//! about eighty in-process tests driving a scripted `Model` through the run
//! loop, and they already cover tool calls, approval, compaction and
//! termination. What they cannot reach is everything below the `Model`
//! trait: the HTTP client, the streaming parser, the retry bound, the read
//! timeout, and the store on disk when the process is gone. That is where
//! the expensive bugs were, so that is the layer this suite occupies.

pub mod case;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use case::{Case, Exit};
use zorp_stub::{scripted_server, Framing};

/// A ceiling on one case, so a bug that hangs is a failed case and not a
/// six hour continuous integration job. Nothing is tuned to it: a case that
/// needs to wait states its own bound through `ZORP_HTTP_TIMEOUT_SECS`, and
/// this is far above any of them.
const CEILING: Duration = Duration::from_secs(120);

/// How many steps a run gets unless the case says otherwise. A script's last
/// reply answers every further request, so a script ending in a tool call
/// would loop until something stopped it.
const DEFAULT_MAX_STEPS: &str = "8";

/// The framing every case is served in.
///
/// Chunked, because that is what an OpenAI-compatible endpoint behind a CDN
/// sends and it is the one that hides a read timeout inside "Error while
/// decoding chunks". Coverage of both framings belongs to
/// `zorp-agent/tests/retry_rate_limit.rs` and `streaming_timeout.rs`, which
/// run every promise they make against both; repeating that here would
/// double the suite to re-prove someone else's point.
const FRAMING: Framing = Framing::Chunked;

pub struct CaseResult {
    pub name: String,
    /// Empty when the case passed. One entry per expectation that did not
    /// hold, each saying what was expected and what happened.
    pub failures: Vec<String>,
    pub elapsed: Duration,
}

/// Run every `.toml` case in `dir`, print one line each, and report whether
/// they all passed.
pub fn run_suite(dir: &Path, agent_binary: &Path) -> anyhow::Result<bool> {
    let agent_binary = agent_binary.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "{}: {e} (build it first: cargo build -p zorp-agent)",
            agent_binary.display()
        )
    })?;

    let mut cases: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    cases.sort();
    if cases.is_empty() {
        anyhow::bail!("no .toml cases in {}", dir.display());
    }

    println!("harness: {} cases from {}", cases.len(), dir.display());
    // One at a time. Each case already gets its own port, workspace and
    // store, so running them at once would be sound, but the suite takes
    // seconds and serial output is readable.
    let mut passed = 0usize;
    let mut results = Vec::new();
    for path in &cases {
        let result = run_case(path, &agent_binary);
        if result.failures.is_empty() {
            passed += 1;
            println!(
                "  pass  {}  ({:.1}s)",
                result.name,
                result.elapsed.as_secs_f64()
            );
        } else {
            println!(
                "  FAIL  {}  ({:.1}s)",
                result.name,
                result.elapsed.as_secs_f64()
            );
            for failure in &result.failures {
                for line in failure.lines() {
                    println!("          {line}");
                }
            }
        }
        results.push(result);
    }
    println!(
        "{} cases: {passed} passed, {} failed",
        results.len(),
        results.len() - passed
    );
    Ok(passed == results.len())
}

/// Run one case in its own temporary directory, against its own stub.
pub fn run_case(path: &Path, agent_binary: &Path) -> CaseResult {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let started = Instant::now();
    let (name, failures) = match case::load(path) {
        Err(e) => (name, vec![e.to_string()]),
        Ok(case) => {
            let name = case.name.clone().unwrap_or(name);
            match run_case_inner(path, &case, agent_binary) {
                Ok(failures) => (name, failures),
                Err(e) => (name, vec![e.to_string()]),
            }
        }
    };
    CaseResult {
        name,
        failures,
        elapsed: started.elapsed(),
    }
}

fn run_case_inner(path: &Path, case: &Case, agent_binary: &Path) -> anyhow::Result<Vec<String>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let home = root.path().join("home");
    std::fs::create_dir_all(&home)?;
    let state_db = root.path().join("state.db");

    if let Some(fixture) = &case.agent.fixture {
        let from = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(fixture);
        crate::snapshot::snapshot_copy(&from, &workspace)
            .map_err(|e| anyhow::anyhow!("fixture {}: {e}", from.display()))?;
    }

    let (address, connections) = scripted_server(FRAMING, case.script());

    let mut command = Command::new(agent_binary);
    command
        .current_dir(&workspace)
        .args([
            "--yes",
            "--no-verify",
            "--base-url",
            &format!("http://{address}/v1"),
            "--model",
            "harness-stub",
            &case.agent.prompt,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Every ZORP_ variable the developer happens to have set is cleared
    // before the case sets its own. A connection count means nothing if the
    // machine gets to choose the retry bound.
    for (key, _) in std::env::vars() {
        if key.starts_with("ZORP_") {
            command.env_remove(&key);
        }
    }
    command
        .env("HOME", &home)
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("ZORP_STATE_DB", &state_db)
        .env("ZORP_TRUST_FILE", root.path().join("trust"))
        .env("ZORP_MAX_STEPS", DEFAULT_MAX_STEPS);
    for (key, value) in &case.agent.env {
        command.env(key, value);
    }

    let output = wait_with_ceiling(command)?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let mut failures = Vec::new();

    if let Some(exit) = case.expect.exit {
        let succeeded = output.status.success();
        if succeeded != (exit == Exit::Success) {
            failures.push(format!(
                "exit: expected {}, got {} ({})\n  stderr: {}",
                if exit == Exit::Success {
                    "success"
                } else {
                    "failure"
                },
                if succeeded { "success" } else { "failure" },
                output.status,
                tail(&stderr),
            ));
        }
    }

    if let Some(want) = case.expect.connections {
        let got = connections.load(Ordering::SeqCst);
        if got != want {
            failures.push(format!(
                "connections: expected {want}, got {got}\n  stderr: {}",
                tail(&stderr)
            ));
        }
    }

    for file in &case.expect.files {
        let full = workspace.join(&file.path);
        let found = std::fs::read_to_string(&full).ok();
        match (&found, file.absent) {
            (Some(_), true) => failures.push(format!("{}: expected it not to exist", file.path)),
            (None, false) => failures.push(format!(
                "{}: expected it to exist, workspace holds {}",
                file.path,
                listing(&workspace)
            )),
            _ => {}
        }
        let Some(found) = found else { continue };
        if let Some(want) = &file.contents {
            if &found != want {
                failures.push(format!(
                    "{}: expected exactly {want:?}, got {found:?}",
                    file.path
                ));
            }
        }
        if let Some(want) = &file.contains {
            if !found.contains(want) {
                failures.push(format!(
                    "{}: expected it to contain {want:?}, got {found:?}",
                    file.path
                ));
            }
        }
    }

    if let Some(want) = &case.expect.transcript_roles {
        match transcript_roles(&state_db) {
            Ok(got) if &got != want => {
                failures.push(format!("transcript roles: expected {want:?}, got {got:?}"))
            }
            Err(e) => failures.push(format!("transcript roles: could not read the store: {e}")),
            _ => {}
        }
    }

    Ok(failures)
}

/// Run the child and refuse to wait forever for it. The wait ends on the
/// child's own exit, not on a poll, so nothing here is tuned to how fast a
/// machine is.
fn wait_with_ceiling(mut command: Command) -> anyhow::Result<std::process::Output> {
    let mut child = command.spawn()?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut out = Vec::new();
        let mut err = Vec::new();
        // The pipes are read on this thread too, so a child that fills one
        // cannot block waiting for someone to drain it.
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut out);
        }
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut err);
        }
        let _ = tx.send((out, err));
    });
    let piped = rx.recv_timeout(CEILING);
    if piped.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let (out, err) = match piped {
        Ok(piped) => piped,
        Err(_) => {
            let _ = reader.join();
            anyhow::bail!("the run was still going after {CEILING:?} and was killed");
        }
    };
    let _ = reader.join();
    Ok(std::process::Output {
        status,
        stdout: out,
        stderr: err,
    })
}

/// The `role` column of the stored transcript, in `seq` order.
fn transcript_roles(state_db: &Path) -> anyhow::Result<Vec<String>> {
    let conn = rusqlite::Connection::open(state_db)?;
    let mut statement = conn.prepare("SELECT role FROM messages ORDER BY seq, id")?;
    let roles = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(roles)
}

/// What the workspace holds, for a failure that says a file is missing.
fn listing(workspace: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(workspace)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if names.is_empty() {
        "nothing".to_string()
    } else {
        names.join(", ")
    }
}

/// The last few lines of the child's stderr. A failure has to say what
/// happened, and the interesting part of a run that died is at the end.
fn tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runner refuses a directory with nothing in it rather than
    /// reporting a green suite that ran no cases. An eval that exits zero
    /// having checked nothing is the one failure mode worth refusing.
    #[test]
    fn an_empty_case_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("agent");
        std::fs::write(&binary, "").unwrap();
        let error = run_suite(dir.path(), &binary).unwrap_err().to_string();
        assert!(error.contains("no .toml cases"), "{error}");
    }

    #[test]
    fn a_missing_agent_binary_says_how_to_build_it() {
        let dir = tempfile::tempdir().unwrap();
        let error = run_suite(dir.path(), &dir.path().join("nope"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("cargo build -p zorp-agent"), "{error}");
    }
}
