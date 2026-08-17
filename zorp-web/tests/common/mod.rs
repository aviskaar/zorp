use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

/// A live connection to `GET /api/sessions/:id/events`.
///
/// That endpoint is long lived on purpose: it stays open across turns so the
/// browser never has to reconnect. So a test must never read it to the end,
/// because the end only comes when the client hangs up. Every read here is
/// bounded by a deadline, the caller stops as soon as it has seen what it
/// needs, and dropping this closes the socket.
///
/// It speaks HTTP by hand rather than through `ureq` so that a read timeout is
/// an ordinary "nothing yet" and not a poisoned response body.
#[allow(dead_code)]
pub struct EventStream {
    socket: TcpStream,
    bytes: Vec<u8>,
    ended: bool,
}

#[allow(dead_code)]
impl EventStream {
    /// Connect as a fresh browser would, with no resume point.
    pub fn connect(addr: SocketAddr, session: &str) -> EventStream {
        EventStream::resume(addr, session, None)
    }

    /// Connect the way a browser does after a dropped connection, telling the
    /// server the last event id it saw.
    pub fn resume(addr: SocketAddr, session: &str, last_event_id: Option<u64>) -> EventStream {
        let socket = TcpStream::connect(addr).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let resume = match last_event_id {
            Some(seq) => format!("Last-Event-ID: {seq}\r\n"),
            None => String::new(),
        };
        let request = format!(
            "GET /api/sessions/{session}/events HTTP/1.1\r\n\
             Host: {addr}\r\n\
             Accept: text/event-stream\r\n\
             {resume}\r\n"
        );
        (&socket).write_all(request.as_bytes()).unwrap();
        EventStream {
            socket,
            bytes: Vec::new(),
            ended: false,
        }
    }

    /// Everything received so far, chunked-transfer framing included. Tests
    /// only ever look for substrings and `id:` lines, which survive it.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    /// Read until `needle` has arrived `count` times. Returns false if it
    /// never does, so the caller can fail with a useful message.
    pub fn wait_for_count(&mut self, needle: &str, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.count(needle) >= count {
                return true;
            }
            if self.ended || Instant::now() >= deadline {
                return false;
            }
            self.pump();
        }
    }

    /// Read until `needle` arrives.
    pub fn wait_for(&mut self, needle: &str, timeout: Duration) -> bool {
        self.wait_for_count(needle, 1, timeout)
    }

    /// Keep reading for `window` and report whether the server ended the
    /// response.
    ///
    /// This is what tells a long lived stream apart from one that ends the
    /// moment a turn finishes, which is what sends the browser into a
    /// reconnect loop. Note that the end of a response is not the end of the
    /// socket: hyper keeps the connection alive for the next request, so
    /// waiting for EOF here would wait forever and prove nothing. What
    /// `EventSource` reacts to is the response ending, which on the wire is
    /// the final zero length chunk.
    pub fn response_ended_within(&mut self, window: Duration) -> bool {
        let deadline = Instant::now() + window;
        while !self.ended && Instant::now() < deadline {
            self.pump();
        }
        self.ended
    }

    /// The HTTP status line, for asserting on a rejected connection.
    pub fn status_line(&mut self, timeout: Duration) -> String {
        self.wait_for("\r\n", timeout);
        self.text().lines().next().unwrap_or_default().to_string()
    }

    pub fn count(&self, needle: &str) -> usize {
        self.text().matches(needle).count()
    }

    /// The event ids the server has sent, in arrival order.
    pub fn seqs(&self) -> Vec<u64> {
        self.text()
            .lines()
            .filter_map(|line| line.strip_prefix("id: "))
            .filter_map(|seq| seq.trim().parse().ok())
            .collect()
    }

    fn pump(&mut self) {
        let mut chunk = [0u8; 4096];
        match self.socket.read(&mut chunk) {
            Ok(0) => self.ended = true,
            Ok(n) => {
                self.bytes.extend_from_slice(&chunk[..n]);
                // The zero length chunk closes a chunked response. No SSE
                // frame can contain it, because frames end in bare newlines.
                if find_subslice(&self.bytes, b"\r\n0\r\n\r\n").is_some() {
                    self.ended = true;
                }
            }
            // A read timeout just means the server has nothing to say yet,
            // which is the normal state of an idle stream.
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => self.ended = true,
        }
    }
}

/// Mock server that serves a queued list of 200 JSON response bodies, one per
/// connection, in order — for multi-turn agent runs. Returns the base URL.
#[allow(dead_code)]
pub fn mock_script(bodies: Vec<&str>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let owned: Vec<String> = bodies.into_iter().map(|s| s.to_string()).collect();
    thread::spawn(move || {
        for body in owned {
            if let Ok((mut stream, _)) = listener.accept() {
                read_request(&mut stream);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        }
    });
    format!("http://{addr}")
}

#[allow(dead_code)]
pub fn mock_capture(
    status: u16,
    content_type: &str,
    body: &str,
) -> (String, std::sync::mpsc::Receiver<String>) {
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
            let request = read_request_capture(&mut stream);
            let _ = tx.send(request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (format!("http://{addr}"), rx)
}

#[allow(dead_code)]
fn read_request_capture(stream: &mut std::net::TcpStream) -> String {
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

fn read_request(stream: &mut std::net::TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                    break pos + 4;
                }
            }
            Err(_) => return,
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    let already = buf.len() - header_end;
    let mut remaining = content_length.saturating_sub(already);
    while remaining > 0 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => remaining = remaining.saturating_sub(n),
            Err(_) => break,
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
