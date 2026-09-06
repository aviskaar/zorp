//! The scripted provider these tests run against, plus the one helper that
//! cannot live with it.
//!
//! The listener itself moved to the `zorp-stub` crate when `zorp-eval`'s
//! deterministic harness suite needed the same thing. Two copies of a socket
//! that lies about being a provider is exactly the drift the stub was
//! written to avoid, so there is one, and this module re-exports it under
//! the name these tests already use.
//!
//! What stays here is `stream_with_patience`, which calls
//! `zorp_agent::streaming::stream_sse` in process. `zorp-stub` depends on
//! nothing in the workspace and is not about to start.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;
use zorp_agent::streaming::{stream_sse, StreamOutcome};

pub use zorp_stub::*;

/// What one call to `stream_sse` did, flattened so it can cross a channel.
pub struct Run {
    pub streamed: bool,
    pub error: Option<String>,
    pub payloads: usize,
    pub elapsed: Duration,
}

/// Run `stream_sse` on its own thread and refuse to wait forever for it.
///
/// The thing under test is whether a call can hang, and a test that hangs to
/// prove a hang reports nothing at all: it just stops, and someone has to go
/// and find out why. Bounding the wait here turns that into a failed
/// assertion with a message.
pub fn stream_with_patience(address: SocketAddr, what: &str) -> Run {
    let url = format!("http://{address}/v1/chat/completions");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut payloads = 0usize;
        let outcome = stream_sse(&url, &[], json!({"stream": true}), None, &mut |_| {
            payloads += 1;
        });
        let _ = tx.send(Run {
            streamed: matches!(outcome, Ok(StreamOutcome::Streamed)),
            error: outcome.err().map(|e| e.to_string()),
            payloads,
            elapsed: started.elapsed(),
        });
    });
    rx.recv_timeout(PATIENCE)
        .unwrap_or_else(|_| panic!("{what}: stream_sse was still blocked after {PATIENCE:?}"))
}
