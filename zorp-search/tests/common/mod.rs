//! Canned HTTP responses served from a local socket. Same pattern as
//! `zorp-agent/tests/common/mod.rs`: no test here reaches the network and
//! none needs a real API key.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

/// Take the captured request, or fail the test. Bounded, so a provider that
/// never sends anything is a failure instead of a hung test run.
pub fn captured(rx: &Receiver<String>) -> String {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("no request reached the mock server")
}

/// One-shot mock HTTP server. Serves one connection with `status` + `body`
/// (using `content_type`), then the thread exits. Returns "http://127.0.0.1:PORT".
/// Reads the client's full request (headers + Content-Length body) before
/// responding, so it stays reliable under parallel test execution.
pub fn mock(status: u16, content_type: &str, body: &str) -> String {
    let (base, _rx) = mock_capture(status, content_type, body);
    base
}

/// Like `mock`, but also hands back the raw request text the client sent, so a
/// test can assert on headers and on the JSON body.
pub fn mock_capture(status: u16, content_type: &str, body: &str) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let reason = if status == 200 { "OK" } else { "ERROR" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let request = read_request(&mut stream);
            let _ = tx.send(request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
    });
    (format!("http://{addr}"), rx)
}

/// Mock server that accepts the connection and then closes it without writing a
/// response. That is a transport failure with no status code, which is a
/// different thing from an HTTP error, and it is deterministic (unlike aiming
/// at a port nothing is listening on).
pub fn mock_hangup() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            read_request(&mut stream);
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    format!("http://{addr}")
}

/// Read headers, then the Content-Length body, and return the whole request.
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
