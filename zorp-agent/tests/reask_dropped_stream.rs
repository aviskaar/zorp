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
//!
//! The second half of the file is the same drop seen from our side of the
//! socket: the peer closes the connection instead of writing an error event
//! first, which rustls reports as "peer closed connection without sending
//! TLS close_notify" and a plain socket as a reset. After a delta it is the
//! same re-ask. Before a reply it is the transport's retry under the one
//! bound, and that is proved here rather than in `retry_rate_limit.rs`
//! because the retry line lands on the binary's stderr, where a test can
//! read it. See `docs/DECISIONS.md` (2026-09-05).

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

/// `attempts` is the transport's bound, `ZORP_RETRY_ATTEMPTS`, set on the
/// child explicitly so the developer's own environment cannot change what a
/// connection count means.
fn run_agent(address: SocketAddr, dir: &Path, attempts: u32) -> Output {
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
        .env(zorp::RETRY_ATTEMPTS_VAR, attempts.to_string())
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

        let out = run_agent(address, dir.path(), zorp::DEFAULT_RETRY_ATTEMPTS);

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

        let out = run_agent(address, dir.path(), zorp::DEFAULT_RETRY_ATTEMPTS);

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

/* ---- the same drop, seen from our side of the socket ---- */

/// The 6 of 35 case: two deltas and then the peer closes the connection with
/// no handshake, after which a provider answers. Same treatment as the error
/// event above: the run ends with the answer, one fresh request and not a
/// replay, one finished reply in the transcript, and the re-ask said so.
#[test]
fn a_connection_the_peer_closed_after_delivery_is_asked_again_and_the_run_ends_with_the_answer() {
    static SCRIPT: &[Reply] = &[Reply::Reset { after: 2 }, Reply::Finished { events: 3 }];
    for framing in Framing::BOTH {
        let dir = tempfile::tempdir().unwrap();
        let (address, connections) = scripted_server(framing, SCRIPT);

        let out = run_agent(address, dir.path(), zorp::DEFAULT_RETRY_ATTEMPTS);

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "{framing:?}: one closed connection killed the run: {stderr}"
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

/// Before a reply nothing has reached anyone, so the same death is the
/// transport's to retry, under the bound it already has, and loudly. The
/// loop is not involved: no re-ask line, and the transport's own line
/// instead.
#[test]
fn a_connection_the_peer_closed_before_a_reply_is_sent_again() {
    static SCRIPT: &[Reply] = &[Reply::ResetBeforeHeaders, Reply::Finished { events: 3 }];
    for framing in Framing::BOTH {
        let dir = tempfile::tempdir().unwrap();
        let (address, connections) = scripted_server(framing, SCRIPT);

        let out = run_agent(address, dir.path(), zorp::DEFAULT_RETRY_ATTEMPTS);

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "{framing:?}: one connection reset before a reply killed the run: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "012\n",
            "{framing:?}: the run did not end with the answer"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: the request was not sent again"
        );
        assert!(
            stderr.contains(zorp::CONNECTION_DROPPED) && stderr.contains("sending again"),
            "{framing:?}: the retry was silent: {stderr}"
        );
        assert!(
            !stderr.contains("asking again"),
            "{framing:?}: a retry before any reply was reported as a re-ask: {stderr}"
        );
    }
}

/// The bound is the transport's bound. A peer that resets every connection
/// gets `ZORP_RETRY_ATTEMPTS` sends and then the run ends with an error that
/// names it. Nothing in the transcript, because nothing ever arrived.
#[test]
fn a_peer_that_resets_every_connection_is_given_up_on_after_the_bound() {
    const ATTEMPTS: u32 = 3;
    static SCRIPT: &[Reply] = &[Reply::ResetBeforeHeaders];
    for framing in Framing::BOTH {
        let dir = tempfile::tempdir().unwrap();
        let (address, connections) = scripted_server(framing, SCRIPT);

        let out = run_agent(address, dir.path(), ATTEMPTS);

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "{framing:?}: a peer that never answered came back as a success"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            ATTEMPTS as usize,
            "{framing:?}: the number of sends is not the bound"
        );
        assert!(
            stderr.contains(zorp::RETRY_ATTEMPTS_VAR) && stderr.contains(zorp::CONNECTION_DROPPED),
            "{framing:?}: the error does not name the bound and the reason: {stderr}"
        );
        assert_eq!(
            assistant_messages(dir.path()),
            0,
            "{framing:?}: a request that never got a reply left something in the transcript"
        );
    }
}
