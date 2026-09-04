//! The streaming path retries a provider that will not take the request, and
//! never retries one that has already started answering.
//!
//! That split is the whole file. A 429 arrives before a single byte of body,
//! so sending the request again is clean: nothing reached the caller and
//! nothing was generated upstream. A failure part way through a stream is the
//! opposite. Payloads have already been handed to `on_payload`, which in the
//! browser means text already on somebody's screen, so a second attempt would
//! replay the beginning of the answer against the middle of the last one. A
//! transport is not allowed to invent that.
//!
//! The line between the two is the first payload handed up, not the HTTP
//! status. A provider can refuse inside a 200 stream, as one event carrying
//! an error object and nothing else, and that refusal is on the clean side of
//! the line: it is named and sent again. The same event after a delta is on
//! the other side, and stays an error.
//!
//! Its own test binary, like `streaming_timeout.rs` and for a related reason:
//! the bound on retrying is read from the environment, and a test that sets
//! it must not race the rest of the suite.

mod sse_stub;

use std::sync::atomic::Ordering;
use std::sync::Once;
use std::time::Duration;

use sse_stub::{scripted_server, stream_with_patience, Framing, Reply};

/// Three sends per request, not the shipped four, so a test that exhausts
/// the bound waits half a second and then a second and is done.
const ATTEMPTS: u32 = 3;

/// Large enough that it never binds here. The budget's own arithmetic is
/// unit tested against `RetryPolicy::delay` in the core, where it costs no
/// wall clock at all.
const BUDGET_SECS: u64 = 30;

/// The body OpenRouter's free tier actually sends, trimmed.
const RATE_LIMITED: &str = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"stealth/ox-alpha is temporarily rate-limited upstream. Please retry shortly."}}}"#;

/// The event OpenRouter sent, verbatim, for nine of nine benchmark trials
/// against `nvidia/nemotron-3-ultra-550b-a55b:free` (ZOR-26): HTTP 200, a
/// streaming body, some comment lines, this as the only event, and a close.
/// Every trial died within 1 to 11 model calls reading it as a cut off
/// stream, and the sentence the provider wrote never reached anyone.
const OVERLOADED: &str = r#"{"id":"gen-1788503823-rSnjToVnWKeIBMh0SRHz","object":"chat.completion.chunk","created":1788503823,"model":"nvidia/nemotron-3-ultra-550b-a55b:free","provider":"Nvidia","choices":[],"error":{"code":502,"message":"Upstream error from Nvidia: Service temporarily overloaded","metadata":{"error_type":"provider_unavailable"}}}"#;

/// The same envelope carrying a code that will never get better.
const UNSUPPORTED: &str =
    r#"{"choices":[],"error":{"code":400,"message":"tool_choice is not supported"}}"#;

static ENV: Once = Once::new();

fn bound_the_retrying() {
    ENV.call_once(|| {
        std::env::set_var(zorp::RETRY_ATTEMPTS_VAR, ATTEMPTS.to_string());
        std::env::set_var(zorp::RETRY_BUDGET_VAR, BUDGET_SECS.to_string());
    });
}

const fn rate_limited() -> Reply {
    Reply::Status {
        code: 429,
        retry_after: None,
        body: RATE_LIMITED,
    }
}

/// The measured case, on the path every real model call takes.
///
/// A 429 comes back before the response body exists, so the request goes
/// again and the caller is told nothing about the first one except on stderr.
#[test]
fn a_rate_limited_stream_is_sent_again_and_the_caller_never_sees_the_429() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[rate_limited(), Reply::Finished { events: 3 }];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream that was rate limited once");

        assert!(
            run.error.is_none(),
            "{framing:?}: a 429 the provider asked us to retry killed the \
             attempt: {:?}",
            run.error
        );
        assert!(
            run.streamed,
            "{framing:?}: the second send was not read as an event stream"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: the request was not sent again"
        );
        // Three deltas, the finish_reason event and [DONE]. The count is
        // here to say the retry delivered one answer and not one and a bit.
        assert_eq!(
            run.payloads, 5,
            "{framing:?}: the retried stream did not deliver exactly one answer"
        );
    }
}

/// The wrinkle, and the reason this file exists rather than a line in the
/// core's test.
///
/// The provider took the request, streamed part of an answer and stopped
/// without saying it had finished. That is an error, PR #96 made it one, and
/// it must stay an error rather than becoming a retry: the events that
/// arrived were handed to the caller on the way past, so sending the request
/// again would put the first three deltas of a fresh answer after the first
/// three of the abandoned one.
#[test]
fn a_stream_that_failed_part_way_through_is_not_sent_again_and_does_not_replay() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::CutOff { events: 3 }, Reply::Finished { events: 3 }];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream that stopped part way through");

        assert!(
            !run.streamed,
            "{framing:?}: a cut off answer came back as a finished one"
        );
        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: a cut off answer was not an error"));
        assert!(
            error.contains("finish"),
            "{framing:?}: the error does not say the provider never finished: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "{framing:?}: a half delivered answer was sent again, and the \
             second one would have replayed over the first"
        );
        assert_eq!(
            run.payloads, 3,
            "{framing:?}: the caller saw something other than the three \
             events that actually arrived"
        );
    }
}

