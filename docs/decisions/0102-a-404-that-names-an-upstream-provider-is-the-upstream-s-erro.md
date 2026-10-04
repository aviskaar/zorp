---
status: accepted
date: 2026-09-04
---

# a 404 that names an upstream provider is the upstream's error and is retried

**Finding:** three attempts on OpenRouter's free providers died today to
one code. Two had each made 26 good model calls and got this on the next
request:

```
zorp-agent: the provider reported an error inside the stream: 404 Provider returned error
```

That is an error object inside an HTTP 200 stream, zero deltas delivered,
code 404, message "Provider returned error". The third died on its very
first request to the same error as a status line:

```
zorp-agent: https://openrouter.ai/api/v1/chat/completions: status code 404: {"error":{"message":"Provider returned error","code":404,"metadata":{"raw":"","provider_name":"Nvidia","is_byok":false}},"user_id":"..."}
```

The 2026-08-23 rule says a 404 is never retried, because it is a wrong URL
or model id and a slow misconfiguration reads like a network problem. That
reasoning is about our own request, and neither of these was about our
request. The URL was found, the request was accepted and routed, and the
upstream OpenRouter picked failed. The code is the upstream's, relayed, and
the body says so: `metadata.provider_name` names the upstream on a status
line, and an event inside a stream may name it only at the top of the
chunk, as the captured 502 event does. A 404 for a model that does not
exist names no provider, because none was asked.

**Decision:** a 404 comes in two shapes and the body tells them apart. One
whose error body names an upstream provider, in `metadata.provider_name`
or as the chunk's top-level `provider`, is that provider's error and is
retried under
`Retrying`: the same bound, the same backoff, the same line on stderr, now
reading "upstream provider error". It is retried whether it arrived on the
status line or as an error event inside a 200 stream, and only while zero
deltas have reached the caller. One whose body names no provider is ours
and is never retried, in either place. `ProviderError` carries the
provider when the body named one, its Display appends "from Nvidia" so a
log says whose error it was, and `retry_reason` consults the name for 404
and for nothing else. The status path now reads the body before deciding
rather than after, and that is the whole of the change there.

**What it ruled out:** telling the two apart by envelope. The first draft
of this rule retried a 404 inside a 200 stream and not one on the status
line, and the third attempt of the day showed the same relayed error
arriving as a status. Retrying a 404 that names no provider, anywhere: it
will not get better, and the 2026-08-23 reason stands for it. Retrying 400
or 401 anywhere, provider named or not: a body the provider will not
accept and a key it will not take are ours whoever relayed them. Retrying
after a delta: the delta is on somebody's screen, and a 404 after a delta
is `InStreamError::Dropped` like any other, which the loop may re-ask and
the transport never re-sends.

**Not changed:** everything else in the 2026-08-23 entry and the two
earlier 2026-09-04 entries. One `Retrying`, one reason table, one count of
sends. 429, 502 and 503 are retried as before, and 504 stays out.
