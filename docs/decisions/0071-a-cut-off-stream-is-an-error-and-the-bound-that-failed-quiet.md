---
status: accepted
date: 2026-08-23
---

# a cut off stream is an error, and the bound that failed quietly is why

**Decision:** `stream_sse` returns `Err` for a stream that ended before
the provider said it had finished, and a read timeout says so in words
that name the timeout and `ZORP_HTTP_TIMEOUT_SECS` whatever ureq called
it underneath. `DEFAULT_READ_TIMEOUT_SECS` goes from 300 to 900.

**Why: the same corpus, the same binary, the same model, minutes apart.**

| `ZORP_HTTP_TIMEOUT_SECS` | usable forecasts |
| --- | --- |
| 180 | 0 of 20, and 9 of 300 on a longer run |
| 300 | 10 of 15 |
| 3600 | 6 of 10 |
| no bound at all (before PR #95) | 76 of 123 |

`stealth/ox-alpha` on OpenRouter, 300 crate directories, each attempt an
agent loop of up to 40 model calls. The only thing that changed between
the rows is the idle timeout.

The 300 second row was measured after this entry was first written, and
it moves the cliff rather than the conclusion. 300 is not a middling
value that loses some attempts, it performs like no bound at all, so the
whole collapse sits between 180 and 300 seconds. That says the stall tail
this model puts on a free endpoint is a few minutes wide and then stops,
which is a narrower thing than the arithmetic below assumed. 900 is still
the right default for the reason the arithmetic gives, headroom over a
tail nobody has bounded on other providers, but it is headroom and not a
rescue: the number that was actually shipped and hurt was 180, and 300
would have been fine.

**The failure was silent, and that is the part worth recording.** The
discard tally for the 300 attempt run read: no fenced json block 286,
agent error 0, agent stopped early 0, step limit reached 2. Zero agent
errors. Grepping the whole log for "timed out" or "timeout" matched only
the run script's own echoed headers. A truncated answer is still an
answer, so every one of those 286 attempts was scored as a model that
replied badly, and the cause was misattributed twice before anyone
measured it. A bound that fails quietly is worse than no bound: no bound
at least hangs somewhere a person can see it, which is exactly how the 3
hours 18 minutes that motivated PR #95 got noticed.

**Where it was being swallowed. Two places, both measured against ureq
2.12.1 with a local listener, neither of them guessed.**

First, and this is the one that produced the 286: the read loop treated
`Ok(0)` as the end of a response. It is the end of a *response*, but not
of an answer. A gateway that hits its own idle limit does not hold the
socket open, it ends the body politely, and a close-delimited body simply
stops while a chunked one gets its terminating chunk. Nothing is wrong at
the transport layer, so `stream_sse` returned `Ok(StreamOutcome::Streamed)`
carrying half an answer and no error at all. It now requires the provider
to have said it was finished, by `[DONE]` or by a non-empty
`finish_reason`, and returns `Err` otherwise.

Second, the timeout that did fire often could not say its own name. ureq
reads a close-delimited body straight off the socket, so a read timeout
arrives as `TimedOut`. It reads a chunked body through a decoder that
consumes the chunk body and then reads the framing bytes around it with
separate calls, and when one of those fails the decoder discards the
reason and reports `InvalidInput`, "Error while decoding chunks". Chunked
is what every OpenAI-compatible endpoint behind a CDN sends. So the check
is now on the clock and not on the error kind: a read that failed after
the socket had been silent for as long as the limit was the limit, and
the transport's own words are kept on the end instead of dropped.

**Why 900, and the arithmetic that was not done the first time.** An
attempt is up to 40 model calls and any single call exceeding the bound
kills the whole attempt, so a per-request stall rate of `p` leaves
`(1 - p)^40` attempts alive. One request in twenty stalling is seven
attempts in eight lost. One in ten is ninety-nine in a hundred. The 180
second row is 9 attempts out of 300, which works back to roughly one
request in twelve going quiet for longer than three minutes: unremarkable
for a reasoning model behind a gateway, and catastrophic once raised to
the fortieth power. 300 is only 1.7 times the value measured to be
catastrophic and there is no reason to think that is far enough into the
tail. 900 is five times it, and still catches the 3 hours 18 minutes that
put a bound here at all, thirteen times over. The asymmetry decides the
rest: too long costs one wait on a socket nobody is coming back to, too
short costs the run.

**The number can be wrong safely now, which matters more than the
number.** 180 did not destroy nine hours because 180 was wrong. It
destroyed nine hours because being wrong was invisible. A run that hits
900 now says the provider sent nothing for 900 seconds and names the
variable that buys more.

**Ruled out: reverting PR #95.** The hang it fixed was real and cost 3
hours 18 minutes of a run, and an unbounded read is not a bound that
never fires, it is a bound that fires after the person has given up. What
was wrong was the silence, not the clock.

**Ruled out: a whole-request timeout.** It would have to be set long
enough for the longest honest answer, which is long enough to be no use
against a dead socket, and it would cut off exactly the long answers zorp
is for.

**Ruled out: a bound on the attempt instead of the request.** That is
arguably the bound that expresses what actually died, and the agent
already bounds an attempt by steps rather than by time. It belongs to the
caller and not to the transport, and nothing has asked for it, so it is
not being added on the way past.

**Ruled out: accepting a stream with no completion signal from providers
that never send one.** A provider that sends neither `[DONE]` nor a
`finish_reason` is indistinguishable from one that was cut off, and if
such a runtime turns up the right answer is to find out and say so, not
to widen the check until it stops catching anything.

**Not changed: `zorp::zorp_stream`.** The core's own streaming primitive
has the same shape, an end of body read as the end of an answer, and it
is left alone on purpose. Its one caller is the one-shot `zorp` CLI,
where a person is watching the text arrive and a truncated answer is
visible in the moment rather than nine hours later in a tally. If that
stops being true it should be fixed there too, and this paragraph is
here so that is a decision rather than something nobody noticed.