/// A request the provider will not take is not made better by making it
/// twice, and a run that spends its retries on a bad key learns nothing.
#[test]
fn a_request_the_provider_refused_is_not_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::Status {
        code: 400,
        retry_after: None,
        body: r#"{"error":"bad request"}"#,
    }];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a provider that refused the request");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: a 400 came back as an answer"));
        assert!(
            error.contains("400"),
            "{framing:?}: the error does not name the status: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "{framing:?}: a 400 was sent again, which will never work and \
             hides the cause"
        );
    }
}

/// The bound holds on this path too, and the error says what happened.
#[test]
fn a_provider_that_is_always_rate_limited_gives_up_and_says_why() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[rate_limited()];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a provider that is always rate limited");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: an endless 429 came back as an answer"));
        assert!(
            error.contains("rate limited"),
            "{framing:?}: the error does not say it was rate limited, so a \
             slow run has no legible cause: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            ATTEMPTS as usize,
            "{framing:?}: the number of sends is not the bound"
        );
        // The waits are backoff and nothing else, so exhausting three sends
        // takes about a second and a half. Well inside what a person in a
        // browser would sit through, which is the case that picked the bound.
        assert!(
            run.elapsed < Duration::from_secs(10),
            "{framing:?}: gave up after {:?}, which is not a bound anyone \
             waiting on a browser would call one",
            run.elapsed
        );
    }
}

/* ---- the refusal that arrives inside a 200 ---- */

/// The ZOR-26 case. The status line said 200, the body said 502, and nothing
/// had been handed up, so this is the clean retry in a different envelope.
#[test]
fn an_error_event_before_any_delta_is_sent_again_and_the_caller_gets_the_answer() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::ErrorEvent {
            after: 0,
            event: OVERLOADED,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream that was overloaded once");

        assert!(
            run.error.is_none(),
            "{framing:?}: a 502 delivered inside a 200 stream killed the \
             attempt: {:?}",
            run.error
        );
        assert!(
            run.streamed,
            "{framing:?}: the second send was not streamed"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: the request was not sent again"
        );
        // Three deltas, the finish_reason event and [DONE], and not the error
        // event: it is not a delta and an accumulator would drop it anyway.
        assert_eq!(
            run.payloads, 5,
            "{framing:?}: the retried stream did not deliver exactly one answer"
        );
    }
}

/// The same event one delta later is on the other side of the line. The
/// delta is on somebody's screen, so no second send, and the error says what
/// the provider said rather than "cut off".
#[test]
fn an_error_event_after_a_delta_is_not_sent_again_and_names_the_code() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::ErrorEvent {
            after: 1,
            event: OVERLOADED,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream that failed after a delta");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: an error event came back as an answer"));
        assert!(
            error.contains("502") && error.contains("Service temporarily overloaded"),
            "{framing:?}: the error does not say what the provider said: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "{framing:?}: a stream that had delivered a delta was sent again"
        );
        assert_eq!(
            run.payloads, 1,
            "{framing:?}: the caller saw something other than the one delta"
        );
    }
}

/// A code that is never retried is not retried inside a 200 either, and it
/// is named rather than reported as a cut off stream.
#[test]
fn an_error_event_the_provider_will_not_take_back_is_named_and_not_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::ErrorEvent {
        after: 0,
        event: UNSUPPORTED,
    }];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream carrying a 400");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: a 400 inside a stream came back as an answer"));
        assert!(
            error.contains("400") && error.contains("tool_choice is not supported"),
            "{framing:?}: the error does not name the code and the message: {error}"
        );
        assert!(
            !error.contains("finish"),
            "{framing:?}: a named refusal was reported as a cut off stream: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "{framing:?}: a 400 was sent again, which will never work"
        );
        assert_eq!(
            run.payloads, 0,
            "{framing:?}: the error event was handed up"
        );
    }
}

/// The bound is the same bound. An upstream that stays overloaded is given
/// up on after the same number of sends, and the error names the last code.
#[test]
fn a_provider_that_is_always_overloaded_gives_up_and_names_the_code() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[Reply::ErrorEvent {
        after: 0,
        event: OVERLOADED,
    }];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a provider that is always overloaded");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: an endless 502 came back as an answer"));
        assert!(
            error.contains("502") && error.contains("overloaded"),
            "{framing:?}: the error does not name the code: {error}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            ATTEMPTS as usize,
            "{framing:?}: the number of sends is not the bound"
        );
        assert!(
            run.elapsed < Duration::from_secs(10),
            "{framing:?}: gave up after {:?}",
            run.elapsed
        );
    }
}

/// The endpoint that ignores `stream` and answers with a document takes a
/// different branch, and a document that is an error object is the same
/// refusal: nothing handed up, so sent again.
#[test]
fn an_error_document_from_an_endpoint_that_ignored_stream_is_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::Status {
            code: 200,
            retry_after: None,
            body: r#"{"error":{"code":503,"message":"upstream is warming up"}}"#,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a 200 document that was an error");

        assert!(
            run.error.is_none(),
            "{framing:?}: a 503 inside a 200 document killed the attempt: {:?}",
            run.error
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: the request was not sent again"
        );
    }
}
