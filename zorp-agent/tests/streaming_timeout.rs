//! The streaming path is bounded, it is bounded by silence rather than by
//! length, and exceeding the bound is loud.
//!
//! Its own test binary, not a module inside `streaming.rs`, for one reason:
//! the read timeout is read once, when the shared agent is first built, so a
//! test that wants a short one has to set it before anything else in the
//! process has made an HTTP call. A separate binary is a separate process,
//! which is the only way to promise that without ordering the whole suite.
//!
//! The listener that plays the provider lives in `sse_stub`, shared with
//! `retry_rate_limit.rs`. Every test here runs against both framings a
//! provider can pick, for the reason that module's doc gives.

mod sse_stub;

use std::io::Write;
use std::sync::Once;
use std::time::Duration;

use sse_stub::{close_body, ending, event, sse_server, stream_with_patience, Framing, PATIENCE};

/// Short enough to keep the suite quick, long enough that a healthy stream
/// pausing for [`GAP`] between chunks is nowhere near it.
const IDLE_TIMEOUT_SECS: u64 = 3;

/// The pause a healthy stream takes between chunks, and the whole point of
/// [`a_slow_but_talking_provider_is_not_cut_off`]: well inside the timeout,
/// repeated until the response as a whole has outlasted it.
const GAP: Duration = Duration::from_millis(400);

/// How many of those chunks. Ten of them is four seconds, so the response
/// lives longer than the idle timeout without ever going quiet for as long.
const PIECES: usize = 10;

static ENV: Once = Once::new();

/// Set the timeout before anything builds the shared agent, which caches it
/// on first use. `Once` because every test needs it and the harness runs them
/// on parallel threads.
fn bound_the_wait() {
    ENV.call_once(|| {
        std::env::set_var(zorp::READ_TIMEOUT_VAR, IDLE_TIMEOUT_SECS.to_string());
    });
}

/// The bug the first version of this file existed for.
///
/// A provider that accepts the request, sends headers and then goes quiet was
/// waited on forever, because the streaming path built its own `ureq::agent()`
/// with no timeouts while every buffered call went through the configured one.
/// Measured in the wild at 3 hours 18 minutes of 0% CPU on an established
/// socket, which is where a 200 sample calibration run stopped at 123.
#[test]
fn a_provider_that_goes_quiet_ends_the_stream_instead_of_being_waited_on() {
    bound_the_wait();
    for framing in Framing::BOTH {
        // Headers, then nothing, for longer than this test will wait. Holding
        // the socket open is the point: closing it would be a different
        // failure, and it has its own test below.
        let address = sse_server(framing, |_socket| std::thread::sleep(PATIENCE * 4));

        let run = stream_with_patience(address, "a provider that went quiet");

        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: a silent socket came back as an answer"));
        assert!(
            error.contains(zorp::READ_TIMEOUT_VAR),
            "{framing:?}: the error does not say what ran out or how to buy \
             more of it: {error}"
        );
        // Not instant, or the error came from something other than the
        // timeout. A refused connection looks identical from here.
        assert!(
            run.elapsed + Duration::from_millis(250) >= Duration::from_secs(IDLE_TIMEOUT_SECS),
            "{framing:?}: gave up after {:?}, too soon to have been the idle timeout",
            run.elapsed
        );
    }
}

/// The case that broke a nine hour experiment, and the one the test above
/// does not cover.
///
/// The provider does not go quiet before saying anything. It says some of the
/// answer and then stops, which is what a stalled generation behind a gateway
/// looks like, and it is the ordinary case rather than the exotic one.
///
/// What made it expensive is how it read from above. A truncated answer is
/// still an answer, so the attempt scored it as a model that replied badly:
/// a 300 attempt calibration run recorded 286 discards as "no fenced json
/// block", zero as "agent error", and the whole log did not contain the word
/// timeout once. A bound that fails quietly is worse than no bound, because
/// no bound at least hangs where somebody can see it.
///
/// Both framings, because they fail differently inside ureq and only one of
/// them used to arrive with the word `TimedOut` on it.
#[test]
fn a_stream_cut_off_mid_answer_is_an_error_and_not_a_short_answer() {
    bound_the_wait();
    for framing in Framing::BOTH {
        let address = sse_server(framing, move |mut socket| {
            for i in 0..3 {
                if socket.write_all(event(framing, i).as_bytes()).is_err() {
                    return;
                }
                let _ = socket.flush();
            }
            // Part way through the answer, and under chunked framing part way
            // through the framing itself, which is where ureq's decoder
            // relabels a read timeout as a protocol error.
            let _ = socket.write_all(&framing.stall_mid_frame());
            let _ = socket.flush();
            std::thread::sleep(PATIENCE * 4);
        });

        let run = stream_with_patience(address, "a provider that stopped mid answer");

        assert!(
            !run.streamed,
            "{framing:?}: a cut off answer came back as a finished one, which \
             is indistinguishable from a model that answered badly"
        );
        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: a cut off answer was not an error"));
        assert!(
            error.contains(zorp::READ_TIMEOUT_VAR),
            "{framing:?}: the error does not name the variable that buys more \
             patience, so a run that hits it looks like a bad answer: {error}"
        );
        assert!(
            error.contains(&IDLE_TIMEOUT_SECS.to_string()),
            "{framing:?}: the error does not say how long was waited: {error}"
        );
        assert!(
            run.elapsed + Duration::from_millis(250) >= Duration::from_secs(IDLE_TIMEOUT_SECS),
            "{framing:?}: gave up after {:?}, too soon to have been the idle timeout",
            run.elapsed
        );
    }
}

