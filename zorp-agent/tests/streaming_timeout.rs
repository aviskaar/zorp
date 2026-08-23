//! The streaming path is bounded, and it is bounded by silence rather than by
//! length.
//!
//! Its own test binary, not a module inside `streaming.rs`, for one reason:
//! the read timeout is read once, when the shared agent is first built, so a
//! test that wants a short one has to set it before anything else in the
//! process has made an HTTP call. A separate binary is a separate process,
//! which is the only way to promise that without ordering the whole suite.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::Once;
use std::time::{Duration, Instant};

use serde_json::json;
use zorp_agent::streaming::{stream_sse, StreamOutcome};

/// Short enough to keep the suite quick, long enough that a healthy stream
/// pausing for [`GAP`] between chunks is nowhere near it.
const IDLE_TIMEOUT_SECS: u64 = 3;

/// The pause a healthy stream takes between chunks, and the whole point of
/// the second test: well inside the timeout, repeated until the response as a
/// whole has outlasted it.
const GAP: Duration = Duration::from_millis(400);

/// How many of those chunks. Ten of them is four seconds, so the response
/// lives longer than the idle timeout without ever going quiet for as long.
const PIECES: usize = 10;

/// How long this test will wait before it calls something a hang. Clear of
/// the idle timeout so a busy machine cannot fail it on timing alone, and
/// nothing like the three hours the wedged run actually sat there.
const PATIENCE: Duration = Duration::from_secs(30);

static ENV: Once = Once::new();

/// Set the timeout before anything builds the shared agent, which caches it
/// on first use. `Once` because both tests need it and the harness runs them
/// on parallel threads.
fn bound_the_wait() {
    ENV.call_once(|| {
        std::env::set_var(zorp::READ_TIMEOUT_VAR, IDLE_TIMEOUT_SECS.to_string());
    });
}

/// Read the whole request, headers and body, before answering.
///
/// Not because the request is interesting, but because closing a socket that
/// still has unread bytes on it sends RST rather than FIN, and the client then
/// reports a connection reset instead of whatever the test is about.
fn drain_request(stream: &mut TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request ended before headers");
        request.extend_from_slice(&buffer[..read]);
        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|l| l.strip_prefix("Content-Length: "))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    while request.len() < header_end + length {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request ended before body");
        request.extend_from_slice(&buffer[..read]);
    }
}

/// Accept one connection, answer with event-stream headers, then hand the
/// socket to `body` to behave however the test needs.
///
/// Port 0, so the OS picks. The test names no port and cannot collide with
/// anything already listening on this machine.
fn sse_server(body: impl FnOnce(TcpStream) + Send + 'static) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        drain_request(&mut stream);
        // No Content-Length and no chunked framing, so the body runs until the
        // socket closes. That is the shape a real SSE response has, and it is
        // the shape whose reads block.
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        let _ = stream.flush();
        body(stream);
    });
    address
}

/// What one call to `stream_sse` did, flattened so it can cross a channel.
struct Run {
    streamed: bool,
    error: Option<String>,
    payloads: usize,
    elapsed: Duration,
}

/// Run `stream_sse` on its own thread and refuse to wait forever for it.
///
/// The thing under test is whether a call can hang, and a test that hangs to
/// prove a hang reports nothing at all: it just stops, and someone has to go
/// and find out why. Bounding the wait here turns the old behavior into a
/// failed assertion with a message.
fn stream_with_patience(address: SocketAddr, what: &str) -> Run {
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

/// The bug this file exists for.
///
/// A provider that accepts the request, sends headers and then goes quiet was
/// waited on forever, because the streaming path built its own `ureq::agent()`
/// with no timeouts while every buffered call went through the configured one.
/// Measured in the wild at 3 hours 18 minutes of 0% CPU on an established
/// socket, which is where a 200 sample calibration run stopped at 123.
#[test]
fn a_provider_that_goes_quiet_ends_the_stream_instead_of_being_waited_on() {
    bound_the_wait();
    // Headers, then nothing, for longer than this test will wait. Holding the
    // socket open is the point: closing it would be a clean end of response,
    // which is not the failure being reproduced.
    let address = sse_server(|_socket| std::thread::sleep(PATIENCE * 4));

    let run = stream_with_patience(address, "a provider that went quiet");

    let error = run
        .error
        .expect("a silent socket came back as a finished answer");
    assert!(
        error.contains(zorp::READ_TIMEOUT_VAR),
        "the error does not say what ran out or how to buy more of it: {error}"
    );
    // Not instant, or the error came from something other than the timeout. A
    // refused connection looks identical from here.
    assert!(
        run.elapsed + Duration::from_millis(250) >= Duration::from_secs(IDLE_TIMEOUT_SECS),
        "gave up after {:?}, too soon to have been the idle timeout",
        run.elapsed
    );
}

/// The other half of the promise: the bound is on silence, not on length.
///
/// A total-request timeout would kill exactly the answers zorp is for, the
/// long ones, and it would do it after the user had already watched most of
/// one arrive. This response runs for longer than the timeout and never once
/// pauses for as long as it.
#[test]
fn a_slow_but_talking_provider_is_not_cut_off() {
    bound_the_wait();
    let address = sse_server(|mut socket| {
        for i in 0..PIECES {
            let piece = json!({"choices": [{"delta": {"content": i}}]});
            // A client that hung up ends this stub rather than failing it.
            if write!(socket, "data: {piece}\n\n").is_err() || socket.flush().is_err() {
                return;
            }
            std::thread::sleep(GAP);
        }
    });

    let run = stream_with_patience(address, "a slow but talking provider");

    assert!(
        run.error.is_none(),
        "a healthy stream was cut off: {:?}",
        run.error
    );
    assert!(run.streamed, "the event stream was not read as one");
    assert_eq!(
        run.payloads, PIECES,
        "the reader stopped short of what the server said"
    );
    // The response outlived the idle timeout, so passing this cannot mean it
    // simply finished before the bound had a chance to apply.
    assert!(
        run.elapsed > Duration::from_secs(IDLE_TIMEOUT_SECS),
        "the stream finished in {:?}, before the idle timeout could have bitten",
        run.elapsed
    );
}
