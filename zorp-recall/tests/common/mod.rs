//! Local sockets that stand in for an embedding server, and one that stands
//! in for somewhere the conversation text must never go.
//!
//! Same shape as `zorp-search/tests/common/mod.rs`: no test in this crate
//! reaches the network, and the "remote" in every test below is another
//! loopback socket, counted. A test that proved nothing left the device by
//! actually contacting a real host would be a test that contacted a real
//! host.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A socket that must never be connected to, and the count that proves it
/// was not. Stands in for a remote embedding API.
///
/// It answers nothing and closes at once, so a client that does reach it
/// fails its request. That is deliberately not what the test asserts on: a
/// failed request looks the same as a request never made, and only the
/// count tells them apart.
pub struct Canary {
    pub base: String,
    hits: Arc<AtomicUsize>,
}

impl Canary {
    pub fn new() -> Canary {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        thread::spawn(move || {
            for stream in listener.incoming() {
                counter.fetch_add(1, Ordering::SeqCst);
                if let Ok(stream) = stream {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
            }
        });
        Canary {
            base: format!("http://{addr}"),
            hits,
        }
    }

    /// How many connections reached it. Read after giving the client every
    /// chance to make one.
    pub fn hits(&self) -> usize {
        // A connection the client opened just before returning may still be
        // in the accept queue. Give the listener thread a moment before
        // declaring nothing arrived, so a pass means "did not connect"
        // rather than "connected slightly too late to be seen".
        thread::sleep(Duration::from_millis(150));
        self.hits.load(Ordering::SeqCst)
    }
}

/// A loopback server that answers every request with a redirect to
/// `location`. Serves connections until the test ends.
pub fn redirector(location: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            read_request(&mut stream);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
    });
    format!("http://{addr}")
}

/// A loopback server that answers `status` with `body`, over and over, and
/// hands back every request it was sent.
pub fn server(status: u16, body: &str) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let reason = if status == 200 { "OK" } else { "ERROR" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let request = read_request(&mut stream);
            let _ = tx.send(request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
    });
    (format!("http://{addr}"), rx)
}

/// A loopback address with nothing listening on it. Binding and dropping is
/// how you get a port that is definitely free rather than one that is
/// probably free.
pub fn dead_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

/// Take the captured request, or fail the test.
pub fn captured(rx: &Receiver<String>) -> String {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("no request reached the mock server")
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(header) = find_subslice(&buf, b"\r\n\r\n") {
            let end = header + 4;
            let headers = String::from_utf8_lossy(&buf[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            while buf.len() < end + length {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
