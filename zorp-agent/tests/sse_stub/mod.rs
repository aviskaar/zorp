//! A local listener that behaves like an OpenAI-compatible streaming
//! endpoint, in both of the body framings a provider can pick.
//!
//! Shared by `streaming_timeout.rs` and `retry_rate_limit.rs` so the two
//! cannot drift apart. Every promise either of them makes is made against
//! both framings, and that is not thoroughness for its own sake. ureq reads
//! a close-delimited body straight off the socket and a chunked one through
//! a decoder, and the two report the same failure differently: the decoder
//! catches the read error part way through a chunk and hands back "Error
//! while decoding chunks", which names neither the timeout nor anything a
//! person can act on. A test that only ever used one framing is how that
//! stayed invisible.
//!
//! Every listener binds port 0 and reads back the port the OS assigned, so
//! nothing here can collide with whatever is already running on the machine.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde_json::json;
use zorp_agent::streaming::{stream_sse, StreamOutcome};

/// How long a test will wait before it calls something a hang. Clear of any
/// idle timeout a test sets, so a busy machine cannot fail one on timing
/// alone, and nothing like the three hours a wedged run actually sat there.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// How a provider frames the body of an event stream.
///
/// Both are ordinary. OpenAI-compatible endpoints behind a CDN send
/// [`Framing::Chunked`]; a plain local runtime usually sends
/// [`Framing::CloseDelimited`]. They take different code paths inside ureq.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    CloseDelimited,
    Chunked,
}

impl Framing {
    pub const BOTH: [Framing; 2] = [Framing::CloseDelimited, Framing::Chunked];

    pub fn headers(self) -> &'static [u8] {
        match self {
            // No Content-Length and no chunked framing, so the body runs
            // until the socket closes.
            Framing::CloseDelimited => {
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                  Connection: close\r\n\r\n"
            }
            Framing::Chunked => {
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                  Transfer-Encoding: chunked\r\n\r\n"
            }
        }
    }

    /// Wrap one piece of body so it is legal under this framing.
    pub fn frame(self, body: &str) -> String {
        match self {
            Framing::CloseDelimited => body.to_string(),
            Framing::Chunked => format!("{:x}\r\n{body}\r\n", body.len()),
        }
    }

    /// Start something and do not finish it, in the place where this framing
    /// hides the failure best.
    ///
    /// Close-delimited has nowhere to hide: half an event is half an event
    /// and the read that waits for the rest times out as a read timeout.
    ///
    /// Chunked does. ureq reads it through a decoder that consumes the chunk
    /// body and then reads the two framing bytes after it with a separate
    /// call, and if that call fails for any reason at all the decoder throws
    /// the reason away and reports `InvalidInput`, "Error while decoding
    /// chunks". A whole chunk with its trailing CRLF withheld parks the
    /// decoder on exactly that read, which is how a timeout arrives wearing a
    /// protocol error's clothes. Measured against ureq 2.12.1, not assumed.
    pub fn stall_mid_frame(self) -> Vec<u8> {
        let half = "data: {\"choices\":[{\"del";
        match self {
            Framing::CloseDelimited => half.as_bytes().to_vec(),
            Framing::Chunked => format!("{:x}\r\n{half}", half.len()).into_bytes(),
        }
    }
}

/// Read the whole request, headers and body, before answering.
///
/// Not because the request is interesting, but because closing a socket that
/// still has unread bytes on it sends RST rather than FIN, and the client then
/// reports a connection reset instead of whatever the test is about.
pub fn drain_request(stream: &mut TcpStream) {
    assert!(try_drain_request(stream), "request ended before headers");
}

fn try_drain_request(stream: &mut TcpStream) -> bool {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    let header_end = loop {
        let Ok(read) = stream.read(&mut buffer) else {
            return false;
        };
        if read == 0 {
            return false;
        }
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
        let Ok(read) = stream.read(&mut buffer) else {
            return false;
        };
        if read == 0 {
            return false;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    true
}

/// Accept one connection, answer with event-stream headers, then hand the
/// socket to `body` to behave however the test needs.
pub fn sse_server(framing: Framing, body: impl FnOnce(TcpStream) + Send + 'static) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        drain_request(&mut stream);
        let _ = stream.write_all(framing.headers());
        let _ = stream.flush();
        body(stream);
    });
    address
}

