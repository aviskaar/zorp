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
