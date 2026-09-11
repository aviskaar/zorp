//! Searching your own conversations, from a terminal.
//!
//! The index is over the store both surfaces share, so it already covered
//! conversations held in the terminal. The terminal just could not read it:
//! `zorp-agent` did not depend on `zorp-recall` at all.
//!
//! The rule that holds the whole thing up is enforced in `zorp-recall` and
//! proved there by counting connections to a canary rather than by checking
//! for an error, because a failed request and a request never made look the
//! same from the caller's side. What is tested here is that the CLI does
//! not weaken it and does not fall back to anything.

use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

fn run(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(bin());
    command.args(args).env_remove("ZORP_API_KEY");
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

/// There is one chunker and one fingerprint in the workspace, and this
/// fails if a second appears. Two of either means the index silently holds
/// two conventions the day they disagree, which is the whole reason this
/// was a lift rather than a copy.
#[test]
fn there_is_one_chunker_and_one_fingerprint() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root");

    let mut chunkers = Vec::new();
    let mut fingerprints = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(name.as_ref(), "target" | ".git" | "node_modules") {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            // Source only. A test that names these functions to check there
            // is one of them is not a second one of them.
            if relative.contains("/tests/") {
                continue;
            }
            if text.contains("fn chunks_for(") {
                chunkers.push(relative.clone());
            }
            // The one that *computes* a fingerprint, not the index's
            // reader of the same name, which takes a conversation id and
            // answers what was stored.
            if text.contains("fn fingerprint(title:") {
                fingerprints.push(relative);
            }
        }
    }

    assert_eq!(
        chunkers,
        vec!["zorp-agent/src/recall.rs".to_string()],
        "there is more than one chunker"
    );
    assert_eq!(
        fingerprints,
        vec!["zorp-agent/src/recall.rs".to_string()],
        "there is more than one fingerprint"
    );
}

/// **Conversation text goes to a loopback address or it goes nowhere.**
/// Naming a remote host gets a refusal that names it, from the CLI as well
/// as the browser, and the CLI does not fall back to anything.
#[cfg(feature = "recall")]
#[test]
fn a_remote_embed_url_is_refused_and_the_refusal_names_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &["recall", "anything"],
        &[
            ("ZORP_EMBED_URL", "http://evil.example.com:11434"),
            ("ZORP_STATE_DB", dir.path().join("s.db").to_str().unwrap()),
            ("ZORP_RECALL_DB", dir.path().join("r.db").to_str().unwrap()),
        ],
    );
    let text = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success(), "a remote host was accepted");
    assert!(text.contains("evil.example.com"), "{text}");
}

/// A CLI that cannot reach the local embedder says so and searches nothing.
#[cfg(feature = "recall")]
#[test]
fn no_local_embedder_is_an_explicit_refusal_and_not_an_empty_result() {
    let dir = tempfile::tempdir().unwrap();
    // A loopback port with nothing on it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let out = run(
        &["recall", "anything"],
        &[
            ("ZORP_EMBED_URL", &format!("http://{addr}")),
            ("ZORP_STATE_DB", dir.path().join("s.db").to_str().unwrap()),
            ("ZORP_RECALL_DB", dir.path().join("r.db").to_str().unwrap()),
        ],
    );

    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    // Its own words, which already name the missing local model.
    assert!(!text.trim().is_empty(), "the refusal said nothing");
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "a failed search printed results"
    );
}

/// An empty query is a usage error rather than a search for nothing.
#[cfg(feature = "recall")]
#[test]
fn an_empty_query_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &["recall"],
        &[
            ("ZORP_STATE_DB", dir.path().join("s.db").to_str().unwrap()),
            ("ZORP_RECALL_DB", dir.path().join("r.db").to_str().unwrap()),
        ],
    );

    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nothing to search for"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Built without the feature, the subcommand is simply not there, and the
/// crate has no `zorp-recall` in its dependency tree. The second half is
/// checked by `cargo tree` in CI; this is the observable half.
///
/// It asks the help rather than running `recall anything`, because there is
/// no subcommand to be unrecognized: a word clap does not know is the start
/// of a prompt, so that would send a turn to whatever endpoint the machine
/// happens to have. Green on a developer running Ollama and red on a runner
/// that is not, which is the wrong way round for a test about a feature
/// nobody compiled in.
#[cfg(not(feature = "recall"))]
#[test]
fn without_the_feature_there_is_no_recall_subcommand() {
    let out = run(&["--help"], &[]);
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(!help.contains("\n  recall"), "{help}");
}
