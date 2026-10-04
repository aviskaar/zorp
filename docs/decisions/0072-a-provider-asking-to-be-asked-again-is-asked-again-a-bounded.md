---
status: accepted
date: 2026-08-23
---

# a provider asking to be asked again is asked again, a bounded number of times and out loud

**Decision:** a 429 or a 503 is retried. The bound is two sided:
`ZORP_RETRY_ATTEMPTS` sends in total, defaulting to 4, and at most
`ZORP_RETRY_BUDGET_SECS` of added waiting, defaulting to 30. A
`Retry-After` the provider sent is waited out in full; without one the
wait is exponential backoff from 500 milliseconds with jitter. Every
retry prints a line to stderr naming the status, the wait and which try
this is. Nothing else is retried, and nothing is retried once a response
body has started arriving.

**Amended by** the 2026-09-04 entry above: an error event delivered inside
a 200 stream before any delta is retried under the same bound.

**Amended by** the third 2026-09-04 entry above: a 404 whose body names an
upstream provider is retried; a 404 naming none still is not.

**Amended by** the 2026-09-05 entry above: a connection the peer closed is
re-sent before a delta and re-asked by the loop after one; a timeout still
is not.

**Why: half a run was being thrown away by a status whose own body said
it was temporary.** A 250 crate calibration run against OpenRouter's free
tier, at the 48 attempt mark:

| outcome | count |
| --- | --- |
| discarded, agent error | 25 |
| discarded, no fenced json block | 3 |
| discarded, step limit reached | 2 |
| scored | 18 |

All 25 of the agent errors were the same thing, and the provider was not
being subtle about it:

```
status code 429: {"error":{"message":"Provider returned error","code":429,
"metadata":{"raw":"stealth/ox-alpha is temporarily rate-limited upstream.
Please retry shortly.", ... ,"remedy_hint":"Retry shortly, ..."}}}
```

There was no retry anywhere in the model path. An attempt is an agent
loop of up to 40 model calls, so one 429 at call 31 discards the whole
attempt and everything it had gathered, which is the same arithmetic that
made the timeout entry above expensive.

**What is retried, and the omission worth arguing about.** 429 and 503
both mean the provider did not take the request: nothing was generated,
nothing was charged, and what it wants is to be asked again shortly.
502 and 504 are left out on purpose. Both mean the request was forwarded
and something went wrong after that, so a second send can duplicate work
an upstream may already have done and billed for, and it has no more
reason to succeed than the first did. 400, 401 and 404 are left out for
the plainer reason: they will not get better, and retrying them turns a
misconfiguration into a slow misconfiguration that reads like a network
problem.

**Why the bound is two numbers.** They stop different things. The count
stops a provider that refuses instantly and keeps refusing. The budget
stops a provider that answers `Retry-After: 600`, which is a legitimate
thing to say and not something a foreground request can honour. A
`Retry-After` that will not fit the budget ends the retrying rather than
being clamped, because waiting less than the provider asked for is worse
than not waiting: it spends a send that cannot succeed and adds load to
something already shedding it.

**Why those numbers.** Both are picked for the person watching a browser,
because a batch run can afford either and a person cannot. Half a minute
is inside the range a model answer already takes, so a turn that was rate
limited and recovered looks like a slow turn; a minute or two of a
spinner looks like nothing except broken, and at that point the retrying
is the outage rather than the cure. The batch case sets the same ceiling
from the other side: 40 model calls at half a minute each is 20 minutes
added to one attempt in a worst case nobody will see, and the measured
rate limiting is nothing like every call. Three retries at 500
milliseconds, 1 second and 2 seconds is what the default actually costs
when a provider is refusing everything, which is under four seconds.

**Jitter, because 40 calls in a row is a fleet.** An attempt is 40
sequential calls and a calibration run is several attempts at once, so a
set of callers all told "come back in a second" all come back in the same
second and rate limit each other again. Every wait is drawn from a range
rather than being a fixed number, including the small amount added on top
of a `Retry-After`, so the number given is a floor and never the instant
everybody else picked too.

**Loud, for the same reason the timeout above is loud.** A retry nobody
can see is a run that got slower with no stated cause, which is the
failure shape that cost nine hours two entries ago. One line per retry on
stderr, the channel everything else in the workspace already uses for
this, and the give-up error says how many tries it took, how long it
waited and which two variables bound it.

**Ruled out: retrying a stream that had started.** A 429 arrives before
any body, so sending again is clean. A failure part way through a stream
is not: payloads have already been handed to the caller, which in the
browser means text already on somebody's screen, so a second send would
replay the start of a fresh answer over the middle of the abandoned one.
The truncation error from the entry above stays an error, and
`retry_rate_limit.rs` has a test that counts connections to prove the
second send never happens.

**Ruled out: retrying in the agent loop instead of the transport.** The
loop could catch the error and repeat the step, but it does not know that
the step failed for a reason that will pass, and repeating a step is not
repeating a request: the model has already seen part of the conversation
change. The transport knows exactly what happened and is the only layer
that can send the same bytes twice.

**Ruled out: a separate policy for the streaming path.** `stream_sse` now
sends through `zorp::send_json` rather than through its own copy of the
core's error handling. Two copies of this is how the streaming path spent
months with no timeout at all while the buffered path had one.

**Ruled out: an event to the browser instead of a stderr line.** It would
have to be threaded from the transport, which knows nothing about
sessions, through five layers that would each have to grow a callback.
The server log is where a zorp-web operator already looks and it is
already stderr.

**What this does not fix.** Retrying does not create capacity. A free
tier that is saturated for an hour will still fail, four tries in thirty
seconds later. What it fixes is the momentary refusal that the provider
itself calls temporary, which is what those 25 discards were.
