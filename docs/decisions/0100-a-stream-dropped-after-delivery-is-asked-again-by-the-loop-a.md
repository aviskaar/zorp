---
status: accepted
date: 2026-09-04
---

# a stream dropped after delivery is asked again by the loop and never re-sent by the transport

**Finding:** on OpenRouter's free providers a stream sometimes dies after
deltas have been delivered. The line in the log is "the provider reported
an error inside the stream: 504 Upstream idle timeout exceeded; 1618 events
had already reached the caller, so the request was not sent again". Three
of nine benchmark trials died this way, each after 5 to 35 tool calls of
good work. The transport was right. The 2026-08-23 rule says a second send
after a delta would replay the start of one answer over the middle of
another, and it would. But the run loop already records nothing for a
reply that never finished, on purpose: a half-arrived message has text
that stops mid-sentence and tool calls that may be half parsed, so pushing
it would leave the transcript with calls no result answers. A step that
left nothing behind can be asked again cleanly.

**Decision:** the loop discards the dead step and asks again with a fresh
request, at most `REASKS_PER_STEP` times per step, which is two. Each
re-ask counts as a step against `max_steps`, because that bound is on
model calls made. `stream_sse` reports the case as a typed error,
`InStreamError::Dropped`, carrying the delivered event count and the
provider's error, and the loop matches on the type and not the words. It
says so on stderr each time: "the provider dropped the answer after N
events; asking again (re-ask 1 of 2)". Past the bound the run ends with
the transport's error plus a tail naming `REASKS_PER_STEP`. The transport
still never re-sends after a delta. The browser is told with an
`assistant_withdrawn` frame carrying the same numbers, takes the streamed
fragments down, and puts a one-line status where they were, so the fresh
answer does not stream onto the end of the dead one.

**Why:** three trials, and the loop already had the property that makes
this safe. Nothing is pushed for an abandoned reply, no tool has run for
it, and a re-ask is the request the model would have got anyway. The
transport cannot do this because it does not own the transcript; the loop
can because it does. The number is two because the measured failure is
one drop in a run of dozens of calls, not a provider that drops
everything, and a provider that drops everything should end the run,
loudly, rather than spend the whole step budget on one step.

**What it ruled out:** re-sending at the transport, for the 2026-08-23
reason; that rule stands. An env var for the bound: nothing has shown two
is wrong, and a knob nobody has needed is a knob nobody has tested; add
one when a run shows the number is wrong. Re-asking the other in-stream
case, an error event before any delta: `Retrying` already retries that
one under `ZORP_RETRY_ATTEMPTS`, the two are distinguishable in the type,
and a re-ask on top would multiply the bound.

**Not changed:** the 2026-08-23 rules on timeouts and retries, and the
earlier 2026-09-04 rule below on errors delivered inside a 200 stream.
Nothing is retried after a tool ran, and nothing is recorded for the dead
step.
