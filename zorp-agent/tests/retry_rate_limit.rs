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
//! A 404 is on whichever side its body puts it. One naming an upstream
//! provider in `metadata.provider_name` is a gateway relaying an upstream
//! that failed, and it is sent again on the status line or inside a 200
//! stream alike, while nothing has been handed up. One naming no provider
//! is a wrong URL or model id and is never sent again.
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

/// The error behind "404 Provider returned error", which killed two attempts
/// today after 26 good model calls each, and a third on its first request.
/// It is OpenRouter relaying an upstream that failed, and the body says whose
/// in `metadata.provider_name`. That name is what makes it the upstream's
/// error rather than ours. The status-line body is as logged, trimmed of the
/// user id. The stream event was not captured whole, so it is the same error
/// object in OVERLOADED's envelope.
const UPSTREAM_NOT_FOUND: &str = r#"{"error":{"message":"Provider returned error","code":404,"metadata":{"raw":"","provider_name":"Nvidia","is_byok":false}}}"#;
const UPSTREAM_NOT_FOUND_EVENT: &str = r#"{"id":"gen-1788600000-x","object":"chat.completion.chunk","created":1788600000,"model":"nvidia/nemotron-3-ultra-550b-a55b:free","provider":"Nvidia","choices":[],"error":{"code":404,"message":"Provider returned error","metadata":{"raw":"","provider_name":"Nvidia","is_byok":false}}}"#;

/// A 404 that is ours: no such model, so no upstream was asked and the body
/// names none. As a document it is a status body; as one event it is the
/// same refusal inside a 200 stream.
/// The same failure as an event inside a stream, in the shape of the captured
/// 502 event: the provider is named only at the top of the chunk.
const UPSTREAM_NOT_FOUND_EVENT_TOP_LEVEL: &str = r#"{"id":"gen-1788600000-y","object":"chat.completion.chunk","created":1788600000,"model":"nvidia/nemotron-3-ultra-550b-a55b:free","provider":"Nvidia","choices":[],"error":{"code":404,"message":"Provider returned error","metadata":{"error_type":"provider_unavailable"}}}"#;
const NO_SUCH_MODEL: &str =
    r#"{"error":{"message":"No endpoints found for nvidia/nemotron-9-ultra:free.","code":404}}"#;

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

/* ---- the 404 that is the upstream's, and the 404 that is ours ---- */

/// Two attempts died to this after 26 good calls each. The status line said
/// 200, the event said 404, and the body named Nvidia: the request was
/// routed and the upstream failed, which is the 502 case with a different
/// number, and the same clean retry.
#[test]
fn a_404_naming_an_upstream_inside_a_stream_is_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::ErrorEvent {
            after: 0,
            event: UPSTREAM_NOT_FOUND_EVENT,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream whose upstream failed once");

        assert!(
            run.error.is_none(),
            "{framing:?}: a 404 naming an upstream, inside a 200 stream, killed \
             the attempt: {:?}",
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
        assert_eq!(
            run.payloads, 5,
            "{framing:?}: the retried stream did not deliver exactly one answer"
        );
    }
}

/// The chunk may name the upstream only at its top, as the captured 502
/// event does. That is still a name, and still the upstream's failure.
#[test]
fn a_404_naming_an_upstream_only_at_the_top_of_the_chunk_is_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::ErrorEvent {
            after: 0,
            event: UPSTREAM_NOT_FOUND_EVENT_TOP_LEVEL,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream whose upstream failed once");

        assert!(
            run.error.is_none(),
            "{framing:?}: a 404 naming an upstream at the top of the chunk killed \
             the attempt: {:?}",
            run.error
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: the request was not sent again"
        );
    }
}

/// The third attempt got the same error on the status line, on its first
/// request. The envelope is not what tells the two 404s apart. The body is,
/// and it is read before the decision.
#[test]
fn a_404_naming_an_upstream_on_the_status_line_is_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::Status {
            code: 404,
            retry_after: None,
            body: UPSTREAM_NOT_FOUND,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a status line relaying a failed upstream");

        assert!(
            run.error.is_none(),
            "{framing:?}: a 404 naming an upstream, on the status line, killed \
             the attempt: {:?}",
            run.error
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "{framing:?}: the request was not sent again"
        );
        assert_eq!(
            run.payloads, 5,
            "{framing:?}: the retried stream did not deliver exactly one answer"
        );
    }
}

/// A 404 whose body names no provider is a model that is not there. Nobody
/// upstream was asked, it will not get better, and it is not sent again in
/// either envelope.
#[test]
fn a_404_naming_no_provider_is_ours_and_is_not_sent_again() {
    bound_the_retrying();
    static AS_STATUS: &[Reply] = &[Reply::Status {
        code: 404,
        retry_after: None,
        body: NO_SUCH_MODEL,
    }];
    static IN_STREAM: &[Reply] = &[Reply::ErrorEvent {
        after: 0,
        event: NO_SUCH_MODEL,
    }];
    for (script, envelope) in [(AS_STATUS, "status line"), (IN_STREAM, "200 stream")] {
        for framing in Framing::BOTH {
            let (address, connections) = scripted_server(framing, script);

            let run = stream_with_patience(address, "a model that is not there");

            let error = run.error.unwrap_or_else(|| {
                panic!("{framing:?}: a 404 on the {envelope} came back as an answer")
            });
            assert!(
                error.contains("404") && error.contains("No endpoints found"),
                "{framing:?}: the error does not say what the provider said: {error}"
            );
            assert_eq!(
                connections.load(Ordering::SeqCst),
                1,
                "{framing:?}: a 404 on the {envelope} naming no provider was sent \
                 again, which will never work and hides the cause"
            );
        }
    }
}

/// The same event after a delta is on the other side of the line, whoever
/// it names. The transport does not send again; the loop above may ask
/// again, and `reask_dropped_stream.rs` proves that by type, not by code.
#[test]
fn a_404_after_a_delta_is_not_sent_again() {
    bound_the_retrying();
    static SCRIPT: &[Reply] = &[
        Reply::ErrorEvent {
            after: 1,
            event: UPSTREAM_NOT_FOUND_EVENT,
        },
        Reply::Finished { events: 3 },
    ];
    for framing in Framing::BOTH {
        let (address, connections) = scripted_server(framing, SCRIPT);

        let run = stream_with_patience(address, "a stream whose upstream failed after a delta");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: an error event came back as an answer"));
        assert!(
            error.contains("404") && error.contains("from Nvidia"),
            "{framing:?}: the error does not say whose error it was: {error}"
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
