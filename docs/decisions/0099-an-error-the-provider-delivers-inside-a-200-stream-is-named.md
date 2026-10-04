---
status: accepted
date: 2026-09-04
---

# an error the provider delivers inside a 200 stream is named, and retried only while nothing has reached the caller

**Finding:** nine of nine benchmark trials against
`nvidia/nemotron-3-ultra-550b-a55b:free` through OpenRouter died within 1 to
11 model calls with "the stream ended before the provider said it had
finished: 1 events arrived and then the response ended, with no [DONE] and
no finish_reason, so the answer is cut off" (ZOR-26). Reproduced with curl:
HTTP 200, a streaming body, a run of `: OPENROUTER PROCESSING` comment
lines, then exactly one event and a close. The event was

```
data: {"id":"gen-1788503823-rSnjToVnWKeIBMh0SRHz","object":"chat.completion.chunk","created":1788503823,"model":"nvidia/nemotron-3-ultra-550b-a55b:free","provider":"Nvidia","choices":[],"error":{"code":502,"message":"Upstream error from Nvidia: Service temporarily overloaded","metadata":{"error_type":"provider_unavailable"}}}
```

Two things were wrong. The provider said exactly what happened and zorp
threw the sentence away: read as a chunk the event has no delta and no
`finish_reason`, so the stream counted as cut off, and the person saw "cut
off" and never "502 Upstream error from Nvidia: Service temporarily
overloaded". And a refusal delivered this way is the transient the
2026-08-23 entry retries, but that retry keys on the status line, which
said 200, so one overloaded moment killed a whole agent attempt.

**Decision:** a `data:` event whose JSON carries a top-level `error` object
is an error naming the code and the message: "the provider reported an
error inside the stream: 502 Upstream error from Nvidia: Service
temporarily overloaded". It is never handed to the caller as a payload.
While no payload has reached the caller, and the code is one
`retry_reason` retries, the request is sent again under the same two-sided
bound, with the same backoff and jitter, a `Retry-After` on the 200
honoured the same way, and a line on stderr per retry. Once any payload has
reached the caller the same event is an error and the request is not sent
again. 502 joins 429 and 503 in `retry_reason`. A 200 whose whole body is
an error object gets the same treatment on the buffered path, where nothing
reaches the caller until the body is parsed. The core's own `zorp_stream`
names the event and does not retry; it is the one-shot CLI's primitive and
a person reruns it.

**Amended by** the second 2026-09-04 entry above: the loop may ask again;
the transport still never re-sends.

**Amended by** the third 2026-09-04 entry above: a 404 whose body names an
upstream provider is retried, inside a stream or outside one; a 404 naming
none still is not.

**Why:** the 2026-08-23 rule was never about the status line. It was that a
second send must not replay the start of one answer over the middle of
another, and an error event that delivered no payload cannot do that. The
bound stays one bound: `Retrying` in the core counts sends and waits for
both the status path and the in-body path, so a stream cannot be sent
`attempts` times for each of `attempts` statuses. 502 was kept out of
`retry_reason` on the argument that a bad gateway means the upstream may
already have done the work. The measured 502 says `provider_unavailable`
and "temporarily overloaded": the upstream refused the request, which is a
503 with a different number, and OpenRouter uses it that way. 504 stays
out, because a gateway timeout means the upstream was working on it.

**What it ruled out:** retrying after a delta has been delivered, for the
2026-08-23 reason: the delta is on somebody's screen. Parsing the code out
of the message text instead of the object: the object has a `code` field,
and the message is prose that changes from one provider to the next.
Treating the event as a cut off stream, which is what it was, and which
lost the one sentence that explained nine dead trials.

**Not changed:** 400, 401 and 404 are never retried, inside a stream or
outside one, and an in-stream 400 is named and not sent again. The
two-sided bound and its defaults. The idle timeout. A stream that ends with
no error event and no `[DONE]` or `finish_reason` is still the truncation
error, word for word.