/// What the stub does with one connection.
#[derive(Clone, Copy)]
pub enum Reply {
    /// A status and a JSON body, with no stream at all. The shape a gateway
    /// sends when it is shedding load, and the shape a provider sends when
    /// it will not take the request in the first place.
    Status {
        code: u16,
        retry_after: Option<u64>,
        body: &'static str,
    },
    /// A whole event stream that says it finished.
    Finished { events: usize },
    /// Some events, and then the response ends with nothing saying it was
    /// over. What a gateway hitting its own idle limit leaves behind.
    CutOff { events: usize },
}

/// Answer each connection from `script`, repeating its last entry for
/// anything past the end, and count the connections.
///
/// The count is the point of this stub. "Was this retried" and "was this not
/// retried" are both statements about how many times the request reached the
/// provider, and nothing on the client side can tell a second send from a
/// slow first one.
pub fn scripted_server(
    framing: Framing,
    script: &'static [Reply],
) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { return };
            counter.fetch_add(1, Ordering::SeqCst);
            drain_request(&mut stream);
            serve(framing, script[i.min(script.len() - 1)], stream);
        }
    });
    (address, connections)
}

/// Finish one request on a keep-alive connection, then leave the next
/// request waiting for response headers. The second request may arrive on
/// the first connection or on a new one. Both paths accept and count it.
pub fn pooled_header_stall_server() -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let request_counter = Arc::clone(&requests);
    std::thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        drain_request(&mut first);
        request_counter.fetch_add(1, Ordering::SeqCst);

        let framing = Framing::Chunked;
        let _ = first.write_all(framing.headers());
        let _ = first.write_all(event(framing, 0).as_bytes());
        let _ = first.write_all(ending(framing).as_bytes());
        let _ = first.write_all(b"0\r\n\r\n");
        let _ = first.flush();

        let reused_requests = Arc::clone(&request_counter);
        std::thread::spawn(move || {
            if try_drain_request(&mut first) {
                reused_requests.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(PATIENCE * 2);
            }
        });

        let Ok((mut second, _)) = listener.accept() else {
            return;
        };
        if try_drain_request(&mut second) {
            request_counter.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(PATIENCE * 2);
        }
    });
    (address, requests)
}

fn serve(framing: Framing, reply: Reply, mut socket: TcpStream) {
    match reply {
        Reply::Status {
            code,
            retry_after,
            body,
        } => {
            let mut head = format!(
                "HTTP/1.1 {code} STUB\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            if let Some(secs) = retry_after {
                head.push_str(&format!("Retry-After: {secs}\r\n"));
            }
            head.push_str("\r\n");
            head.push_str(body);
            let _ = socket.write_all(head.as_bytes());
            let _ = socket.flush();
            let _ = socket.shutdown(Shutdown::Write);
        }
        Reply::Finished { events } => {
            let _ = socket.write_all(framing.headers());
            for i in 0..events {
                let _ = socket.write_all(event(framing, i).as_bytes());
            }
            let _ = socket.write_all(ending(framing).as_bytes());
            let _ = socket.flush();
            close_body(framing, socket);
        }
        Reply::CutOff { events } => {
            let _ = socket.write_all(framing.headers());
            for i in 0..events {
                let _ = socket.write_all(event(framing, i).as_bytes());
            }
            let _ = socket.flush();
            close_body(framing, socket);
        }
    }
}

/// One SSE event carrying a content delta, framed for this provider.
pub fn event(framing: Framing, i: usize) -> String {
    let payload = json!({"choices": [{"delta": {"content": i.to_string()}}]});
    framing.frame(&format!("data: {payload}\n\n"))
}

/// The event an OpenAI-compatible provider sends when it has finished, and
/// the sentinel after it.
pub fn ending(framing: Framing) -> String {
    let last = json!({"choices": [{"delta": {}, "finish_reason": "stop"}]});
    format!(
        "{}{}",
        framing.frame(&format!("data: {last}\n\n")),
        framing.frame("data: [DONE]\n\n")
    )
}

/// Close the body properly. A chunked response needs its terminating chunk;
/// a close-delimited one only needs the socket to go away.
pub fn close_body(framing: Framing, mut socket: TcpStream) {
    if framing == Framing::Chunked {
        let _ = socket.write_all(b"0\r\n\r\n");
        let _ = socket.flush();
    }
    drop(socket);
}

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
