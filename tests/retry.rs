//! The buffered path retries a provider that says "not now", and only that.
//!
//! Its own test binary, not a case inside `http.rs`, because the bound is
//! read from the environment and a test that sets it must not race the rest
//! of the suite. One process, one policy, set once before anything sends.
//!
//! What made this worth building: a 250 crate calibration run against
//! OpenRouter's free tier discarded 25 of its first 48 attempts, every one of
//! them a 429 whose body said "Please retry shortly". An attempt is an agent
//! loop of up to 40 model calls, so one 429 anywhere in it throws away the
//! whole attempt and everything it had gathered.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use serde_json::json;

/// Three sends per request, not the shipped four, so a test that exhausts
/// the bound waits half a second and then a second and is done.
const ATTEMPTS: u32 = 3;

/// Large enough that it never binds here. The budget's own arithmetic is
/// unit tested against `RetryPolicy::delay`, where it costs nothing.
const BUDGET_SECS: u64 = 30;

/// What the provider asks for in the one test about `Retry-After`. Longer
/// than any backoff this policy would have picked on its own, which is how
/// that test can tell the two apart.
const RETRY_AFTER_SECS: u64 = 1;

/// Long enough that a busy machine cannot fail these on timing, short enough
/// that a retry loop with no bound is caught rather than waited on.
const PATIENCE: Duration = Duration::from_secs(20);

static ENV: Once = Once::new();

fn bound_the_retrying() {
    ENV.call_once(|| {
        std::env::set_var(zorp::RETRY_ATTEMPTS_VAR, ATTEMPTS.to_string());
        std::env::set_var(zorp::RETRY_BUDGET_VAR, BUDGET_SECS.to_string());
    });
}

/// One reply from the stub, in the order the script lists them.
#[derive(Clone, Copy)]
struct Reply {
    status: u16,
    /// The `Retry-After` header, when this reply carries one.
    retry_after: Option<u64>,
    body: &'static str,
}

impl Reply {
    const fn status(status: u16, body: &'static str) -> Self {
        Self {
            status,
            retry_after: None,
            body,
        }
    }

    const fn after(mut self, secs: u64) -> Self {
        self.retry_after = Some(secs);
        self
    }
}

/// The body OpenRouter's free tier actually sends, trimmed. Kept verbatim
/// rather than invented, because the useful half of this feature is that the
/// provider says what it wants and we do it.
const RATE_LIMITED: &str = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"stealth/ox-alpha is temporarily rate-limited upstream. Please retry shortly."}}}"#;

const ANSWER: &str = r#"{"choices":[{"message":{"content":"hi"}}]}"#;

/// Answer each connection from `script`, repeating its last entry for
/// anything past the end, and count the connections.
///
/// The count is the whole point of the stub. "Was this retried" and "was
/// this not retried" are both statements about how many times the request
/// reached the provider, and nothing else on the client side can tell a
/// second send from a slow first one.
fn scripted(script: &'static [Reply]) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { return };
            counter.fetch_add(1, Ordering::SeqCst);
            drain_request(&mut stream);
            let reply = script[i.min(script.len() - 1)];
            let mut head = format!(
                "HTTP/1.1 {} STUB\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n",
                reply.status,
                reply.body.len()
            );
            if let Some(secs) = reply.retry_after {
                head.push_str(&format!("Retry-After: {secs}\r\n"));
            }
            head.push_str("\r\n");
            head.push_str(reply.body);
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
        }
    });
    (format!("http://{address}/v1/chat/completions"), connections)
}

/// Read the whole request before answering. Closing a socket with unread
/// bytes on it sends RST, and the client then reports a connection reset
/// instead of the status this test is about.
fn drain_request(stream: &mut TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    let header_end = loop {
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while request.len() < header_end + length {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
}

/// What one call to `zorp_raw` did.
struct Run {
    content: Option<String>,
    error: Option<String>,
    elapsed: Duration,
}

/// Send on another thread and refuse to wait forever, so a bound that does
/// not hold fails with a message instead of hanging the suite.
fn send_with_patience(url: String, what: &str) -> Run {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let outcome = zorp::zorp_raw(&url, &[], json!({"model": "m", "messages": []}));
        let _ = tx.send(Run {
            content: outcome.as_ref().ok().map(|value| {
                value["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            }),
            error: outcome.err().map(|e| e.to_string()),
            elapsed: started.elapsed(),
        });
    });
    rx.recv_timeout(PATIENCE)
        .unwrap_or_else(|_| panic!("{what}: the request was still going after {PATIENCE:?}"))
}

/// The measured case. Half a calibration run was being thrown away by a
/// status the provider itself described as temporary.
#[test]
fn a_rate_limited_request_is_sent_again_and_the_caller_sees_the_answer() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::status(429, RATE_LIMITED), Reply::status(200, ANSWER)];
    let (url, connections) = scripted(SCRIPT);

    let run = send_with_patience(url, "a provider that was rate limited once");

    assert!(
        run.error.is_none(),
        "a 429 the provider asked us to retry reached the caller: {:?}",
        run.error
    );
    assert_eq!(
        run.content.as_deref(),
        Some("hi"),
        "the answer from the second send did not come back"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "the request was not sent again"
    );
}

/// 503 is the same statement in a different word: the provider is not taking
/// work, nothing was generated, come back. It is retried for that reason and
/// no other.
#[test]
fn an_unavailable_provider_is_sent_the_request_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::status(503, r#"{"error":"upstream is down"}"#),
        Reply::status(200, ANSWER),
    ];
    let (url, connections) = scripted(SCRIPT);

    let run = send_with_patience(url, "a provider that was briefly unavailable");

    assert!(
        run.error.is_none(),
        "a 503 reached the caller: {:?}",
        run.error
    );
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

/// When the provider says how long, that is the answer. Guessing over the
/// top of a number you were given is how a client ends up back too early and
/// gets rate limited for it.
#[test]
fn the_wait_the_provider_asked_for_is_the_wait() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::status(429, RATE_LIMITED).after(RETRY_AFTER_SECS),
        Reply::status(200, ANSWER),
    ];
    let (url, connections) = scripted(SCRIPT);

    let run = send_with_patience(url, "a provider that named its own delay");

    assert!(run.error.is_none(), "the retry failed: {:?}", run.error);
    assert_eq!(connections.load(Ordering::SeqCst), 2);
    // The backoff this policy would have picked for a first retry is well
    // under a second, so waiting a second is only explicable by the header.
    assert!(
        run.elapsed >= Duration::from_secs(RETRY_AFTER_SECS),
        "came back after {:?}, sooner than the {RETRY_AFTER_SECS}s the provider asked for",
        run.elapsed
    );
}

