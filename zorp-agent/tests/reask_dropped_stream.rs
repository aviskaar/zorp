//! A stream the provider dropped after deltas had reached the caller is asked
//! again by the loop, and never re-sent by the transport.
//!
//! The transport's rule is proved in `retry_rate_limit.rs`: once a delta has
//! been handed up the request is not sent again, because a second send would
//! replay the start of one answer over the middle of another. This file is
//! the layer above. The agent loop records nothing for a step whose reply
//! never finished, so it may discard the step and ask with a fresh request.
//! These tests run the real binary against the same stub to prove that it
//! does, that the bound holds, and that the dead step left nothing in the
//! transcript. Connections are counted rather than error strings matched,
//! for the reason `sse_stub` gives: a re-ask and a transport retry look the
//! same from the client side, and only the listener can tell them apart.

mod sse_stub;

use std::net::SocketAddr;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::Ordering;

use sse_stub::{scripted_server, Framing, Reply};

/// The event behind the log line that motivated this: OpenRouter's free
/// tier reporting a gateway timeout inside a 200 stream, 1618 events in.
/// Three of nine benchmark trials died to it after 5 to 35 tool calls.
const IDLE_TIMEOUT: &str =
    r#"{"choices":[],"error":{"code":504,"message":"Upstream idle timeout exceeded"}}"#;

/// One delta and then the drop. `after` must be at least one, or this is the
/// other case, the refusal the transport retries on its own.
const DROPPED: Reply = Reply::ErrorEvent {
    after: 1,
    event: IDLE_TIMEOUT,
};

fn run_agent(address: SocketAddr, dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zorp-agent"))
        .current_dir(dir)
        .args([
            "--yes",
            "--no-verify",
            "--base-url",
            &format!("http://{address}/v1"),
            "--model",
            "m",
            "say something",
        ])
        .env("ZORP_STATE_DB", dir.join("s.db"))
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .output()
        .unwrap()
}

/// What the run left in the transcript. A dead step must leave nothing: an
/// assistant message with half an answer would be sent back to the provider
/// on the next step as something the model said.
fn assistant_messages(dir: &Path) -> i64 {
    let conn = rusqlite::Connection::open(dir.join("s.db")).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE role = 'assistant'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

/// The measured case: one drop after real deltas, then a provider that
/// answers. The run ends with the answer, the stub saw one fresh request and
/// not a replay, and the transcript holds the one reply that finished.
#[test]
fn a_stream_dropped_after_delivery_is_asked_again_and_the_run_ends_with_the_answer() {
    static SCRIPT: &[Reply] = &[DROPPED, Reply::Finished { events: 3 }];
    for framing in Framing::BOTH {
        let dir = tempfile::tempdir().unwrap();
        let (address, connections) = scripted_server(framing, SCRIPT);

        let out = run_agent(address, dir.path());

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "{framing:?}: one dropped stream killed the run: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "012\n",
            "{framing:?}: the run did not end with the second answer"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: expected the dead request and one fresh one"
        );
        assert_eq!(
            assistant_messages(dir.path()),
            1,
            "{framing:?}: the dead step left something in the transcript"
        );
        assert!(
            stderr.contains("asking again (re-ask 1 of 2)"),
            "{framing:?}: the re-ask was silent: {stderr}"
        );
    }
}

/// The bound. A provider that drops every stream gets the first ask and two
/// more, then the run ends with an error that names the bound. Three
/// connections says the transport never re-sent inside any of them.
#[test]
fn a_provider_that_drops_every_stream_is_given_up_on_after_the_bound() {
    static SCRIPT: &[Reply] = &[DROPPED];
    for framing in Framing::BOTH {
        let dir = tempfile::tempdir().unwrap();
        let (address, connections) = scripted_server(framing, SCRIPT);

        let out = run_agent(address, dir.path());

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "{framing:?}: three dropped streams came back as a success"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            3,
            "{framing:?}: expected the first ask and two re-asks, no more and no less"
        );
        assert!(
            stderr.contains("asking again (re-ask 2 of 2)"),
            "{framing:?}: the second re-ask was silent: {stderr}"
        );
        assert!(
            stderr.contains("REASKS_PER_STEP") && stderr.contains("Upstream idle timeout exceeded"),
            "{framing:?}: the error does not name the bound and what the provider said: {stderr}"
        );
        assert_eq!(
            assistant_messages(dir.path()),
            0,
            "{framing:?}: a dead step left something in the transcript"
        );
    }
}