/// The other half of the same silence, and the one that never even waited.
///
/// A gateway that hits its own idle limit does not hold the socket open. It
/// ends the response, politely and immediately: a close-delimited body just
/// stops, a chunked one gets its terminating chunk. Nothing is wrong at the
/// transport layer, so this arrived as a successful stream carrying half an
/// answer and no error at all.
///
/// The provider never said `[DONE]` and never sent a `finish_reason`, and
/// that is the whole test. A stream that ends without either is truncated,
/// whoever closed it and however cleanly.
#[test]
fn a_stream_that_ends_without_saying_it_finished_is_an_error() {
    bound_the_wait();
    for framing in Framing::BOTH {
        let address = sse_server(framing, move |mut socket| {
            for i in 0..3 {
                let _ = socket.write_all(event(framing, i).as_bytes());
                let _ = socket.flush();
            }
            close_body(framing, socket);
        });

        let run = stream_with_patience(address, "a provider that stopped short");

        assert!(
            !run.streamed,
            "{framing:?}: a stream that stopped short came back as a finished \
             answer, so the attempt will read it as a model that answered badly"
        );
        let error = run
            .error
            .unwrap_or_else(|| panic!("{framing:?}: a truncated stream was not an error"));
        assert!(
            error.contains("finish"),
            "{framing:?}: the error does not say the provider never finished: {error}"
        );
        // The events that did arrive were still delivered. Refusing the
        // response is not a reason to pretend it never started, and the count
        // is what says how far it got.
        assert_eq!(
            run.payloads, 3,
            "{framing:?}: the events that did arrive were not delivered"
        );
    }
}

/// The guard against the fix above firing on a healthy answer.
///
/// A provider that says it has finished has finished, and both ways of saying
/// so count: the `finish_reason` on the last choice and the `[DONE]` sentinel
/// after it.
#[test]
fn a_stream_that_says_it_finished_is_not_an_error() {
    bound_the_wait();
    for framing in Framing::BOTH {
        let address = sse_server(framing, move |mut socket| {
            let _ = socket.write_all(event(framing, 0).as_bytes());
            let _ = socket.write_all(ending(framing).as_bytes());
            let _ = socket.flush();
            close_body(framing, socket);
        });

        let run = stream_with_patience(address, "a provider that finished");

        assert!(
            run.error.is_none(),
            "{framing:?}: a finished answer was refused: {:?}",
            run.error
        );
        assert!(
            run.streamed,
            "{framing:?}: the event stream was not read as one"
        );
        assert_eq!(
            run.payloads, 3,
            "{framing:?}: the reader stopped short of what the server said"
        );
    }
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
    for framing in Framing::BOTH {
        let address = sse_server(framing, move |mut socket| {
            for i in 0..PIECES {
                // A client that hung up ends this stub rather than failing it.
                if socket.write_all(event(framing, i).as_bytes()).is_err()
                    || socket.flush().is_err()
                {
                    return;
                }
                std::thread::sleep(GAP);
            }
            let _ = socket.write_all(ending(framing).as_bytes());
            let _ = socket.flush();
            close_body(framing, socket);
        });

        let run = stream_with_patience(address, "a slow but talking provider");

        assert!(
            run.error.is_none(),
            "{framing:?}: a healthy stream was cut off: {:?}",
            run.error
        );
        assert!(
            run.streamed,
            "{framing:?}: the event stream was not read as one"
        );
        assert_eq!(
            run.payloads,
            PIECES + 2,
            "{framing:?}: the reader stopped short of what the server said"
        );
        // The response outlived the idle timeout, so passing this cannot mean
        // it simply finished before the bound had a chance to apply.
        assert!(
            run.elapsed > Duration::from_secs(IDLE_TIMEOUT_SECS),
            "{framing:?}: the stream finished in {:?}, before the idle timeout \
             could have bitten",
            run.elapsed
        );
    }
}