/// The half of this that is about not wasting a person's time.
///
/// A 400 is a body the provider will not accept, a 401 is a key it will not
/// take and a 404 is a model that is not there. None of them get better by
/// being asked again, and retrying them turns a misconfiguration into a slow
/// misconfiguration that looks like a network problem.
#[test]
fn a_request_the_provider_refused_is_not_sent_again() {
    bound_the_retrying();
    static BAD_REQUEST: &[Reply] = &[Reply::status(400, r#"{"error":"bad request"}"#)];
    static UNAUTHORIZED: &[Reply] = &[Reply::status(401, r#"{"error":"invalid api key"}"#)];
    static NOT_FOUND: &[Reply] = &[Reply::status(404, r#"{"error":"no such model"}"#)];
    for (script, code) in [(BAD_REQUEST, 400), (UNAUTHORIZED, 401), (NOT_FOUND, 404)] {
        let (url, connections) = scripted(script);

        let run = send_with_patience(url, "a provider that refused the request");

        let error = run
            .error
            .unwrap_or_else(|| panic!("a {code} came back as an answer"));
        assert!(
            error.contains(&code.to_string()),
            "the error does not name the status: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "a {code} was sent again, which will never work and hides the cause"
        );
    }
}

/// A provider can refuse inside a 200 too: OpenRouter answers an overloaded
/// upstream with a 200 whose body is an error object carrying a 502. On the
/// buffered path nothing has reached the caller until the whole body is
/// parsed, so that is the same clean retry keyed on the body's code rather
/// than the status line's.
#[test]
fn an_error_object_inside_a_200_body_is_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::status(
            200,
            r#"{"choices":[],"error":{"code":502,"message":"Upstream error from Nvidia: Service temporarily overloaded"}}"#,
        ),
        Reply::status(200, ANSWER),
    ];
    let (url, connections) = scripted(SCRIPT);

    let run = send_with_patience(url, "a 200 whose body said 502");

    assert!(
        run.error.is_none(),
        "a 502 inside a 200 body reached the caller: {:?}",
        run.error
    );
    assert_eq!(run.content.as_deref(), Some("hi"));
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

/// And one carrying a code that will never get better is named, code and
/// message, rather than becoming "no choices in response" one layer up.
#[test]
fn an_error_object_inside_a_200_body_is_named_and_not_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::status(
        200,
        r#"{"error":{"code":400,"message":"tool_choice is not supported"}}"#,
    )];
    let (url, connections) = scripted(SCRIPT);

    let run = send_with_patience(url, "a 200 whose body said 400");

    let error = run
        .error
        .unwrap_or_else(|| panic!("a 400 inside a 200 body came back as an answer"));
    assert!(
        error.contains("400") && error.contains("tool_choice is not supported"),
        "the error does not name the code and the message: {error}"
    );
    assert_eq!(connections.load(Ordering::SeqCst), 1);
}

/// The bound, which is the difference between retrying and hanging.
///
/// A provider having a bad afternoon can 429 every request for hours. What
/// must not happen is a client that waits it out, because nobody watching a
/// browser will, and a batch run that quietly triples in length is a run
/// nobody can plan around.
#[test]
fn a_provider_that_is_always_rate_limited_gives_up_and_says_why() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::status(429, RATE_LIMITED)];
    let (url, connections) = scripted(SCRIPT);

    let run = send_with_patience(url, "a provider that is always rate limited");

    let error = run
        .error
        .unwrap_or_else(|| panic!("an endless 429 came back as an answer"));
    assert!(
        error.contains("rate limited"),
        "the error does not say it was rate limited, so a slow run has no legible cause: {error}"
    );
    assert!(
        error.contains("429"),
        "the error does not name the status: {error}"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        ATTEMPTS as usize,
        "the number of sends is not the bound"
    );
}
