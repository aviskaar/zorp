---
status: accepted
date: 2026-09-05
---

# a connection the peer closed is a dropped stream and gets the same treatment

**Finding:** across 35 benchmark trials on OpenRouter's free providers, 6
died with this as the last line, each after 25 to 58 tool calls of good
work:

```
zorp-agent: peer closed connection without sending TLS close_notify: https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof
```

That is rustls reporting that the peer closed the TCP connection with no
TLS close handshake. It reached `stream_sse` as an `std::io::Error` of
kind `UnexpectedEof` from a body read, which `read_error` measured
against the clock, found was no timeout, and passed up verbatim; the
line has no URL and no "Network Error" prefix, which is what a send-time
`ureq` error would carry. It is the 504 case of the second 2026-09-04
entry seen from our side of the socket: the gateway gave up on the
upstream and closed the connection instead of writing an error event
first. Nothing retried it. The 2026-08-23 rule retries a 429, a 502 or
a 503 and nothing else, and that rule was written about status codes; a
connection the peer closed has no status.

**Decision:** a transport read error that is not a timeout is a dropped
stream. The kinds are the ones a dead peer produces: unexpected EOF,
connection reset, connection aborted and broken pipe. After a delta has
reached the caller `stream_sse` reports it as `InStreamError::Dropped`,
which now carries either the provider's error object or the transport's
own words, and the loop re-asks it under `REASKS_PER_STEP` exactly as it
does for a 504. Before any delta, or at send time when `ureq` reports
the same kinds before a status line, the request is sent again under the
one `Retrying` bound and backoff, with the same line on stderr naming
the reason, "connection dropped before a reply". A timeout is still not
retried anywhere. `read_error`'s clock rule decides first on the body,
and the send path looks for `TimedOut` in the chain before it looks for
a dropped kind.

**Why:** it is the 504 case seen from our side. The loop already records
nothing for the dead step, so a re-ask after a delta is as clean as it
was for the error event. Before a delta nothing has reached anyone, so a
re-send is as clean as it is for a 429, and it costs one count against a
bound that already exists.

**What it ruled out:** retrying a timeout, for the 2026-08-23 reason: a
re-send would wait the whole limit again, and every try the bound allows
would multiply it. A second bound, a second backoff or a second count of
sends for the transport case: `Retrying` grew `again_for` and
`exhausted_for`, and the status path calls through them, so there is
still one of each. Guessing on a transport error it cannot classify:
ureq's chunk decoder reports a close that lands on the framing bytes
after a chunk as `InvalidInput` with the reason thrown away, and that
one is not retried, because a retry on an unknown transport error is the
wrong default.

**Not changed:** everything else in the 2026-08-23 entries and the three
2026-09-04 entries. The transport never re-sends after a delta. Nothing
is recorded for the dead step, and nothing is retried after a tool ran.
A stream that ends cleanly with no `[DONE]` and no `finish_reason` is
still the truncation error, word for word.
