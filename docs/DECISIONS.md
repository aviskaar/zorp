# Decision log

A running record of product and architecture decisions made while
building zorp. Newest entries at the top. Each entry is short: what was
decided, why, and what it ruled out. Full design writeups, when they
exist, live in `docs/superpowers/specs/` and are linked from here.

Entries are never rewritten or deleted. When a later decision reverses an
earlier one, the earlier entry gets a **Superseded by** line pointing
forward and is otherwise left as it was written, so the record shows what
was believed at the time and not only what survived.

---

## 2026-08-24: conversation indexing is a quiet background loop, not a button

**Decision:** `zorp-web` starts one conversation-index worker when the
`recall` feature is enabled. It sweeps the store once after startup, repeats
every 300 seconds by default, and accepts a session as soon as a turn finishes.
`ZORP_RECALL_SWEEP_SECS` changes the interval and 0 disables automatic sweeps.
The worker serializes startup, periodic, per-session and forced passes. None of
them can interleave, and neither server startup nor a turn waits for embedding.
The existing conversation fingerprint remains the only change detector, so an
unchanged sweep makes no embedding calls.

The sidebar has no Index button. Status compares conversations in the source
store with conversations in the derived index, reports a running pass, and
reports ready only after a complete pass has caught up. It refreshes every 15
seconds while the page is visible. `POST /api/recall/index` remains for tests
and scripts that need to force a pass.

**Failure is quiet and survivable.** A missing local Ollama is normal on a
machine that has not set recall up. The first failed pass is logged, later
failures stay quiet, a recovery is logged once, and the next tick retries. The
status endpoint carries the last failure so the page can name the missing local
embedder. No fallback was added. Conversation text still goes to a checked
loopback address or nowhere.

**Cost:** every interval reads session headers and message text to check the
stored fingerprints. Unchanged text costs no model call and no vector write.
The read is accepted because it catches conversations that existed before this
server started and changes made outside a live turn, which the per-session path
cannot see.

**Supersedes:** the button-driven indexing paragraph in the 2026-08-22
conversation-search entry below. Its corpus, fingerprint, message-unit and
loopback decisions still stand.

---

## 2026-08-24: recorded voice stays on loopback and Qwen3-ASR writes only into the composer

**Decision:** browser voice input uses Qwen3-ASR through a new standalone
`zorp-voice` crate and the local `qwen-asr-serve` 0.0.6 runtime. It is behind a
non-default `voice` feature on `zorp-web`. The HTTP client carries the same
four protections as `zorp-recall`: a written-form and resolution check, a
resolver pinned to one checked host and port, redirects off, and environment
proxies off. `ZORP_VOICE_URL` and `ZORP_VOICE_MODEL` are the only overrides,
and an off-device URL is refused. The transcript goes into the editable
composer and is never sent automatically.

The runtime registers Qwen3-ASR with its pinned vLLM 0.14.0 and uses
`GET /health`, `GET /v1/models`, and `POST /v1/chat/completions`. It has no
HTTP model pull endpoint. The page shows an explicit install and start command
and polls real readiness. It shows no download percentage or stage. Model
weights download when the operator starts the runtime. Zorp constructs that
command only for a root HTTP endpoint. HTTPS and path-prefixed endpoints need
an operator-managed loopback proxy.

Transformers 5.13.1 was checked and rejected. Its `/load_model` endpoint is
real, but its generic audio transcription handler does not use Qwen3-ASR's
required processor and output parser. A working loader paired with an
incompatible transcription endpoint is not a valid runtime contract.

**Why:** voice is at least as sensitive as stored conversation text, and it
must not acquire a quieter route off the machine. Qwen3-ASR has open weights
and multilingual inference in Python, but no Rust implementation. A small
checked loopback client keeps Python and model libraries outside the Rust
workspace while preserving the privacy boundary. Putting the transcript in
the composer keeps a person between recognition errors and every agent action.

**What it rules out.** No in-process Python, torch binding, Candle, ORT,
DashScope provider, cloud fallback, flavor-manifest endpoint, automatic send,
or language fixed to English. A transcript grants no tool, changes no
approval, and bypasses no denylist. A failed local runtime makes voice input
unavailable. It never sends audio somewhere else to keep the button working.

Design:
[`docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md`](superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md).
## 2026-08-24: distribution happens at the capability boundary, over zorp-web's API, with git as the state bus

**Decision:** zorp's cluster model is a coordinator (a new `zorp-fleet`
crate) driving worker pods that each run `zorp-web` plus its agent. The
unit of distribution is a whole capability invocation on one track,
never a tool call inside a turn. The protocol is zorp-web's existing
HTTP API, versioned, plus two new worker endpoints (`/api/health`,
`/api/capabilities`). Track state moves through a central git remote
per track; workers clone, run one job, commit, and push, so the
tamper-evident git foundation doubles as the cluster's consistency
mechanism. Human checkpoints are relayed to the coordinator and decided
in one place; kill-threshold breaches still kill in code on the worker.

**Ruled out:** an MCP server mode on zorp-agent as the control protocol
(net-new server code, and request/response fits long streaming turns
worse than the SSE API that already exists), any new wire protocol
(gRPC, message brokers), distributing tool calls across machines, and
running workers with checkpoints auto-approved.

**Why:** the synchronous in-process agent loop is a feature worth
keeping, `zorp-web` already is a worker control surface (submit turn,
stream events, stop, token auth off loopback), and `zorp-track`'s
git-backed, one-attempt-per-invocation design makes the track the
natural shard. Design only; nothing is implemented yet.

**Full writeup:** `docs/superpowers/specs/2026-08-24-zorp-fleet-distributed-design.md`
## 2026-08-24: the calibration tolerance is 0.10, set from the first observed curve

**Decision:** the operating tolerance for `CalibrationReport::verdict` is
**0.10**. `Tolerance` stays a newtype with no default and no constant, so
this fixes what callers in this repo should pass and not what the type
permits.

**Why 0.10, and why only now.** `Tolerance`'s doc comment says the design
refuses to fix the number, "since it should be set from the first observed
curve rather than guessed here". Until this week there was no observed
curve. There is one now. Run 7 scored 151 forecasts from
`stealth/ox-alpha` over a crates.io corpus, above `MIN_CALIBRATION_N`, on
two bands both thick enough to judge:

    0.85..0.95   n= 39   stated=0.936   cov=0.949   gap=0.013   judged
    0.96..0.99   n=112   stated=0.976   cov=0.884   gap=0.092   judged

0.10 admits that curve with 0.008 to spare and refuses a band drifting
further. It is also what `MIN_BAND_N` was sized against: twenty rows move a
band by five points, so 0.05 is the tightest number anything here can
resolve at all, and asking for it turns a floor into a coin toss.

**What this does not license.** The number was chosen by a person looking
at a curve, which is what the design asked for, and it is not a licence to
choose a tolerance per report once its gap is known. A tolerance picked to
admit the result in front of it is the boundary problem `bin_boundaries`
exists to prevent, one level up. If a later curve says 0.10 is wrong, the
argument is made from that curve and recorded here, not applied silently
at a call site.

**What the curve says about the model, which is separate.** ox-alpha is
well calibrated below 0.96 and overconfident above it. Run 3 found the same
shape on a disjoint corpus at a different magnitude, so it is a property of
the model rather than of the sample. Whether the miss is concentrated above
a ceiling is written down as a prediction in
`docs/experiments/2026-08-24-calibration-ceiling-preregistration.md` and is
not a finding: the ceiling there was first noticed with the outcomes in
view, so it has to be confirmed on data it did not shape.

---

## 2026-08-23: a pooled connection cannot be allowed to forget its read timeout

**Decision:** `zorp::http_agent` does not keep idle connections. Every model
request opens a fresh socket, and ureq arms `timeout_read` on that socket before
it waits for response headers. `ZORP_HTTP_TIMEOUT_SECS` remains the one bound.
It is still a per-read timeout, so it bounds silence and never the total length
of a streamed answer.

**Why: a second long calibration run passed the configured bound and never
returned.** The run stopped at attempt 203 of 250. Twenty-five minutes after
its last log line the process was alive at 0.0% CPU, one worker was blocked in
`__recvfrom`, and one established OpenRouter socket had received no bytes over
a five second sample. The default 900 second timeout had been exceeded by
about ten minutes. This is the same signature that first put a timeout on the
streaming path, but the binary already contained that fix.

The missing bound was in ureq 2.12.1's pool. A new connection gets
`timeout_read` in `stream.rs`. Returning it to the pool calls `Stream::reset`
and clears both socket timeouts. `pool.try_get_connection` then gives that
socket to the next request without restoring them. The response body reader
does restore `timeout_read`, which hid the bug after headers arrived. The wait
for those headers on request two and later had no bound at all.

**Why no pooling.** A model call is much longer than a TCP and TLS handshake,
and one agent attempt can make 40 calls. Paying for those handshakes is small
next to losing the whole attempt, or a thirteen hour run, to one silent reused
socket. A two-request regression server finishes the first response so its
connection can be pooled, then accepts the second request and sends no response
bytes. The test fails against the pooled agent and requires the second call to
end near a short configured read timeout.

**Ruled out: a whole-request timeout.** It would cap the length of an honest
answer. The read timeout deliberately restarts with each read so an answer may
stream for as long as it keeps talking.

**Deferred: ureq 3.x.** Its timeout implementation was rewritten and may make
pooling safe again, but that upgrade is larger than this fix and brings a new
API and dependency tree under the workspace's Rust 1.95 floor. It should be
evaluated on its own. Reapplying the timeout to a pooled ureq 2.x socket is not
available through its public API.

---

## 2026-08-23: the calibration sample is nested, so a bigger run extends the smaller one

**Decision:** `evidence_calibration` fixes an order over the eligible
directories that does not depend on how many it wants, and takes the
first `ZORP_CAL_N` of it. The order is by a keyed hash of each
directory's path, seeded by `ZORP_CAL_SEED`, default 0. The `corpus:`
line now prints the root, the eligible count, a digest of the eligible
paths, the seed and how many were taken, which is everything needed to
run the same sample again. Nesting and determinism are tests, not
comments, and both fail against the sampler they replaced.

**Why: two published calibration numbers from this harness were not
comparable, and nobody could have told from the output.** The old
sampler took every kth eligible directory with k chosen so the stride
landed on n, so k moved when n did. Two real runs printed:

```
corpus: 3457 eligible directories, sampling every 17th for 200
corpus: 3457 eligible directories, sampling every 13th for 250
```

Of the 76 directories scored in the first and the 33 scored in the
second, exactly 1 was shared. The first reported a calibration gap of
0.107 and the second 0.000, and none of that difference can be
attributed to the model, because the two runs asked almost entirely
different questions. Raising n did not extend the sample, it replaced
it, so evidence could not accumulate and the reported number was partly
a property of the stride. That is precisely the confusion this
subsystem exists to prevent.

**What it rules out.** A plain sorted order is deterministic and nested
and still wrong: sorted paths group by crate name and therefore by
ecosystem and vintage, so the first n are one letter's worth of crates.
A seeded shuffle of the list gives an unclumped fixed order too, and was
rejected for what happens when the tree grows: a shuffle is a property
of the whole list, so one new crate in `~/.cargo/registry/src` reorders
everything and no earlier run can be extended. A key per path is a
property of that path, so a new directory takes its own place and leaves
every other pair in the same relative order. Reaching for a hash from a
crate, or for `DefaultHasher`, is ruled out for the same reason the seed
is printed: the number chooses the sample, so it has to be the same in
next year's build, and neither of those promises that.

**What is honestly not fixed.** A run reproduces against the same
corpus and no other. A directory added to the registry can still land
inside the prefix and push the last one out, which is why the count and
the digest are on the `corpus:` line: two runs over different corpora
are visibly different rather than silently so. Nothing about what is
measured changed, only which directories are chosen: the prompt, the
scoring, the discard categories and the calibration report are
untouched. `ollama_calibration` samples by truncation, which is
alphabetically biased but already nested, and is left alone.

---

## 2026-08-23: a provider asking to be asked again is asked again, a bounded number of times and out loud

**Decision:** a 429 or a 503 is retried. The bound is two sided:
`ZORP_RETRY_ATTEMPTS` sends in total, defaulting to 4, and at most
`ZORP_RETRY_BUDGET_SECS` of added waiting, defaulting to 30. A
`Retry-After` the provider sent is waited out in full; without one the
wait is exponential backoff from 500 milliseconds with jitter. Every
retry prints a line to stderr naming the status, the wait and which try
this is. Nothing else is retried, and nothing is retried once a response
body has started arriving.

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

---

## 2026-08-23: a cut off stream is an error, and the bound that failed quietly is why

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

---

## 2026-08-22: one HTTP agent, so the streaming path cannot be the unbounded one

**Decision:** the core's shared `ureq::Agent` is public as
`zorp::http_agent`, and `zorp-agent`'s `stream_sse` uses it instead of
`ureq::agent()`. So does the Ollama `/api/tags` probe in
`zorp-agent/src/main.rs`. There is one agent for model traffic and one
place that decides how long it waits: `ZORP_HTTP_TIMEOUT_SECS`, still
defaulting to 300 seconds. No second variable and no separate streaming
default.

**Why:** a 200 sample calibration run against OpenRouter wedged. The
process sat at 0% CPU for 3 hours 18 minutes holding an ESTABLISHED
connection to the provider, produced nothing, never recovered and had to
be killed; it had finished 123 of the 200 attempts. The buffered path was
configured all along, 30 seconds to connect and 300 to read. The
streaming path built ureq's default agent, which has neither. Every real
model call streams, so the one path that could not hang was the one
nothing used, and the failure was invisible for as long as providers
behaved.

**A read timeout is the right bound here because ureq applies it per
read.** On a streamed body that makes it an idle timeout: it bounds
silence between chunks and says nothing about total length, so an answer
that keeps producing tokens for an hour is untouched while a socket that
stops mid-sentence is not. A whole-request timeout would have to be set
long enough for the longest honest answer, which is long enough to be no
use against this.

**The comment that dismissed this is now the record of it.**
`stream_sse`'s doc said that a model producing an answer sends something
several times a second, so a quiet socket was not the case worth
covering. That was an assumption written as a fact, and it is the
assumption that failed. It now says what happened.

**Ruled out: a second knob.** A `ZORP_STREAM_TIMEOUT_SECS` beside
`ZORP_HTTP_TIMEOUT_SECS` would let the two paths drift apart by
configuration rather than by oversight, which is this bug again with a
config file in front of it.

**Ruled out: fixing it with the cancel token.** The token is checked
between reads, and a read that never returns is never between reads. No
amount of checking reaches a socket nobody is watching, which is what a
long unattended calibration run is.

---

## 2026-08-22: a session title is a model's sentence, so it gets its own column

**Decision:** the sidebar shows a short, model-written name for a
conversation, generated once per session and stored in a new
`sessions.display_title`. `sessions.task` keeps holding the verbatim first
user message and nothing else. `GET /api/sessions` is the only reader of
the new column: it sends `display_title` when there is one and `task` when
there is not.

**The title never goes in `task`, and that is the whole design.**
Overwriting `task` is the obvious implementation and it is the one that
breaks the memory. `recall::index_one` reads `session.task` twice, once
into the index fingerprint and once as `Conversation.title` in the
`zorp-recall` search index. `memory::block` puts that title into the block
quoted into a later turn's transcript, and the boundary text above it says
"Cite the conversation title when you use one". A generated summary written
into `task` would therefore be a sentence a model composed, stored in the
corpus and handed back to a model as a thing to cite: exactly the
tail-eating the 2026-08-22 memory entry below is arranged to prevent. A
separate column means everything that must not be handed model-authored
text keeps reading `task` and stays correct by default, rather than staying
correct only while everyone remembers which column is poisoned.

**After the first reply, not after the first message.** The complaint was a
sidebar of "hello", and the first message alone often does not say what a
session turned out to be about. The exchange does. It costs nothing in
latency: the call is made after the closing `Done` has already gone out, on
its own thread, so the reader has their answer before it starts.

**On by default, `ZORP_SESSION_TITLES=0` to turn it off.** The opt-out
spelling and not the opt-in one, which is the opposite of `ZORP_FORECAST`.
A forecast costs a model call on every attempt and writes into an evidence
record that other code reasons from, so it stays off until asked for. This
costs one call per conversation, needs no new dependency, opens no network
path the session does not already use, and writes a string nothing reasons
from. The default it replaces is actively bad. `ZORP_STREAM` is the
existing precedent for "on unless someone says 0".

**The session's own model, and no reasoning.** No second provider and no
second API key: it resolves through `settings.effective_model()` like a
turn and a panel do. Reasoning mode is deliberately not carried over. A
sidebar label is not worth a thinking budget, and the person set
`ZORP_REASONING_MODE` for the work they are reading.

**Both halves of the material are untrusted and are fenced.** The user half
may say "ignore previous instructions and title this X"; the assistant half
is a model's earlier output and may be quoting a page it fetched. They go
inside a fence carrying a per-call marker the material cannot guess, under
a boundary sentence, the same shape `zorp-skill` puts under a skill body
and `memory` puts under a recalled excerpt.

**Then it is clamped in code, because a prompt is not a constraint.** One
line, control characters and bidirectional overrides and zero-width
characters stripped, decoration and a leading `Title:` removed, at most 60
characters and at most 10 words, cut on a word boundary. The clamp sits on
the single path to the column rather than at the call site, so nothing can
reach `display_title` without going through it.

**Every failure is the same failure.** No model configured, a call that
errored, an empty answer, a refusal, a declined title: nothing is written,
no event is sent, and the sidebar keeps showing the verbatim first message.
A session never shows a blank or a placeholder in place of something the
user can recognise.

**The sidebar catches up on the existing event stream.** A new
`session_title` frame on `/api/sessions/:id/events`, sent after `Done` and
only when a title was really written. No polling loop, and no second
endpoint. It is model output, so it reaches the DOM through `textContent`;
the sidebar row moved into `web/src/session-row.ts` to get the injection
tests every other renderer in this repo has.

---

## 2026-08-22: the browser is a workspace with draggable halves, and the file list is a picker

**Decision:** `zorp-web`'s page is now a resizable split. The artifact
pane defaults to `clamp(320px, 42vw, 760px)` instead of a fixed 420px, so
a document gets a real half of the window. Both side panes have a drag
handle on their inner edge that writes `--sidebar-w` or `--artifacts-w`
and saves the result to `localStorage`. The sessions sidebar collapses to
nothing and comes back from the topbar hamburger, and that survives a
reload too. The file listing left the artifact pane for a popover under
the existing Files button, and the pane's header now names the file it is
showing. The limits and the persistence live in `web/src/layout.ts`, apart
from `main.ts`, so they can be tested without a page.

**Why:** the pane was a strip down the side with a file list eating the
top third of it, and the third it ate was the part a document wanted. A
listing is a picker, and a picker belongs on the control that opens it.

**No pane may be dragged over the conversation.** Every clamp is computed
against `MAIN_MIN`, which the conversation keeps whatever the two side
panes are doing. A layout control that can squeeze the thing being read
down to a gutter is a way to break the page by accident and then not know
how to undo it.

**The handle answers the keyboard.** `role="separator"` with a tabindex,
arrow keys, Home and End, and Enter or a double click for the default
back. A resize only a pointer can do is a resize some people cannot do.

**Collapsing hides the sidebar, it does not remove it.** A grid item set
to `display: none` stops being an item, so the conversation auto-places
into the collapsed 0px column and vanishes with it. `visibility: hidden`
keeps the slot at zero width and still takes the sidebar out of the tab
order and the accessibility tree.

**Collapse is a wide-layout idea only.** Under 820px the sidebar is
already a drawer, so closing it there shuts the drawer and records
nothing; otherwise a phone user shutting a drawer would find their
sidebar gone on the next wide window.

**Nothing here assembles HTML.** Filenames in the popover and in the pane
header are workspace data, so they go through `textContent` like every
other thing on this page the agent had a hand in.

---

## 2026-08-22: a PDF in the artifact pane is a PDF, and the isolation is the response header

**Decision:** `GET /api/artifacts/raw` sends a `.pdf` as
`application/pdf` again, and the pane frames it for the browser's own
viewer to draw. The frame it goes into carries no `sandbox` attribute.
What isolates it is the response:
`Content-Security-Policy: sandbox allow-scripts`, one token wider than
the bare `sandbox` every other served type still gets. The text read out
of the file stays, as the fallback, behind `?as=text`.

Showing the extracted text was the previous answer and it was reported as
a bug, correctly: a run that produces a paper produces a paper, and a
pane that shows the words out of it has thrown away the layout,
the figures and the page count. The reason it was doing that is in the
2026-08-17 pane design and in the comments that were sitting on this
code: an earlier attempt pointed the existing sandbox frame at the raw
endpoint, got a broken-document icon on grey, and concluded no browser's
viewer starts in a sandbox. Half right. The viewer does not start with
scripting off, and that is the only thing wrong with the old
configuration.

Measured in Chrome 151 on macOS, one PDF under six header and attribute
combinations, captured over CDP with `fromSurface` because neither
`captureVisibleTab` nor headless `--screenshot` composites the viewer at
all and both come back a plausible empty grey:

- iframe `sandbox=""`, response `sandbox`: broken-document icon. This is
  what was shipped and what was reported.
- iframe `sandbox="allow-scripts"`, response `sandbox allow-scripts`:
  broken-document icon.
- iframe `sandbox="allow-scripts"`, no response CSP: broken-document
  icon. So it is the attribute, not the header, and no value of the
  attribute rescues it.
- no attribute, response `sandbox allow-scripts`: the viewer starts. Two
  page thumbnails, a toolbar, the text on the page.

The security question that leaves is whether the header alone isolates,
and that was measured too rather than read off the spec. A hostile page
served under exactly `sandbox allow-scripts`, framed with no attribute:
`parent.document`, `parent.location` and `localStorage` each throw
`SecurityError`, `window.origin` is `null`, and the framing page's title
is untouched. `allow-same-origin` is the token that would undo all of
that and it is not there.

Two things carry the rest. `served_as` gives a PDF its own variant rather
than making it a third `Sandboxed` one, so the widened policy cannot
spread to the two types that really do execute, and the test that pins
the sandboxed list now pins the framed list beside it. And
`content_type(path)` became `response_type(path, form)`, because the type
of a file and the type of a response stopped being the same question the
moment a PDF had two answers at one URL: keying the header off the
extension alone is how a body full of extracted text ends up labelled
`application/pdf`.

The fallback is real and has two triggers, neither of which is a guess.
The pane frames a PDF only when `navigator.pdfViewerEnabled` is `true`,
so a browser that says no, or is too old to have been asked, gets the
words instead of an empty pane. And the server checks for a `%PDF-`
header before handing bytes to a viewer, because a model writing markdown
into `report.pdf` is an ordinary Tuesday and the viewer's answer to that
is the same broken-document icon this entry is about.

**Ruled out:** bundling pdf.js and rendering to a `<canvas>`. It would
work in every browser, and it is easier to screenshot, which given the
capture trouble above was a real temptation. But it parses hostile PDF
bytes in this page's own origin, which is the one thing
`web/src/markdown.ts` and everything around it exists to prevent, and
CVE-2024-4367 is what that costs when it goes wrong. It also puts about a
megabyte and a half of runtime dependency into a `web/` that has none at
all today, and a worker that cannot be loaded from an opaque origin
anyway. The browser's viewer runs in a process this page cannot reach,
and that is a stronger boundary than any bundle.

**Not verified:** Safari. There is no Safari automation on this machine
and the claim is not made. Desktop Safari reports
`navigator.pdfViewerEnabled` and has a viewer, so the framed path is what
it should take; if it does not render, the capability check is the wrong
question and the fix is to widen the fallback, not the sandbox.

See `docs/superpowers/specs/2026-08-17-artifact-pane-design.md`, whose
PDF section described this design and whose one wrong sentence, that the
bare `sandbox` CSP is what keeps a hostile PDF away from the page, is
corrected here.

---

---

## 2026-08-22: the calibration harness counts every attempt it samples, and prints why it dropped each one

**Decision:** `zorp-agent/tests/evidence_calibration.rs` accounts for
every sampled directory. Each attempt is either scored or discarded into
one of eight named categories, a `Tally` counts and prints in the same
call so neither can happen without the other, and the run asserts that
scored plus discarded equals sampled before it prints a calibration
report. The per attempt work moved into `record_attempt`, which takes
the store write as a closure, so the whole accounting path runs in tests
with no model and no database.

**Why:** the 60-directory registry run reported 25 discards. Three
discard paths printed three different phrasings and the summary used the
word from only one of them, so a grep for that word found 19 and the
other six looked like silent losses. They were not lost. One was a step
limit and five were dropped connections to the model endpoint, printed
as `no answer (...)`. A count nobody can check against the lines above
it is a count nobody can act on, and this run exists to produce a
defensible go/no-go.

**Categories report zero rather than disappearing.** PR #86 fixed both
parser reasons, so `no fenced json block` and `not the shape asked for`
should fall to zero. A category that vanishes when it stops firing takes
the evidence of the fix with it.

**A discard is never a way to raise the pass rate.** Nothing invents an
interval, an unreadable answer contributes nothing to `n`, and
`a_discarded_attempt_never_becomes_a_scored_one` says so.

**What a discard now carries:** an excerpt of the model's answer, next
to the reason. The two bugs #86 fixed could not be replayed because only
the error survived. That text is model authored, it goes to stdout and
nowhere else, and it is never written to the store, so no detector and
nothing in the search layer can read it back.

---

---

## 2026-08-22: a band too thin to judge is its own no-go, and never a miss

**Decision:** `CalibrationReport::verdict` now judges a band only when the
band carries enough forecasts for its coverage to mean something. A band
under `required_band_n(confidence)` comes back as
`NoGoReason::BandTooThin { confidence, n, required, observed_coverage }`
and never as `BandOutOfTolerance`, whatever its gap. The bar is
`max(MIN_BAND_N, ceil(1 / (1 - confidence)))`, with `MIN_BAND_N = 20`.
`MIN_CALIBRATION_N` is untouched and still applies to the report as a
whole. Both reasons block the go. Thin is not a pass.

**What was wrong.** `CalibrationBand` has carried `n` since it was
written and `verdict` never read it, so every band was judged against
the tolerance no matter how few rows it held. A real 60 directory run
against `stealth/ox-alpha` produced six bands of between three and
eleven forecasts, and the three row band at a stated 0.96 with one
outcome covered came back as `BandOutOfTolerance` with a gap of 0.627 at
every tolerance up to 0.20. Three rows can only read 0, 1/3, 2/3 or 1,
so the nearest coverage that band can reach to 0.96 is a perfect score,
and a forecaster who is exactly right fails it 11.5% of the time on the
arithmetic alone. The report was manufacturing findings its own data
could not carry, in a shape indistinguishable from a real miss. The
design already refuses to answer on thin evidence, which is what
`MIN_CALIBRATION_N` is; the same reasoning had simply never been applied
one level down.

**Why two floors and not one.** `1 / (1 - confidence)` is the size at
which a perfectly calibrated forecaster expects one miss in the band.
Under it, the band cannot express any coverage between what it states
and a perfect score, which is exactly the arithmetic above. It scales
with the claim, and it has to: 0.96 needs 25 rows, 0.98 needs 50, and
0.99 needs 100, while a flat number would wave all three through. It is
useless at the low end, though, where it asks for two rows at a stated
0.50, so `MIN_BAND_N` holds that end. Twenty is where one row moves the
observed coverage by five points, the tightest tolerance anything here
asks for. It is deliberately under `MIN_CALIBRATION_N`: at fifty, no
report short of the overall minimum could ever hold a band that met the
per-band one, the overall check would become unreachable except for an
empty report, and the test guarding it would stop guarding anything.

**Rejected: a Wilson interval as the test.** Failing a band only when
the interval on its observed coverage excludes the stated confidence is
the statistically standard move, and it makes the go easier, which
settles it. The existing boundary test has 42 of 60 covered at a stated
0.75, whose Wilson interval is [0.575, 0.801] and contains 0.75, so a
band the caller's own tolerance rejects would pass. A correctness fix
for over-reporting must not quietly start under-reporting.

**Rejected: the false alarm rate of the tolerance test itself.** The
honest question is how often a perfect forecaster fails this band at
this tolerance, computed from the binomial. It was written out and
measured before being dropped. It ties thinness to the tolerance, so
tightening the tolerance turns real misses into "too thin", which is
backwards. It is brutal on the existing suite: the 0.75 band above has a
29.6% false alarm rate at a tolerance of 0.05, the central Go fixture at
n = 50 sits at 4.93% against a 5% bar, and the two decisive misses in
`each_band_out_of_tolerance_gets_its_own_reason` (20 of 45 at a stated
0.80, 10 of 45 at 0.95) would be suppressed even though both are
decisive by any test. A rule about the null hypothesis suppresses
findings about the data.

**What changed in what the report says.** Newly raised: any band under
the bar, including ones that used to pass silently, so a report made
entirely of thin bands can no longer return Go. That was the hole. Newly
suppressed: `BandOutOfTolerance` on a thin band, including where the
miss is genuinely decisive. The observed coverage rides along in the new
reason so nothing is hidden, and the caller keeps the whole band list.
What the report refuses to do is call a three row gap a finding.

**A floor, not a proof of power.** Twenty rows at a stated 0.80 still
fails a tolerance of 0.10 about 16% of the time when the forecaster is
exactly right. Getting that under 5% takes fifty rows in that band
alone. This fix removes the bands where the coverage grid is coarser
than the verdict, and it does not certify that the verdict has power
above the bar. A caller who wants that can compute it: `n` and `covered`
are on every band.

**The go got harder, and that is checkable.** A band is silent now only
if it was silent before and clears the bar, so the reason list can only
grow. Every existing test still passes unchanged, including the mutation
test that says deleting the `n < MIN_CALIBRATION_N` check returns Go,
which still catches its own deletion because a band of 49 at a stated
0.80 is judgeable. Deleting the new per-band check, dropping the
`1 / (1 - confidence)` half of the bar, or widening the comparison to
`<=` each fail a test that exists for it.

---

## 2026-08-22: a calibration band is a bin of adjacent confidences, sized by what it can judge

**Decision:** `CalibrationReport` no longer makes one band per distinct
stated confidence. Rows arrive ascending by stated confidence and are
gathered into bins until a bin holds `required_band_n` of its own mean,
at which point it closes. A band's `confidence` is the mean of the
confidences its forecasts stated, not a bin edge, because the comparison
has to be against what was actually claimed. `CalibrationBand` gained
`parts`, one entry per distinct stated confidence pooled into the bin,
so a reader can see everything that went into that mean. Two rules hold
the binner up. A whole group of identical stated confidences moves
together, so no boundary depends on the order rows were written in. And
a leftover tail too small to close joins the bin before it rather than
being dropped.

**What was wrong.** `required_band_n` arrived the same day and is right,
but nothing had asked what it does to a forecaster that writes free-form
numbers. A registry run against `stealth/ox-alpha` produced 35 scored
forecasts on six confidences between 0.93 and 0.99, the thickest holding
eleven. Every band was `BandTooThin`, so nothing at all could be
concluded, and the run would have needed roughly five hundred usable
attempts to fix that. The data was not the problem: pooled, those same
rows say a stated 0.968 against an observed 0.886, a forecaster
overconfident by about eight points, which is a finding. The report was
refusing to state a conclusion its own rows carried, purely because of
where it had drawn its lines.

**Why not fixed bins.** They were tried first. Bins of `[0, 0.7)`,
`[0.7, 0.85)`, `[0.85, 0.95)`, `[0.95, 1)` split that run into three and
thirty two, and the bin of three is `BandTooThin` at every tolerance
forever. One stray forecast at 0.93 would hold the verdict hostage for
the life of the record. A grid fixed in advance cannot know where the
rows will land, so it is a bet on the forecaster's habits.

**What this makes easier, said plainly.** A go that no tolerance could
reach is now reachable, so this is a loosening, and a loosening of a
measurement needs an argument. The argument is that what it removes is
silence rather than a finding: every band in the motivating run was
`BandTooThin`, which is the report saying it has not measured anything.
A group carrying the rows its own requirement asks for still closes its
own bin, so a demonstrated miss is not pooled away by a neighbour that
could have stood alone.

**What it can still hide.** Pooling averages, and the pooled gap is the
row-weighted mean of the gaps that went into it. Two groups miscalibrated
in the same direction cannot cancel, and uniform overconfidence, the
failure this whole report exists to catch, is exactly that case. What can
cancel is a bin holding a group that over-covers and a group that
under-covers, and there the smaller group can carry real weight: 19 rows
at a stated 0.50 that all landed inside, pooled with 20 at a stated 0.90
covering 0.60, read as a gap of 0.09 where the second group alone would
have been a miss. Three things bound it. Bins are the narrowest the
requirement allows, so only a group that arithmetically cannot be judged
alone is ever absorbed. `parts` puts every pooled group on the page with
its own `n` and `covered`, so a pass is auditable rather than asserted.
And no scheme that never drops a forecast can avoid pooling the groups
that cannot be judged; the alternative is to drop them, which biases the
curve silently and is worse.

**What is not negotiable.** The boundaries are computed by
`bin_boundaries`, which is handed the stated confidences and nothing
else. A boundary chosen with the outcomes in view would be fitted to the
answer, and the coverage it then reported would be a property of the fit.
A test flips every outcome and asserts the bins do not move; the
signature is what makes that true and the test is what keeps it true.
Every scored row lands in exactly one bin, with a test that the bins' `n`
sums to the report's `n`. And the binner closes a bin on exactly the
predicate `verdict` judges it by, through the same `required_band_n` and
the same mean, because a bin the binner called finished coming back as
`BandTooThin` would be the report contradicting itself.

**What did not change.** `verdict` is untouched. `MIN_CALIBRATION_N`,
`MIN_BAND_N`, `required_band_n`, the thinness check before the tolerance
check, the NaN handling before every comparison, and the empty report
answering `NotEnoughEvidence` all still hold, with their tests. A report
too small to fill one bin is one band that `verdict` calls
`BandTooThin`, exactly as before.

**Worth knowing.** Written out, the closing rule `n >= ceil(1 / (1 -
mean))` is just `sum(1 - c_i) >= 1`: a bin is thick enough when a correct
forecaster expects at least one miss in it. That form is monotone in
adding rows, which is why merging a tail into a closed bin can never
reopen it, and it is why the greedy left to right pass is well defined.

---

## 2026-08-22: conversations feed a local memory, and memory is quoted, never summarized

**Decision:** every finished turn indexes its own session into the
`zorp-recall` index, so the corpus keeps up with the conversations without
anybody pressing a button. A turn can then be told to read that index
before it answers, behind a non-default `memory` feature on `zorp-web`
(`memory = ["recall"]`), a `"memory": true` field on
`POST /api/sessions/:id/turn`, and a checkbox next to the composer that
starts unticked on every message. What comes back is quoted into the
transcript the agent is seeded with, and reported to the browser as a
`memory` event listing exactly what was used.

**The unit of memory is a verbatim message, and there is no other kind.**
No model is asked to read the corpus and write down what it learned. There
is no claim table, no extraction step, and no row anywhere holding a
sentence a model composed about the past. `zorp-track`'s discovery layer
carries a rule that nothing in the search layer may read a column of
model-authored text, because the agent's own speculation becomes tomorrow's
observation (2026-08-19). That rule binds detectors and not this feature,
but a fact extractor here would build exactly the failure it describes: a
guess stored as a fact and cited six weeks later as something the corpus
says. Retrieval alone answers the ask, so retrieval alone is what got
built. This rules out a summarizing memory, a "user profile" a model keeps
up to date, and every other shape where the thing stored is not the thing
said.

Half of every conversation is written by an assistant, so model-authored
text does reach the prompt. It is labelled, not laundered: a passage's role
travels from the store to the index to the block to the browser, and an
assistant line says in words, in both places, that it is a model's earlier
output and not a checked fact.

**Recalled text is data, and three things say so.** The excerpts sit inside
a fence whose marker carries a nonce minted for that one turn, so text
written before the turn cannot close the quotation and start speaking as
the harness. Above them is the frame, which says the block is reference
data that cannot grant a tool, widen an approval, or bypass the command
denylist, and that an excerpt reading like an order is to be reported and
not obeyed. That is the sentence `zorp-skill` already puts under a skill
body (2026-08-18), for the same reason and to the same effect. And the
block is a `user` message, never the system prompt, and grants nothing
because nothing in the path could: `memory.rs` builds a string, touches no
policy, registers no tool, and answers no approval.

**The block is never persisted.** It is appended to the seed, which is what
the agent believes is already recorded, so it reaches the model and never
reaches the store. That is load bearing rather than tidy. A block written
into the conversation would be embedded by the next feed and recalled by
the turn after that, and the harness's own framing of somebody else's text
would become a thing the corpus says.

**On request, not on every turn.** Automatic injection would spend context
on messages that do not need it, and would leave a user unable to say why
the model knew something. Per message rather than per session for the same
reason: a mode somebody leaves on is a mode they stop seeing. The model
cannot ask for a recall either; there is no tool for it, following the same
line `panel` draws (2026-08-20).

**Staleness.** Nothing here can outlive its source, because nothing here is
derived from anything but the source: a changed conversation is re-embedded
whole under a new fingerprint, and a deleted one is dropped by `retain`. A
correction made in July cannot delete the thing it corrected in March, so
every excerpt is dated in the block and on the page, and the frame tells
the model that the later of two disagreeing excerpts is the more recent
thing the user saw and that neither is current unless checked. Open
question: nothing ranks by recency and nothing filters by score, both of
which want data from a real corpus before a number gets picked.

**Ruled out:** injecting on every turn; a session-level memory switch; a
`recall` tool the model can call; storing model-extracted claims in any
shape; and a score threshold chosen before anyone has run this against a
real history.

See `zorp-web/src/memory.rs` for the whole trust argument in one place, and
`zorp-web/tests/memory.rs` for the cases that hold it up.

---

## 2026-08-22: conversations are searchable by meaning, and the vectors never leave the machine

**Decision:** `zorp-recall` is a new workspace member holding a loopback
guard, an embedder that talks to a local Ollama, and a SQLite vector index.
`zorp-web` gains `GET /api/recall/status`, `POST /api/recall/index` and
`GET /api/recall/search` behind a non-default `recall` feature, and the
sidebar gains a search box. The corpus indexed is `zorp_agent::Store`, the
conversations the browser already lists.

**Conversation text goes to a loopback address or it goes nowhere.** There
is no remote embedding provider, no flag that adds one, and no fallback when
the local model is missing. That is the whole decision and everything else
here is in service of it. This corpus is a person's entire history with an
agent that has been reading their files, so a capability that quietly keeps
working by posting it to an API is not a degraded version of this feature.
It is the worst thing the code could do, and a silent fallback is the shape
that failure always takes.

**Four things enforce it, and they are layered on purpose.**

1. `LoopbackUrl::parse` is the only way to name an endpoint. The written
   form has to be a loopback IP literal or exactly `localhost`, because a
   substring test for "127.0.0.1" accepts `127.0.0.1.evil.example`, which is
   a name somebody else owns. The name is then resolved once and every
   address it yields has to be loopback, which catches a `localhost` pointed
   elsewhere in `/etc/hosts`. A name that answers with one loopback address
   and one that is not is refused whole rather than filtered down to the
   safe half.
2. The addresses from step 1 are kept, and `LoopbackResolver` is the only
   thing the HTTP client can resolve through. It performs no lookup of its
   own and answers for exactly one host and port. Checking a name and then
   letting the client look it up again is a check with a gap in the middle,
   and the gap is where the answer changes.
3. `redirects(0)`. A 302 is a request to send the same body somewhere else,
   chosen by whatever answered.
4. `try_proxy_from_env(false)`. `ureq::AgentBuilder::new` turns proxy
   detection on when the `proxy-from-env` feature is enabled, Cargo unifies
   features across the whole graph, so another crate can enable it without
   this one asking. `HTTP_PROXY` on a managed laptop is somebody else's
   server. Note `ureq` routes a proxied connection through the resolver too,
   so step 2 covers this even when step 4 is wrong.

**The tests count connections, not errors.** A request that failed and a
request that was never made look identical from the caller's side, and only
one of them is the guarantee. Each case points a would-be escape at a
loopback socket that counts what reaches it and passes only at zero. Removing
`redirects(0)` and the resolver together was checked to make those cases fail,
so they have teeth.

**Ollama over loopback HTTP, not an in-process model.** `fastembed` and
`candle` are genuinely self-contained, and they are a large tree in a
workspace that pares `zip` down to two codecs and pins an MSRV job to stop
the floor drifting. They also download weights from a model host on first
use, which is a network call, a large one, and one this feature would have to
explain. Ollama is already the first entry in the settings panel's provider
list and already has a test driving it. The cost is an external process, and
the honest answer to not having one is the refusal, not a hosted API.

**SQLite, not the LanceDB library in `zorp-track`.** Reuse was the reflex and
it is wrong three times over. `Library` is keyed by track id because it holds
an investigation's evidence, and chat history is not evidence: the 2026-08-17
entry already drew that line for open-context. The `library` feature is
opt-in specifically because it pulls the whole Arrow tree, and this would pull
it into the web binary. And `rusqlite` with `bundled` is already linked by
`zorp-agent`, the crate that holds the conversations, so the entire capability
adds no crates to the tree at all. The scan is brute force for the same
reason: 93 conversations is 145 vectors, a dot product over that is
sub-millisecond, and an approximate index is a data structure, a build step,
and a recall tradeoff bought for a problem nobody has.

**Indexed on request, incrementally, one vector per message.** Embedding on
write would put a model call in the path of sending a message and make the
chat depend on Ollama being up. Embedding on first search would put minutes
behind a text box. So it is a button, and the button skips any conversation
whose text hash has not moved. A message rather than a whole conversation,
because a conversation averaged into one vector is a vector about nothing in
particular, and because a per-message hit gives the result list a line to
show. Tool results are left out: they are the largest thing in most sessions,
they are mostly files the agent read on the way to an answer, and indexing
them would fill the results with the same file in nine conversations.

**Ruled out:** a relevance floor. Measured against a real local model,
unrelated conversations score 0.47 to 0.58 and the right one scores 0.74, so
any fixed cutoff would be a number invented to look decisive. The list is
ranked and capped instead, weakest last.

**Known limit:** between resolving a name and connecting to it there is a
window the resolver closes for the destination but not for the operating
system's own routing. Nothing here defends against a machine whose loopback
interface has been reconfigured, and nothing can.

---

## 2026-08-21: Zorp mode is a browser-driven investigate, not a new capability

**Decision:** the browser gets a "Zorp mode" button. It runs one
pre-registered `investigate` attempt on the session, streams it on the event
stream a turn already uses, and then reads back what landed in the aryabhatta
ledger. Three routes: `POST /api/sessions/:id/investigate`,
`GET /api/investigate/status`, `GET /api/investigate/ledger`. All of it is
behind a new, non-default `research` feature on `zorp-web`.

**Why it is not a fifth capability.** The ask was to "call the aryabhatta
engine". There is no aryabhatta engine. aryabhatta is record plus readers, nine
modules inside `zorp-track`, and it ships no command on purpose. What writes to
it is `investigate`. So the faithful browser-facing shape is one `investigate`
attempt plus a read, and inventing an engine to sit in front of the record
would have added the one thing the design says not to add.

**It mirrors `panel`, deliberately.** An attempt occupies the session exactly as
a turn does, shares the session's sequence counter, answers the existing stop
endpoint, and closes with `investigate_done` then `done`. That is the shape
`panel` already established for a non-turn operation driven from the browser,
and a second transport would have given the page a third state machine to keep
in step with the first two.

**A person launches it and no model can.** The same rule `panel` holds, for a
sharper reason: an attempt writes to a pre-registered evidence record and to the
ledger, so a model that could start one could feed the record it is later read
against. There is no tool that reaches the endpoint, `agent.rs` carries a test
asserting the unfiltered builtin set has none, and `zorp-web` carries a second
over the set this server hands out.

**The feature is opt-in and the routes exist either way.** `research` on
`zorp-web` pulls in `zorp-track` and DuckDB, which is the most expensive build
in the workspace, and the ordinary chat server has no use for it. The routes
are registered unconditionally and answer 501 without the feature, because a
404 cannot tell "this server does not do that" from "you typed the URL wrong",
and `GET /api/investigate/status` lets the page say which one it is before
anybody clicks.

**Checkpoints are auto-approved, and that is stated on the page.** There is no
terminal behind a browser, so `CheckpointMode::terminal` refuses outright. This
is the CLI's `--yes`, chosen explicitly rather than fallen back to. What it
cannot do is skip the pre-registered kill threshold: a breach kills the track
unconditionally without consulting the checkpoint mode at all, so the
commitment still holds from the browser. What is missing is the human
judgement call on top of it, and the gap is recorded rather than hidden,
because `checkpoint_mode` is one of the conditions every attempt writes and it
reads `auto-approve` in the ledger. Ruled out: a browser checkpoint decider on
the existing approval card. It would have meant an unanswered prompt killing a
track after five minutes, which is a product decision worth taking on purpose
rather than as a side effect of building this.

**Forecasting is reported and never set from the browser.** The status endpoint
says whether `ZORP_FORECAST` is on where the server runs. There is no control
that turns it on, because it costs a model call on every attempt and one page
flipping it would change what the server does for everyone using it. Off stays
the default, and the ledger view says out loud that an empty expectations list
is why nothing can be scored.

**The ledger view is a display reader and names no model-authored column.**
Integrity rules 5 and 7 bind detectors and the search layer, not a page, but
the cheapest way to keep them checkable is for no read path anywhere to name
such a column. So `expectations.assumptions` is not in the frame, and a test
asserts the serialized ledger never carries it. Nothing read back is fed to a
model either: the lines on the page are recorded rows and arithmetic over them,
which is the same split `critique` and the detectors already use.

**Open.** The ledger is read by question rather than browsed, so there is no
way to see a track you did not just run. That is deliberate for now: a browser
that can list every track is a different feature with its own scope.

---

## 2026-08-21: the browser is told what search it has, and is never left to guess

**Decision:** `zorp-web` gained a non-default `search` feature
(`search = ["zorp-agent/search"]`) and a read-only route,
`GET /api/capabilities`, that reports whether the `web_search` tool is
actually there. The chat UI draws a small pill in the topbar when the answer
is yes and nothing at all when it is no. The pill is a report, not a switch.

**Why the feature:** `zorp-web` depended on `zorp-agent` with no features, so
`web_search` could never register under the browser however the environment
was set up. Adding it as a default would have been the easy fix and the wrong
one: `search` is the only built-in that sends anything off the machine, it is
opt-in on `zorp-agent` for exactly that reason, and `research` deliberately
does not pull it in either. Starting a local web UI should not acquire an
egress path by side effect. Run `cargo run -p zorp-web --features search` when
you want it.

**Why a server answer rather than a guess in the page:** three separate things
decide whether the tool exists, and the browser can see none of them. Whether
the binary was built with the feature is a fact about the build. Whether the
policy permits the tool is a fact about the code. Whether the search provider
found its key is a fact about the environment the server was started in, and
it can change without a restart, so the question is answered per request
rather than cached at startup.

**The answer is observed, not re-derived.** `web_search_availability` in
`zorp-agent` shares one function with the registration site, so the two cannot
drift, and it reads the real `Policy` rather than a copy of its reasoning. The
test that matters asserts the reported answer against `tool_names()`, which is
registration itself. A hand-written copy of the three conditions would have
been right on the day it was written and silently wrong afterwards, and the
failure mode is the worst one available here: a page saying nothing leaves
this machine while something does.

**What it does not claim.** That the tool is registered, not that searching
will work. Whether Tavily accepts the key is only knowable by spending a
search, and this question gets asked on page loads.

**A separate route rather than another field on `/api/settings`.** Settings
are things a person chose and can PUT back. Nothing here is choosable from a
browser, and putting it beside the settings would invite an attempt to set it.

---

## 2026-08-21: aryabhatta gets a producer, and forecasting is opt-in

**Decision:** `investigate` writes to aryabhatta. Every attempt records the
conditions it ran under, and, when `ZORP_FORECAST` is set, asks a separate
tool-less agent for a forecast and records that before the work begins.
Forecasting is off by default.

**Why:** the discovery layer was complete and inert. Nothing outside
`zorp-track`'s own tests had ever called `record_condition` or
`record_expectation`, so `conditions` and `expectations` were always empty. The
boredom detectors read an empty table, the confounding search had no graph, the
re-run gate had no expectation to gate, and the calibration report always
answered `NotEnoughEvidence`. Nine modules of readers and no writer. Steps 1
through 7 were built to the design and the thing still could not observe
anything.

**Both writes happen before the attempt runs, and that ordering is the whole
point.** A condition recorded afterwards describes a different run than the one
that happened. An expectation recorded afterwards is a postdiction, which is the
one thing `expectations` exists to refuse.

**The forecast is a separate agent with no tools and one step.**
`record_expectation` refusing a late forecast stops the database being lied to,
but it cannot tell a real forecast from a number the model produced in the same
breath as the result. Asking the working agent to report its expectation
alongside its answer would satisfy the guard and mean nothing. So the forecaster
runs first, sees the hypothesis and the metric name, and sees nothing about how
the work will be done. It gets no tools because a forecaster that can read the
repository can find last run's number and report that, which is measurement
dressed as prediction.

**Off by default.** A forecast costs a model call on every attempt, and the
prior art in the design says the calibration it feeds is more likely to fail its
own gate than pass it. Making every run pay for that unasked would be the wrong
default. Left off, the ledger stays empty, which is the honest state for a
record nobody has fed.

**A forecast never fails an attempt.** A malformed one is skipped with a warning
on stderr and the run continues. The work still happened and its outcome is
still worth recording; an experiment with no expectation is simply one the
calibration report does not score. Nothing substitutes a default interval,
because a forecast worth having is never worth inventing. The skip is said out
loud because silence would look exactly like a forecast that was made, and the
difference decides whether that experiment is ever scored.

**Conditions carry only what the harness observed.** The model name, from a new
`Model::identity`, and the checkpoint mode. Not the hypothesis, which is prose
and would put speculation on the observation side of integrity rule 5, and it is
the tempting one. Not the pre-registered metric name, kill threshold or
threshold direction: those cannot vary within a track by construction, so
recording them would make the invariant-condition detector fire on every track
forever while telling nobody anything. An endpoint configured without a model
name records no model condition rather than a blank one, because a blank string
would group unrelated runs together as though they shared a model.

---

## 2026-08-21: the go/no-go is computed, the prose list is one list again

**Decision:** three things that aryabhatta claimed but did not do now hold.
`CalibrationReport::verdict(tolerance)` turns a report into `Go` or
`NoGo(reasons)`. `MODEL_AUTHORED_COLUMNS` covers all nine prose columns in the
schema instead of three, and every module that enforces integrity rule 5 reads
that one list. `record_metric` refuses a non-finite outcome, the way
`record_expectation` already refused a non-finite forecast.

**Why now:** the four merged pull requests were reviewed against their own
claims, and each of these was a place where a document asserted a property the
code did not have. That gap is worse than a missing feature, because a reader
stops checking a thing the instructions say is guaranteed.

**The go/no-go is computed and still not enforced.** Before this,
`calibration_report()` had no caller outside its own tests and the decision
existed only as a sentence. Nothing consults a verdict before the ledger is
written and nothing should: the ledger has to be buildable in order to be
measured, so a runtime block is circular, since a gate that refuses to run until
it has passed can never pass. The design agrees, framing the report as the
deliverable that decides "whether the rest of the architecture is worth
building", which is a decision for people. What changes is that the decision now
has arithmetic attached instead of a feeling, and CLAUDE.md no longer implies
enforcement that does not exist.

**The tolerance has no default, on purpose.** The design says the number "should
be set from the first observed curve rather than guessed here", so shipping a
constant would guess exactly what it refuses to guess. `Tolerance` is a newtype
whose constructor refuses anything outside (0.0, 1.0]: zero is refused because
no forecaster lands on its stated coverage exactly, so a zero tolerance is a
no-go wearing the costume of a measurement, and above one is refused because it
admits every curve that can exist, which is a go wearing the same costume.
`MIN_CALIBRATION_N` is 50 and is a constant, because that number is in the
design.

**Not a number is never a pass.** The verdict checks finiteness before every
comparison. A NaN gap loses every comparison including `gap > tolerance`, so
without that check a report made entirely of NaN bands returns `Go`. It is the
one failure mode where the wrong answer is also the reassuring one. Nine
mutations were run against the verdict and all nine were caught.

**One list, in one place, as the comment always said.** `MODEL_AUTHORED_COLUMNS`
carried three columns while a second, longer copy sat in a `partition` test
carrying five. The constant's own doc comment predicted this: "a second copy
would drift the first time a model-authored column was added, and the drift
would be silent." It had drifted, and the shared list was the shorter one, so
`hypothesis_snapshot` and `assumptions` were unguarded in `detectors` and
`families` while `partition` caught them. The list is now nine columns, each
annotated with its table, checked at runtime against `duckdb_columns()` so it
cannot guard a column that does not exist, and pinned by name so a member cannot
be swapped for a harmless string.

**A name check is not enough, because `SELECT *` has no names.** The rule was
enforced by substring search, which a star defeats completely, and `families`
already contained one. It was safe only because the table it read happens to
hold no prose. That query now names its columns and a bare star is refused
everywhere, with `COUNT(*)` still allowed.

**Testing for the column name tested the wrong thing.** The old inquiry test
asserted a brief did not contain the strings "decision_notes", "prompt_shown" or
"explanation", which are column names. A brief that interpolated the prose
stored in those columns would have passed. It now plants a sentence in every one
of the nine columns and asserts that sentence reaches no brief, and it fails if
a column joins the shared list without a fixture row to write into.

**An outcome that is not a number is refused where it enters.** `record_metric`
had no finiteness check while `record_expectation` had one, so a stored NaN
counted as a miss in every future calibration report, permanently, and could not
be told apart from a forecast that genuinely missed. The two refusals are a pair
and neither works alone. This also makes the finiteness claim in `rerun.rs`
true: repeats are read back out of `metrics`, and that comment now names the
writers providing the guarantee rather than gesturing at "the callers above this
function".

---

## 2026-08-20: two anomalies with no recorded conditions are not alike

**Decision:** the anomaly-family similarity is the Jaccard overlap of two
deviations' condition sets, and an empty intersection over an empty union
scores 0.0 rather than 1.0.

**Why:** the degenerate case is not a corner. Early in a track almost nothing
records conditions, so scoring empty against empty as a perfect match would
bundle every unconditioned row in the ledger into one enormous family, at every
threshold in the sweep, and it would look like the strongest possible finding
because it would survive the widest possible band. The sweep cannot defend
against that: the family really is stable across all of θ. Empty over empty is
the absence of evidence about whether two things co-occur, not evidence that
they do.

**Second decision:** `boredom_candidates` and `family_candidates` are two calls,
not one. Boredom findings are reads of what never varied and need no gate.
Anomaly families sit behind the calibration gate. One call returning both would
let a caller cross the gate without noticing which half of the result did that,
and the gate is only worth having if crossing it is deliberate.

**Third, recorded because it bounds a claim about the handoff:** code enforces
that the model never chooses which findings to look at, never sees a
model-authored column on the way in, and never supplies the facts a candidate
carries. The brief is generated from the record and generation is
deterministic. Code does **not** enforce that the sentence the model writes
back contains no invented claim. Every candidate therefore travels with the
brief it came from, so a question can always be checked against what produced
it. Stated in the module docs rather than left as an implication, because "the
model may not add invariants of its own" reads like an enforced property and
only most of it is one.

---

## 2026-08-20: nothing reaches the anomaly ledger except through the gate

**Decision:** `anomalies` has exactly one writer, `record_gate_verdict`, and it
takes a whole `GateVerdict` rather than loose numbers. There is no argument
list a caller could assemble by hand, so the only way to produce a ledger row
is to have run the re-run gate.

The same call always writes a `gate_runs` row, admitted or not. Rejections are
counted rather than discarded, which is the only way the noisy TV rate becomes
measurable: a rejection that leaves no row cannot be counted afterwards, and a
system that cannot see its own noise floor cannot tell a quiet environment from
a blind one. Making it one call rather than two means no caller can record an
admission and forget the rejection.

`unverifiable` is admitted and flagged but is not counted as noise. A replay
that could not be performed says nothing about how noisy the measurement is,
and folding it into the rate would let a broken harness read as a clean one.
For the same reason `noise_rate()` returns `None` rather than `0.0` when the
gate has never run: zero looks like the best possible result, and "we have not
looked" is not a result at all.

**Second decision, recorded because the spec says otherwise:** the spec lists
four classification rules for the gate and the code has three branches. The
rule "repeats fall outside on opposite sides is volatile" was written as its
own check first, and the mutation that deletes it leaves every test green: a
repeat on the opposite side can satisfy neither the transient branch nor the
reproduced branch, so it already falls through to volatile. The branch was
removed rather than kept for symmetry with the spec. A branch that can never
change an answer implies a distinction that does not exist, and the next person
to read it would look for the case it handles and not find one. The spec's
table is still right about what the gate decides; it just states in four rules
what takes three to compute.

---

## 2026-08-20: the calibration report scores the last forecast, not every draft

**Decision:** where several expectations exist for one experiment and metric,
the calibration report counts only the last one written before the outcome.
The metrics side already took the first recorded value; this is its mirror,
so one prediction is scored against one result.

**Why:** `expectations` deliberately allows a forecast to be rewritten while
no outcome exists, because revising a belief before observing anything is
legitimate. Scoring every version undoes that. Nine absurdly wide drafts and
one real forecast would read as nine tenths covered, which is precisely the
"buy coverage with wide intervals" failure the report exists to expose. The
mean interval width would have made the padding visible, but visible is not
the same as excluded, and the coverage figure is the number people will
quote.

Found during integration rather than by either author: the module that allows
revision and the module that scores it were written in parallel and neither
could see the other. Caught by a test written to fail first, which counted
2 where it should have counted 1.

**Second decision, recorded because it bounds a claim:** above the |V|
crossover the search backend answers a slightly different question from the
exact one. Confounding is transitive, so the exact answer is a connected
component; modularity rewards assortativity, and a long thin chain scores
better cut in half. Because `erbga` only removes edges, its partition is
always a refinement of the components: it can split a true bundle, never
invent one. So a bundle reported above the crossover is a floor on the
confounding, not the whole of it. This is documented on the function. The fix,
if it ever matters, is a component-preserving objective, not dropping erbga.
## 2026-08-20: a review panel is launched by a person, never by a model

**Decision:** the review panel is a button in the browser and a function in
`zorp-agent`. There is no model-callable tool to spawn a reviewer, and none is
planned.

**Why:** an agent that can spawn agents can spawn agents that spawn agents, and
nothing in the loop bounds that. A human-launched panel has a natural bound:
one click, one panel, a fixed number of reviewers, each of which runs with no
panel of its own. `zorp-agent/src/agent.rs` already carried a test asserting a
filtered agent has no `spawn_subagent`, `monitor_subagents`, `cancel_subagent`
or `invoke_subagent`. That test still passes and this work did not add any of
them.

**A reviewer gets strictly less than the panel that launched it.** Read-only
tools by allow-list, named one at a time rather than derived by excluding the
dangerous ones, because an allow-list stays correct when a new tool is added
and a deny-list silently grants it. No `write_file`, no `apply_patch`, no
`run_command`. An opinion that can edit the thing it is reviewing is not a
review. This is the rule `zorp-skill` already follows for skill bodies.

**The panel is not `critique`, and the two must not merge.** Critique is a gate:
it audits a draft against a track's own evidence record, the audit is
arithmetic, and it refuses if the record moved underneath it. The panel is a
reader: it produces opinions, changes nothing, and can be wrong. Conflating
them would let an opinion block a deliverable the evidence record was happy
with.

**Agreement is computed in code and never negotiated inside the panel.**
Reviewers do not see each other's findings, and the prompt says so. Reviewers
that read each other converge, and a panel that converges is one reviewer with
extra cost. Corroboration counts *lenses* rather than findings, so one reviewer
listing the same objection three times cannot corroborate itself. Locus
matching normalizes case and surrounding whitespace and nothing else: a fuzzier
match would merge two different objections that happen to be worded alike, and
inflating a corroboration count is the one error the function must not make.

**A failed reviewer is a first class part of the report.** A panel of five where
two fell over is not a panel of three. Without `lenses_requested` next to
`verdicts`, "every reviewer agreed" can mean "the one that ran agreed", and the
browser leads its summary with the shortfall for the same reason.

---

## 2026-08-20: a command may not call the server it is running under

**Decision:** when `zorp-agent` runs under `zorp-web`, `Policy` is given the
server's port and denies any `run_command` naming a loopback address on it.
The CLI configures no port, so nothing there changes.

**Why:** one approved `run_command` was enough to `curl` the API and turn
auto-approve on, after which nothing the agent did was reviewed again. That
turns a single approval into a standing one, which is the one escalation the
approval gate exists to prevent. It is a hard deny rather than a prompt, so
the call never reaches the gate and cannot be waved through by a user who is
clicking quickly.

The check rides on `deny_reason`, so it inherits that function's recursion
into `sh -c` payloads and `$(...)` bodies. Wrapping the call in a shell was
otherwise the whole bypass, and there is a test for it.

**What this is not.** It is not a boundary against an adversary, and the doc
comment on `with_own_server` says so in as many words. A shell is a shell:
anything holding one can obfuscate a URL past a literal string check without
effort, and can do considerably worse things than call an HTTP endpoint. What
this defends against is the model doing it, which is the case that actually
occurs: a curl to an address sitting in its own context is an obvious next
step for a model that has been told, or has talked itself into, wanting fewer
prompts.

The real control is authentication the local process cannot replay, for
instance a per-session secret handed to the browser at creation and never
written to disk. That is a larger change and deliberately not this one.

**Deliberately still allowed:** every other loopback port. Denying all of
them would break talking to a local model, which is how most of zorp is run.
`http://localhost:11434` has a test saying so.

---

## 2026-08-20: the architecture index is deleted, the specs are the index

**Decision:** `docs/ARCHITECTURE.md` is removed. The per-capability specs in
`docs/superpowers/specs/` are the architecture record, and this log is the
decision record. There is no third document summarizing them.

**Why:** it was a summary of things that each already had a durable home, so
it could only ever be a second copy going stale. It had started to: it still
described `erbga` as off any critical path the day after 2026-08-19 wired it
in. A pointer file that lies is worse than no pointer file, and the failure
mode is built in rather than accidental, because nothing makes a summary
update when the thing it summarizes changes.

**What went with it, honestly:** the only index of which spec covers which
capability, and the only place recording that the systems paper is written
but unposted and that hand-checked runs live in `uat/`. Nothing else stated
those. If the paper and UAT status need a home, it is `README.md` or their
own directories, not a revived index.

**What did not go:** the constraint that four capabilities is the whole set.
That is recorded here, 2026-08-14 and 2026-08-15, where the fifth was
designed and turned down. The 2026-08-19 entry below refers to
`docs/ARCHITECTURE.md` in the present tense; per this log's convention that
entry is left as written, and this one supersedes the pointer.

**Superseded by this entry:** the `docs/ARCHITECTURE.md` references in the
2026-08-19 entry below.

---

## 2026-08-20: the API answers named origins, and the MSRV is checked

**Decision, part one:** `zorp-web` allows no cross origin caller unless one
is named with `--allow-origin`, repeatable. It was `allow_origin(Any)`.

**Why:** `POST /turn` runs an agent that executes commands and edits files on
the machine the server was started on, and the token gate only arms on a non
loopback bind. On the ordinary install the two together meant any page the
user happened to visit could drive that agent and read back what it produced,
with nothing on screen to show it happening. The permissive header was there
for a real reason, the container split and an `index.html` opened off disk,
but "some other origin needs this" was answered with "every origin may".

The default costs the normal install nothing. When this server serves the UI
the page and the API share an origin and the browser runs no CORS check at
all, which is why the flag is empty by default rather than seeded with
loopback addresses.

`null`, the origin of a `file:` page, is available as `--allow-origin null`
and is deliberately not on by default: `null` is also the origin of a
sandboxed iframe, so allowing it silently would reopen the hole under another
name.

**One implementation note worth keeping.** The origins go to `CorsLayer` as
one list, not one call each. A single value sets a fixed
`Access-Control-Allow-Origin` that goes out whatever the request asked for,
leaving the browser to catch the mismatch; the list form makes the server
compare and answer only for an origin on it. Repeated calls also replace
rather than accumulate. The first version here did it the wrong way and a
test caught it.

**What this does not fix:** an agent with one approved `run_command` can still
`curl` this server's own endpoints, because that is a local process and not a
browser. CORS is not the control for that; a policy rule denying loopback
calls to zorp-web's own port would be, and it is not written yet.

**Decision, part two:** `rust-version` is 1.95, and CI has an `msrv` job that
installs exactly the declared version and builds with it.

**Why:** the manifest said 1.82 and had for a long time. Every CI job used
`stable`, so nothing ever checked it, and the dependency tree had walked to
1.95 without a single build going red: `hashbrown` wants Cargo's
`edition2024` feature (1.85), `globset` and `icu_collections` want 1.88, and
`merman`, a non optional dependency of `zorp-agent`, wants 1.95. The declared
floor was thirteen minor versions below the real one.

The job reads the version out of the manifest rather than hardcoding it, so
the two cannot disagree. Note there is no headroom left: 1.95 is current
stable, so anyone on an older Rust cannot build zorp at all. That is worth
revisiting, and it is a dependency question rather than a code one.

---

## 2026-08-19: the discovery layer is called aryabhatta, and erbga is wired into it

**Decision, part one:** the anomaly-driven inquiry subsystem is named
**aryabhatta**, and it is the home for discovery ideas generally. New ideas
about how zorp derives its own questions land there rather than becoming
capabilities of their own. It stays record-and-readers, the way `critique` is
a gate, so it is not a fifth capability. `docs/ARCHITECTURE.md` still says
four is the whole set.

**Decision, part two:** ideas live in aryabhatta at a state, not at a
yes or no. The registry in the spec has four: Proposed, Gated, Building,
Retired. An idea advances on stated evidence, never on enthusiasm, and
nothing is deleted for failing. The point is that everything can belong
while each part stays independently killable, because step 3 of the
implementation order is a stop sign and a monolith cannot be stopped.

**Decision, part three:** `erbga` is integrated as the backend of the
subsystem's community detection service, for graphs too large to solve
exactly. This reverses part of 2026-08-15, which shipped it off any critical
path, and `CLAUDE.md` requires an entry saying so before it is wired in.
This is that entry.

**Why erbga now, and not later:** it has a caller that does not depend on
the anomaly ledger. Bundling confounded conditions reads the `conditions`
table from step 1, sits on the ungated side, and answers a real question:
when two conditions never vary independently, no effect can be attributed
to either alone. That is aliasing in the design of experiments sense, and
it strengthens `invariant_condition`, which is blind to a pair that always
moves together while both vary.

**What the integration had to solve, rather than wave through:**

- **The threshold.** `erbga` takes unweighted edges, so a continuous
  similarity has to be cut somewhere, and an unpriced cut is exactly the
  defect that sank `evolve` (the blob-maximizing score, with edge addition
  free). The service does not price θ, it refuses to pick one: it sweeps a
  range and keeps only communities surviving a contiguous band, recording
  the band. Nobody chooses θ, so nobody can choose it to reach an answer.
- **The scale.** 2026-08-15 observed that at `V = 20` an exact
  clique-partitioning ILP finishes in about 0.2 seconds. That stands, so the
  backend is chosen by graph size: exact below the crossover, `erbga` above.
  In the band where both run, the exact result is a continuous regression
  check on the search against proven optimality, which the four fixed
  benchmark networks cannot give.
- **The stochasticity.** Seed, island count and parameters are recorded
  beside the partition, so a clustering is reproducible like any other
  recorded result.

**What may not be claimed.** Reading the source rather than the module
names: `chromosome.rs`, `rng.rs`, `selection.rs` and three of the five
operators are representation-agnostic and transfer. `RepairTargets` and
`gene_repair` use `graph.degree()` and `graph.incident()` and do not. Both
things that make erbga *erbga* are graph-specific: the reduced-bias encoding
solves a `k!` label-permutation blowup a non-graph genome does not have, and
Gene Repair needs a vertex degree a `condition_key` does not have. The four
benchmarks certify ERBGA on graphs and nothing else, so any future consumer
reusing only the scaffolding is running a new algorithm and needs its own
validation. Calling such a thing validated prior work would be false.

**Rejected:** spawning an agent per population member, each holding a
hypothesis, with the unfit eliminated. That is `evolve` again, and the first
of its three findings applies unchanged: there is no free inner search,
because variation is model-proposed. erbga's own thesis parameters are
250,000 fitness evaluations per island across 25 islands, which is fine for
bit flips and impossible for model calls. A few hundred calls is a
tournament, not evolution. Two further objections also carry over: a
population drawn from one model is one model sampled many times, and
selecting on a metric that is then reported is biased upward twice. The
structured-genome version, where the model proposes the vocabulary once and
the search over combinations runs in code, is recorded as Proposed in the
registry rather than built.

**Also folded into the spec:** the prior art pass
(`docs/superpowers/specs/2026-08-19-anomaly-driven-inquiry-prior-art.md`).
Its findings change what the design may claim, not what it builds. The
architecture is substantially AutoDiscovery (arXiv 2507.00310), which must be
cited and distinguished. The calibration question is answered next door for
the exogenous case, and badly: FermiEval reports 65% coverage at a nominal
99%, so the step 3 stop sign is now more likely to fire than not, which is an
argument for keeping it exactly where it is. What survives is the endogenous
case, where the quantity being measured is manipulable by the thing being
measured.

**Full writeup:**
`docs/superpowers/specs/2026-08-19-anomaly-driven-inquiry-design.md`, kept in
sync with `designs/` in the kb repo. The subsystem as a whole is still
proposed and not approved; these three decisions inside it are taken.

---

## 2026-08-19: the web composer's send button becomes a stop button

**Decision:** the browser gets a stop control, and it is the send button
itself rather than a second button beside it. It stops the run for real:
`POST /api/sessions/:id/stop` raises the turn's cancel token, which the
agent loop reads between steps and around every tool call and which its
sandbox reads while a command is running, so a `run_command` in flight
has its process group killed. The same request also resolves any pending
approval as a denial, because that gate parks the agent's own thread and
raising a flag it is not reading stops nothing for the five minutes the
approval takes to time out.

A stopped turn ends like every other turn: a new `stopped` event, then
`done`. It is not an `Error`. The transcript says the reader stopped it,
the approval card that was open says the same rather than claiming it
expired, and whatever the model had already streamed stays on the page.

**Why:** the agent has been cancellable since it was written and nothing
could cancel it from a browser. `zorp-web` built its own cancel token
inside `run_agent` and handed the only copy to the agent, so the feature
was present and unreachable. The button was disabled for the length of a
run, which meant the one control on screen during the one moment you
might want to intervene did nothing.

**What it rules out:** a second, separate stop button. There is one spot
at the end of the composer, a hand is already there, and a stop parked
somewhere else is one you hunt for while the thing you want stopped keeps
running. Also ruled out: stopping locally and letting the run continue
server side, which would put the composer back while the agent carried on
writing files, and treating a stop as a failure, which would file a
deliberate act under "Something went wrong".

**Known bound:** cancellation is checked between agent steps and around
tool calls, not inside a model call. A stop pressed while a single long
completion is streaming takes effect when that completion finishes.
Nothing further runs, and the turn still ends cleanly; it just does not
end instantly. Interrupting mid-stream would mean teaching the provider
transport about cancellation, which is inherited harness code and its own
piece of work.

**Superseded by** the 2026-08-19 entry below on the streaming read loop.
The bound was measured rather than reasoned about and turned out to be
minutes, not seconds, which made it the ordinary case rather than an
edge one.

---

## 2026-08-19: the streaming read loop watches the cancel token

**Decision:** `streaming::stream_sse` takes an optional cancel token and
checks it between reads, on both the event-stream path and the path where
an endpoint ignored `stream` and answered with a document. A raised token
abandons the response and returns an error. The agent, which owns the
token, reads it when a model call fails and reports `Outcome::Cancelled`
rather than `Outcome::Error`, so a deliberate stop is still not a
failure.

The token reaches the transport as a new argument on
`Model::complete_streaming`. Three signatures in the workspace define
that method and no test double overrides it, so this is a smaller change
than putting a cancel field on `HttpModel`, whose public struct literal
appears seventeen times including in the public-API compatibility test.

**Why:** the bound recorded above was measured against a real local model
and was not a bound worth accepting. Pressing stop twenty seconds into a
`qwen3.8:27b-mlx` answer took **303 seconds** to end the turn. For most
of that the browser showed only a spinner, because a thinking model's
output is withheld by `ThinkGate`, so the button said stop, the page said
running, and nothing visibly happened for five minutes. Waiting on a long
answer is exactly when somebody reaches for stop. The same press now ends
the turn in about one second.

**What happens to the half-arrived response:** it is dropped. Nothing is
pushed to the transcript and nothing is recorded. A response cut off
partway has text that stops mid-word and tool calls that may be half
parsed, and recording one would leave an assistant turn holding calls
that no tool result answers, which is a transcript the next turn has to
send back to the provider. The stopped turn therefore ends with the user
message and no assistant reply, and the next turn works normally.
Whatever had already streamed stays on the page for the reader; it just
does not become part of what the model is told it said.

**What this still does not cover:** the check sits between blocking
reads, not inside one, so a provider that accepts a request and then
sends nothing at all is still waited on. A model that is producing an
answer sends something several times a second, including while it is
reasoning, so this covers the case that occurs. Genuinely buffered
completions are not interruptible either: `Provider::Anthropic` and every
`complete()` caller go through `zorp::zorp_raw`, which is one blocking
request that reads the whole body, and making that interruptible means
restructuring a primitive shared with non-agent callers. For those the
run stops when the call returns, which is the behavior described in the
superseded entry above.

---

## 2026-08-19: the browser can stand its approvals down, per session, loudly

**Decision:** the web UI gets auto-approve: a per session standing yes
that stops the browser being asked about every tool. Off for every new
session, turned on only by an explicit control, revocable in the middle
of a run, and never written to disk. `POST /api/sessions/:id/auto-approve`
sets it, `GET` reads it back, and it lives on `SessionState` as an
`AtomicBool` shared with each turn's `WebApprover`.

**Why it is not a new `ApprovalMode`:** the mode already exists.
`ApprovalMode::AutoApprove` is what `--auto-approve` and the chat
`/approve` command have always given the CLI, and this is the same thing
with the same name. What the web needed that the CLI did not is the
ability to change its mind mid-run: `Agent::set_approval` takes `&mut
self` and the agent is owned by the turn thread, out of reach of an HTTP
handler. The session's `WebApprover` is the one object a handler can
reach while a turn is running, so the standing answer lives there and is
read per call rather than baked into the agent at construction.

**Why it is not a `Preset` change either:** presets say which operations
have to ask. This says who answers. Flipping the web to `Preset::Full`
would change what the policy decides, would still leave `mcp__` tools
asking, and would be a server configuration decision taken by a browser
button. Wrong axis, wrong owner.

**What it does not bypass:** anything. `Policy::decide` runs first in
`Agent::run` and only its `Ask` reaches an approver at all, so the hard
denylist, compound commands and `sh -c` payloads included, refuses the
same commands with this on as with it off. `zorp-agent` was not modified
to add this feature; the only change there is a test that pins the
ordering (`a_denylisted_command_is_refused_even_under_auto_approve`),
which fails if `decide` and the approval gate are ever swapped.

**What it costs:** the user has to be able to see it. A red pill in the
toolbar and a banner over the composer both say so for as long as it is
on, each auto-approved call leaves an `auto-approved <tool>` line in the
transcript, and a call that cannot be written to that transcript is
refused rather than run unrecorded.

**What it rules out:** persisting it. There is no config key and no
database column, so it cannot follow the user into tomorrow's session or
survive a restart, and no manifest can turn it on for someone. It also
rules out turning it on as a side effect of anything else: the switch
does not answer the approval already on screen, and the "Allow all for
this chat" button on a card does both steps in the open, mode first.

---

## 2026-08-19: a turn is seeded from the store, and compaction never writes to it

**Decision:** every web turn rebuilds its agent from the stored
transcript rather than starting empty. The store is the source, not a
live agent kept in memory per session. `zorp_agent::plan_seed` builds
what gets sent: the current system prompt, then the recorded
conversation, repaired so no tool call is left dangling, compacted if it
will not fit. The CLI's `resume` goes through the same planner, so both
surfaces continue a session the same way.

**Why the store and not a live agent:** the store outlives the process
and a live agent does not. Reopening a session from the sidebar after a
restart and continuing it is then the same code path as continuing it a
second after the last turn, rather than a second mechanism that only gets
exercised when something has already gone wrong.

**Why the prompt is not replayed:** stored system messages are dropped
and one current prompt is put at the front. The prompt is the harness's
to set. A session recorded before a prompt change should not keep
re-sending the old one, and the several stored copies the web server used
to write (one per turn, since every turn built a fresh agent that
recorded its own system message) collapse back to one.

**What compaction throws away, in order:** the bodies of the oldest tool
results, replaced by a marker saying how many bytes went. That extends
the byte cap that already existed rather than sitting beside it: one
mechanism with two triggers, the 512 KiB cap on accumulated tool results
and, when a window is configured, a token target. Two passes eliding the
same bodies for their own reasons would double-count what they freed. On
the seed path only, whole exchanges go from the front first, a user
message and everything that answered it together, so the model never sees
a reply to a question that is no longer there. The newest exchange is
never dropped.

**No model-written summary.** A summary is a second chance to
hallucinate, and when it is wrong the material it replaced is no longer
in the request to contradict it. It also costs a model call that can fail
mid-turn, on the endpoint that just ran out of room. Deterministic
elision states exactly what left.

**Compaction never rewrites the record.** It rewrites message bodies in
the agent's own transcript after the recorder has already been handed the
originals, and drops whole messages only before a run starts, never
during one, because `sync` tracks what it has persisted by index. What is
sent shrinks; what was said does not. `zorp-track` and the research
capabilities treat the record as evidence and the 2026-08-18 critique
entry below says why a record that moves under you is worthless.

**The user is told.** Compaction emits a `notice` naming what went and
saying the full transcript is still on disk, because silent context loss
is how an agent starts confidently contradicting itself.

**What it rules out:** guessing the context window. zorp talks to
arbitrary OpenAI-compatible and Anthropic endpoints, local Ollama
included, and none of them can be asked how large their window is. There
is no default that is not wrong for somebody, so the window is unknown
unless `ZORP_CONTEXT_TOKENS` says otherwise. Unknown means the meter
shows tokens with no percentage and no bar and names the variable, and
token-driven compaction is off while the byte cap still runs. The meter
prefers the provider's own reported usage and marks the fallback estimate
with a tilde and its own wording, because a measurement and an estimate
must not be drawn the same way.

---

## 2026-08-19: a PDF is read for its text, not framed for its layout

**Decision:** `.pdf` leaves the sandboxed-iframe category and joins the
office formats. The server reads the text out of it with `pdf-extract`,
in a new `zorp-web/src/pdf.rs`, and sends markdown. The pane renders that
through the same renderer `.md` and `.docx` already go through. This
reverses the PDF half of the 2026-08-18 entry below and the "PDFs" section
of `superpowers/specs/2026-08-17-artifact-pane-design.md`.

**Why:** the old design said the browser's own viewer would render the file
inside the iframe. It does not, and never did. The raw endpoint sends every
file with a bare `Content-Security-Policy: sandbox`, which puts the document
in an opaque origin with scripting off, and no browser's PDF viewer starts
under that. What a user actually saw was a broken-document icon on grey.
Confirmed in Chrome against a running server, and the frame was the right
size and visible, so it was never a layout bug.

That left three ways out and only one of them is defensible.

**Ruled out, loosening the sandbox for PDFs.** Making the viewer run needs
`allow-scripts` and `allow-same-origin`, and the raw endpoint is same origin
with the app, so `allow-same-origin` hands the framed document back the
handle the sandbox existed to take away. It also has to be loosened twice,
in the response header and in the iframe's own `sandbox` attribute, and both
would become per-type decisions. Today the header is one string for every
response, which is a property that cannot be got wrong for one type; making
it conditional trades that away. And the thing being made to run is PDFium
plus a PDF's own embedded JavaScript, over a file a model wrote or
downloaded. Paying all of that to render page images, when the request was
to read the file, is the wrong trade in both directions.

**Ruled out, bundling a JavaScript PDF renderer.** `web/` has zero runtime
dependencies on purpose, pdf.js is the opposite of small, and it would parse
a hostile PDF in this origin, which is strictly worse than what is there
now.

**What the extraction gives up:** layout. No page images, no figures, no
columns reassembled, no headings, because a PDF has no headings to recover:
it records where each glyph was drawn, and a heading in one is text that was
set larger. Promoting it to `#` would be inventing structure the file does
not carry. Text and the breaks between blocks survive, and the renderer
reflows them, which is what a narrow side pane wants anyway. Same scope as
the office formats and the same sentence applies: this is for reading what a
run produced, not for rendering a document.

**Cost paid:** one dependency, `pdf-extract`, and nineteen crates the
workspace did not have, all MIT or MIT/Apache-2.0. Roughly half of them are
`lopdf`'s AES and hashing for encrypted files, which lopdf does not put
behind a feature. `pdf-extract` already asks for lopdf with default features
off, so lopdf's chrono, jiff, rayon and time stay out. The alternative was
`lopdf` alone, which is most of that tree anyway and was measured before it
was rejected: on zorp's own paper its text extraction returns
`zorp:AHuman-CheckpointedResearchAgent`, with every space gone and the
ligatures dropped, because it does not apply the kerning offsets that stand
in for spaces or resolve the font's ToUnicode table. `pdf-extract` returns
the sentence. Hand-rolling it, the way `documents.rs` is hand-rolled, lands
at lopdf's quality and not at pdf-extract's, because that gap *is* the font
machinery.

**Also changed:** both readers now run under `spawn_blocking`. Both are
parsers pointed at a file a model wrote, both are slow enough to matter on a
long document, and a panic in one comes back as a join error rather than
taking anything else with it.

**What did not change:** `.svg` and `.html` are still the only sandboxed
types, still served with a bare `sandbox` and `nosniff`, and the test that
says so now asserts the exact header for every type the endpoint serves
rather than for one of them. Verified in the browser alongside the PDF: a
served SVG carrying `parent.document.title = "pwned"` is still refused by
the engine with "Blocked script execution ... the document's frame is
sandboxed", and the title is unchanged.

---

## 2026-08-19: a run that wrote a file opens the pane showing it

**Decision:** when a turn produces a file, the artifact pane opens by
itself and shows the newest one. This reverses part two of the
2026-08-18 entry below, which had the pane stay shut and put a count on
the Files button instead.

**Why:** the earlier reasoning was that a pane appearing over a half-read
answer is an interruption. That holds for a run that happened to touch a
file on its way to answering something else. It does not hold for the
ordinary case, which is somebody asking for a document: they get a small
dot on a button, read it as nothing having happened, and the document
sits unread behind a click nobody knows to make. The model cannot open
the pane either, so it ends its turn telling the user to go and find the
file, which is the interface asking the user to do its job.

**What it rules out:** the badge. Nothing sets it once the pane always
opens, so `showArtifactBadge`, the `artifacts-badge` span and its styles
are gone rather than left as unreachable code. Somebody who wants the
pane out of the way closes it, and closing it once is a smaller cost
than never finding the file at all.

---

## 2026-08-18: a draft gets audited against the record, and the auditor is code

**Decision:** `zorp-agent critique "<question>"` audits a track's
`draft.md` against that track's evidence record and revises what the
record does not support. It is not a fifth capability. It has no scope of
its own, gathers nothing, and produces no evidence: it reads the record
and edits the artifact `co-write` produced. Four capabilities is still
the whole set. See
[`superpowers/specs/2026-08-18-zorp-self-critique-design.md`](superpowers/specs/2026-08-18-zorp-self-critique-design.md).

**Why the model does not judge:** asking a model whether its own draft is
good produces a confident answer about a confident answer. The model's
only job here is extraction: inventory the draft's claims and name, from
a fixed list, the one piece of evidence each rests on. Code decides
whether that evidence exists. Alongside it, a purely deterministic pass
flags every figure in the draft that the record cannot account for at any
rounding, with no model involved at all, so a critic that reports nothing
cannot declare a draft clean.

**Why it terminates:** two bounds, neither of them the model's opinion. A
configured round bound (`--critique-rounds`, `ZORP_CRITIQUE_ROUNDS`,
default 2, where 0 means audit only), and a strict-improvement rule: a
revision is kept only if it leaves strictly fewer findings than the draft
it replaced, and the first one that does not ends the pass. Findings are
a non-negative integer that must strictly decrease, so the loop cannot
run longer than the initial finding count. That rule is also what makes
"the draft is fine" reachable: a clean draft costs one model call and
nothing is rewritten.

**What is recorded:** a `critiques` table in `zorp-track`, one row per
audited draft (round, draft hash, findings as JSON, whether it was
carried forward), plus `draft.pre-critique.md` for the diff and
`critique.md` for reading. Findings are deliberately **not** recorded as
metrics: they would then be evidence `co-write` reads, and a later draft
could cite its own critique as a finding about the question.

**What it rules out moving:** the Kill Threshold and everything else
pre-registered. The pass snapshots the record before it runs and
re-checks it after every model turn, and any movement aborts with the
draft untouched. This is enforced at runtime rather than assumed from the
call graph, and it is tested against a write-capable agent that really
does rewrite `prereg.md` mid-run.

**What it cannot catch:** that cited evidence actually implies the
sentence citing it. The pass verifies the evidence exists, not that it
supports the claim. Derived figures also read as invented. Both are in
the spec's limits section.

---
## 2026-08-18: skills are Claude Code's format, and they grant nothing

**Decision:** zorp discovers and loads skills in Claude Code's format,
unchanged: a directory holding a `SKILL.md` with YAML frontmatter
(`name`, `description`) and a markdown body. Discovery reads
`~/.claude/skills/<name>/SKILL.md`, then `<cwd>/.claude/skills/<name>/SKILL.md`,
then `$ZORP_SKILLS_DIR/<name>/SKILL.md`, later scopes winning a name
collision. Parsing and discovery live in a new crate, `zorp-skill`, that
depends on nothing else in the workspace. `zorp-agent` exposes it as one
built-in, `skill`, through the existing `Tool` trait.

**Why Claude Code's format and not a zorp native one:** the value of a
skill is that the user already wrote it, or already installed someone
else's. A zorp specific format would start with zero skills in existence
and ask users to port. Reading the format that already has skills in it
costs one small parser and buys the whole existing corpus. This was
checked, not assumed: 342 `SKILL.md` files on the author's machine, 339
parsed, and every one of those 339 descriptions matches what PyYAML
reads from the same file. The three that did not parse have no
frontmatter at all and are malformed by Claude Code's own rules.

**How this differs from capsules.** A capsule is loaded by the *human*,
with `/load`, and stays in the system prompt for the rest of the session,
where it can also point at a `scripts/` directory. A skill is chosen by
the *model*, mid turn, from a one line index, and arrives as a tool
result for that turn. The two answer different questions: "run this whole
session in this mode" against "this particular task looks like something
the user has written down". They also live in different places, `.zorp`
against `.claude`, because the second one is not ours to move. Neither is
a replacement for the other, and neither should be deleted for the other.
If they ever converge, the merge point is the parser, not the lifecycle.

**How this differs from flavors.** A flavor is configuration: model,
tool allow-list, approval preset, verification commands, all of it
trusted or gated as configuration. A skill is content. The layering rule
is borrowed from flavors, user then project, because users already know
it. Nothing else is.

**What a skill is not allowed to do.** It cannot enable a tool, widen an
approval preset, or get anywhere near the `run_command` denylist. Real
skills in the wild carry an `allowed-tools` frontmatter field; zorp
parses it, prints a warning saying it is being ignored, and ignores it.
The reason is that a skill is untrusted input by construction. It is a
markdown file that can arrive by `git clone`, and its body becomes
instructions. Anything that let that file also move the boundary would
make the boundary decorative. So: the tool argument is a lookup key into
an already scanned registry and is never joined onto a path; a name has
to be a single ordinary path component; a `SKILL.md` that resolves
outside its own skill directory is skipped (the directory itself may be a
symlink, because installing a skill that way is normal); and a file over
64 KiB is skipped rather than truncated.

**The one thing a skill does get:** `Policy::decide` treats `skill` like
a local read, `Allow`, alongside `read_file`. Gating it at `Ask` would
make skills unusable under the default `NonInteractive` mode, where `Ask`
means deny, and would buy nothing: the body confers no capability, and
every action it suggests still routes through the same policy. zorp
already folds project `AGENTS.md` and `CLAUDE.md` into the system prompt
with no prompt at all, so project controlled instruction text already
reaches the model unasked. A skill is strictly narrower than that: named,
capped, opt in per turn, and reported.

**Ruled out:** a YAML dependency (the frontmatter in use is flat scalars,
block scalars, and one level of nested map, and a full YAML parser is a
much larger surface aimed at a hostile file for features no skill uses);
letting a skill widen the tool set behind a trust prompt like the one
project flavors get (a flavor is configuration the user is choosing, a
skill is content that arrived with a repository); and putting skill
discovery in `capsule.rs` (adjacent, different lifecycle, and capsules
are inherited harness code).
## 2026-08-18: the artifact pane surfaces what a run wrote, and reads office formats

**Decision, part one:** the pane notices produced files by diffing the
`/api/artifacts` listing, not by reading tool events. Every listing row now
carries `modified_ms`. The browser snapshots the listing when a turn starts
and compares after tool activity and at the end of the turn; anything new or
newer is something the run wrote.

**Why not tool events:** because how a file got written is not knowable from
one. A PDF that pandoc produced under `run_command` names no path anywhere,
and it is exactly as much a result of the run as one `write_file` wrote.
Parsing tool summaries for paths would catch the easy half and quietly miss
the other, which is the worst of both. Asking the directory catches whatever
the next way of writing a file turns out to be.

**Decision, part two:** a run that produced something says so with a count on
the Files button, not by opening the pane. The pane opens itself only when it
is already open. A pane that appears over the answer somebody is reading is an
interruption, and the run has already finished, so there is nothing being
missed by waiting for a click.

**Superseded by** 2026-08-19: a run that wrote a file opens the pane showing
it. Parts one and three stand.

**Decision, part three:** `.docx`, `.odt`, `.xlsx` and `.pptx` are extracted
to markdown on the server, in `zorp-web/src/documents.rs`, and rendered
through the markdown renderer that already exists. Headings, paragraphs,
lists and tables. Images, fonts, colours, page layout, footnotes, comments,
tracked changes and numbered-list numbering are out of scope and stay out.
This is for reading what a run produced, not for rendering a document. The
extractor treats its input as hostile: caps on archive entries, on
decompressed bytes per part and per archive, and no XML entity expansion
beyond the five predefined ones, so neither a zip bomb nor billion laughs is
a case that needs thinking about at the call site.

**Decision, part four, and the one that needed the argument:** `.svg` and
`.html` are served and shown. Both execute. They are safe here for exactly
one reason: they load into the pane's iframe by URL, and the raw endpoint
sends every file with `X-Content-Type-Options: nosniff` and a bare
`Content-Security-Policy: sandbox`, no `allow-` token. That puts the document
in a unique origin with scripting off, so script inside it neither runs nor
gets a handle on the page that framed it. Verified in a browser: a served SVG
containing `parent.document.title = "pwned"` is refused by the engine with
"Blocked script execution ... the document's frame is sandboxed".

**What that rules out:** inlining either one. An `<svg>` element built from a
served file, or an iframe `srcdoc`, would run that script in this origin and
make every precaution in `web/src/markdown.ts` beside the point. The rule is
in `web/src/artifact-view.ts` and there are tests either side of it: the
server's headers, and the pane's refusal to put the bytes on the page even
when handed them.

**Cost paid:** two dependencies, `zip` and `quick-xml`. `zip` is pinned to 4
rather than the 6 already in the lock as a build dependency of
`libduckdb-sys`, because 6 requires Rust 1.83 and this workspace's floor is
1.82. Every codec but deflate is switched off; an office file uses no other.

## 2026-08-18: answers stream, and the streaming path filters reasoning

**Decision:** `Model::complete_streaming` is the agent loop's model call.
It has a default body that runs the buffered path and reports the answer
as one delta, so every existing `Model` keeps working; only `HttpModel`
on an OpenAI-compatible provider genuinely streams. `Renderer` gains
`assistant_delta`, empty by default, so only the browser changes.
Anthropic and the CLI are unchanged on purpose.

**Why the default body matters:** it means there is one code path in the
agent loop rather than a streaming branch and a buffered branch that
drift. A provider that cannot stream is not a special case, it is a
`Model` that reports its answer in one piece.

**The part that is not obvious:** `extract_think_tags` strips
`<think>...</think>` out of content before anyone sees it, so a
qwen-family model's chain of thought never reaches the browser today.
Streaming raw content deltas would put it on screen, formatted as the
answer. `streaming::ThinkGate` filters it, and withholds text that could
still turn out to be a tag opening, because the tags arrive split across
chunks like anything else. The accumulator then rebuilds a
buffered-shaped response and hands it to `parse_assistant_completion`, so
streamed and buffered turns cannot produce different messages.

**What it rules out:** shortening the SSE backlog. Pruning fragments once
the finished answer arrived was tried, and `stream_events` holds an index
into that vector across polls, so it panicked the streaming task and
poisoned the session mutex. The backlog is append-only and the doc
comment on `record` says why.

**Also decided:** a provider that ignores `stream` and returns a whole
JSON body is handled rather than treated as an empty stream. Silence is a
worse answer than a slow one.

---

## 2026-08-18: one system prompt, and it says zorp is a research agent

**Decision:** `zorp_agent::DEFAULT_SYSTEM_PROMPT` is the single default
system prompt for every zorp surface. The CLI and `zorp-web` both use it.
It positions zorp as a research agent, asks the model to ground claims in
what it actually checked, tells it to say so when it cannot, and says the
question does not have to be about code.

**Why:** the two surfaces had written their own. The CLI carried quecto's
"You are zorp-agent, a helpful coding assistant" unchanged through the
rename. `zorp-web` said "You are zorp, a careful assistant", which is
vague enough that the model fills in the rest itself: asked "zorp", a
local model introduced itself to a user as their "coding buddy". Neither
string matched the README, zorp.dev, or the four research capabilities.

The positioning is not decoration. A prompt that says "coding assistant"
is asking for a different behaviour than one that says claims must be
grounded and counter-evidence reported, and that behaviour is the
product.

**What it rules out:** a surface keeping its own copy. That is the
mechanism that produced two wrong answers here, so `zorp-web` gets a test
asserting its prompt is the shared constant, not merely a similar one.

**What it does not change:** overriding is still supported and still
cheap. `ZORP_SYSTEM` in the environment and `system_prompt` in a flavor
both win over the default, so a coding-focused flavor is a config file,
not a fork. `validate`'s preamble still narrows the frame further for
that one subcommand.

---

## 2026-08-18: the markdown renderer is ours, because the alternative is innerHTML

**Decision:** `web/src/markdown.ts` renders markdown by building DOM nodes.
No markdown dependency, and no `innerHTML` anywhere in the path from model
output to the page. Alongside it, `GET /api/artifacts` and
`GET /api/artifacts/raw` serve files from the directory the server was
started in, and a pane renders them: markdown through the same renderer, PDF
in an iframe. Design:
`docs/superpowers/specs/2026-08-17-artifact-pane-design.md`.

**Superseded by** 2026-08-19: a PDF is read for its text, not framed for its
layout, and only as to the PDF. The iframe never rendered one: the sandbox
this entry relies on is exactly what stops a browser's PDF viewer starting.
Everything else here stands, the sandbox included, which is still what makes
`.svg` and `.html` safe to show.

**Why not a library:** every markdown library worth using returns an HTML
string and hands you the `innerHTML` call. The text being rendered here is
model output, and the model has been reading tool results, web pages and
files, so treating it as trusted markup is cross-site scripting with extra
steps. The old code-block-only renderer already had this property and said
so in a comment; keeping it was the requirement, and writing the renderer
was the only way to keep it while rendering more than code blocks.

Two consequences that needed their own care. A markdown link is the one
construct where model output chooses a URL that the page makes clickable, so
only `http`, `https` and `mailto` become anchors and everything else renders
as visible text. Images are not supported at all, because `![](url)` would
make the page fetch an attacker-chosen URL on render, which is a beacon.

**Why serving files is scoped the way it is:** `zorp-web` already runs
commands and edits files in that directory, so reading from it is not new
reach. The new thing is a door that takes a path from a URL. Paths resolve
against the root and are checked for containment *after* canonicalization,
because looking for `..` in a string does not catch a symlink, and a symlink
inside the workspace can point anywhere. Types are an allowlist rather than
a denylist: an unknown type guessed at as `text/html` is a hole, while an
unknown type refused is an inconvenience. Every response carries `nosniff`
and a `sandbox` CSP, which is what makes it safe to put a PDF in an iframe.

**What it rules out:** rendering raw HTML in markdown, which shows as text
instead; generating PDFs, which needs LaTeX or typst and is its own project;
and editing from the pane, which is read-only. Syntax highlighting is
deferred, and the existing `data-lang` attribute is enough to add it later.

`web/` also gained its first test harness, jsdom plus `node:test`, wired
into the `web` CI job. A security-critical renderer with no automated test
was not worth shipping, and a typecheck does not notice a renderer that
starts producing markup.

## 2026-08-17: open-context connects as an MCP server, and searching your own material is not evidence

**Decision:** open-context is documented as an MCP server that a user
connects, not something zorp depends on, embeds, or starts. The README
carries a `.zorp/mcp.toml` recipe pointing at a local checkout.

Separately, `validate`'s search gate now rejects tool names that carry a
search verb over the user's own stored material (`context`, `memor`,
`note`, `recall`), not only names with no verb at all.

**Why:** zorp already has a general MCP client, so "use open-context in
zorp" needs no new coupling: eleven tools show up prefixed
`mcp__opencontext__` and the agent gets memory that outlives a session.
Wiring a Node package into a Rust product as a soft runtime dependency
would buy nothing this does not.

The gate change is not cosmetic. `mcp__opencontext__search_contexts`
carries "search", so before this, connecting a memory server silently
satisfied the evidence gate, and `validate` would score a question
against notes the user wrote themselves while reporting that it had
searched. A test now pins that, and it failed before the fix.

**Ruled out:** an `npx` recipe. The npm package named `opencontext` is an
unrelated project by another author, so that recipe would have zorp users
download and run a stranger's code.

**Known limit:** the gate is a heuristic over tool names, so it only
covers the case it can see. The durable version is an explicit per-server
flag in `.zorp/mcp.toml`, which needs the server config threaded from
`zorp-mcp`'s registry down into `validate`. Not done here: it changes
public API for a hazard the name check already closes in practice.

## 2026-08-17: model settings resolved server-side, UI over env over default

**Decision:** `zorp-web` gained `GET/PUT /api/settings`,
`GET /api/settings/models`, and `POST /api/settings/test`, all behind the
existing token gate. Settings are resolved on the server with a fixed
precedence per field: a value saved through the settings panel beats the
matching `ZORP_*` env var (`ZORP_PROVIDER`, `ZORP_BASE_URL`, `ZORP_MODEL`,
`ZORP_API_KEY`, `ZORP_MAX_TOKENS`), which beats the hardcoded default
(`https://api.openai.com/v1`, `gpt-4o`). `GET /api/settings` reports
`configured: false` only when every field is still at its hardcoded
default and no key is set anywhere, which is exactly the shape that used
to fail silently: `turn.rs` called `HttpModel::try_from_env()`, which
defaults quietly and let the first message a fresh install ever sent die
on a raw 401 deep inside the provider call. Ollama is not a third
`Provider` variant; it is `OpenAiCompatible` pointed at
`http://localhost:11434/v1`, offered in the UI as a preset. One code path
(`GET {base_url}/models`) lists models for Ollama and OpenAI alike,
verified against a real local Ollama instance during this change.

**Why:** The bug was not "no Ollama support," it was "no way to tell the
server what to talk to from the one place a user actually is: the chat
UI." Browser-shipped provider config per turn was rejected in favor of
server-side settings so the API key never has to leave the machine
running the server, and so the UI stays a thin client for state the
server owns.

**Secrets are held in memory only; non-secrets persist.** `PersistedSettings`
(provider, base_url, model, max_tokens) is written to
`~/.config/zorp/web.toml` on every successful `PUT`. It has no `api_key`
field, so there is nothing on that type to accidentally serialize a key
through. The API key lives in `SettingsState::api_key`, seeded once from
`ZORP_API_KEY` when the server process starts and replaced in memory by a
UI save; it is never written to disk and `GET /api/settings` reports only
`has_api_key: bool`, never the key. This matches the existing stance in
`zorp-agent/src/main.rs` that `api_key` is never read from a flavor
manifest either: secrets stay out of anything that gets committed, synced,
or read back over HTTP.

**What it rules out:** the settings file surviving a restart requires
`main.rs` to load it into `AppState` explicitly at startup; `AppState::new`
and `AppState::with_token` deliberately do not do this themselves, so
`zorp-web`'s existing tests stay hermetic and are not affected by whatever
a developer's own machine happens to have saved.

---

## 2026-08-17: clippy gates CI, on one runner, over all targets

**Decision:** CI runs `cargo clippy --workspace --exclude zorp-track
--all-targets --locked -- -D warnings` in a single ubuntu job. Warnings
fail the build.

**Why:** clippy had never run in CI at all. Nothing gated it, so
warnings accumulated unnoticed. Among them: dead test scaffolding, a
`mut` that was never needed, a length comparison to zero, an argument
list that had outgrown the lint's limit, and a std lock held across an
await in zorp-web's tests. That last one is test-only. It serialises
tests that write process-global env vars, and no production path
touches it. A check that only prints is a check nobody reads, which is
how the pile grew, so this one fails instead. Every warning was cleared
before the gate went in, so it starts from zero.

One runner rather than the two-OS test matrix: every lint clippy found
here is platform independent, and the macOS test leg still compiles the
platform-specific code. `--all-targets` because most of the warnings
were in test code, and linting the library alone would have gated the
easy half.

**What it rules out:** nothing lints `zorp-track` or the `research`
feature. That is deliberate, for the same reason the fast test job skips
zorp-track: it builds DuckDB from source and would dominate the job.
Closing that gap means adding a clippy step to the research jobs, which
already carry the warm shared cache for that tree. It is a known gap,
not an oversight.

## 2026-08-17: the web event stream belongs to the session, not to the turn

**Decision:** `GET /api/sessions/:id/events` holds its response open for as
long as the browser is listening. A finished turn does not end it, and the
next turn streams down the connection that is already open. There is no
"this session is finished" state on the server. A session the server does
not have in memory gets a 404 rather than an empty stream.

**Why:** the stream used to end when a turn ended, and `EventSource`
reconnects on its own whenever a response ends. A finished conversation
therefore opened a fresh connection every three seconds for as long as the
tab stayed open, with the status badge stuck on "reconnecting" and nothing
actually wrong. Measured in a browser against a local model: 43 connections
in 135 seconds from two turns, versus 1 connection now. It was worse than a
cosmetic bug, because `SessionState::finished()` looked for any `Done` in a
backlog that is never cleared, so after the first turn every later stream
closed the instant it opened and the transcript arrived only over the
automatic reconnects. The reconnect storm was load bearing.

**What it rules out:** ending the stream on `done`, on either side. Closing
the `EventSource` when a turn finishes is the obvious tidy-up and it stops
the next turn streaming at all, so both sides now say so in comments. It
also rules out treating an unknown session as an empty stream: that is a
reconnect loop with extra steps, which is what opening a stored session
from the sidebar was after a restart.

The polling loop behind the stream stays, per the original design note that
a chat UI does not need sub-100ms latency. It now walks only the new tail of
the backlog each tick, because the loop runs for hours rather than for one
turn.

## 2026-08-17: one version for the product crates, and a release refuses to disagree with it

**Decision:** `zorp`, `zorp-agent`, `zorp-mcp`, `zorp-track`, `zorp-eval`
and `zorp-web` inherit a single `[workspace.package] version`, bumped
with each release tag. `erbga` keeps its own version. A tag push whose
tag, workspace version and Dockerfile `ARG VERSION` default disagree
fails the release.

**Why:** the versions had drifted from the tags without anyone noticing.
`zorp` sat at 0.1.0 and `zorp-agent` at 0.2.1 across both v0.3.0 and
v0.3.1, so the published v0.3.1 binary answered `--version` with 0.2.1.
That is the release whose first message times out on a cold model, and
v0.3.1 is its fix, so every user who installed the fix and checked was
told they had not got it. The Dockerfile default had rotted the same
way, which meant a bare `docker build` fetched the broken release on
purpose.

**What it rules out:** independent per-crate versioning for the product
crates. If one of them ever needs to be published to crates.io on its
own cadence, this has to be revisited. Nothing needs that today, and the
cost of the current scheme was a public release that lied about which
release it was.

`erbga` is excluded because it is standalone published prior work that
does not ship with zorp, and giving it zorp's release number would claim
a relationship that does not exist.

## 2026-08-17: --version and --help are answered by the binary, not the model

**Decision:** `zorp` intercepts a leading `--version`, `-V`, `--help` or
`-h` and answers locally. Anywhere other than the first argument they are
still part of the prompt.

**Why:** they were joined into the prompt and POSTed to the model, so two
of the first flags anyone types at an unfamiliar binary cost a
completion, and with no key configured the new user's first impression
was a 401 wall of JSON from OpenAI. This is not a departure from "argv is
the prompt": `main` already intercepted `--init`. These were missing from
that list rather than deliberately excluded from it.

**What it rules out:** a prompt whose *first* word is one of those four
flags. `zorp what does --version print` still reaches the model, and
there is a test for it, because that is the behavior the change could
plausibly have broken.

## 2026-08-17: CI decides what to run with git, not a downloaded action

**Decision:** the research-stack path filter is a `git diff` and a grep
instead of `dorny/paths-filter`.

**Why:** the action is fetched from codeload when the job starts, and
when codeload answers 429 or 503 the job fails during setup before
running anything. That happened on three runs in one morning, twice in a
row on a rerun of the same commit, on pull requests that had not touched
the research stack and would have skipped every step anyway. A
dependency that can be unavailable was sitting in the critical path of
every pull request in order to decide to do nothing.

**What it rules out:** the action's richer glob syntax. The filter is six
patterns; if it ever needs more than a grep can express, reconsider. A
diff that fails now assumes the research stack changed, because failing
towards running the tests is the only safe direction.

## 2026-08-16: web search is a capability with a provider behind it, not a Tavily integration

**Decision:** search becomes a new workspace crate, `zorp-search`,
holding a `SearchProvider` trait with Tavily as the first
implementation. `zorp-agent` exposes it as a built-in `web_search` tool
behind a non-default `search` feature, gated at `Decision::Ask`, and
`validate`'s search gate accepts it by exact name alongside the existing
`mcp__` verb matching. The API key comes from `ZORP_TAVILY_API_KEY` and
never from a flavor manifest. Design:
`docs/superpowers/specs/2026-08-16-tavily-web-search-design.md`.

**Why:** `validate` is zorp's entry point and it could not run without
an MCP server, which means installing node, choosing a server, and
writing `.zorp/mcp.toml` before asking the first question. That barrier
showed up in the first-run walkthrough (#2) and forced the first two
UATs to validate against a stub server. Tavily removes the setup chain:
one endpoint, one key, results already extracted. Putting a trait in
front of it keeps the "vendor-neutral harness" claim honest and lets a
second provider land without touching the agent.

**Why `search` is separate from `research`:** this is the first built-in
tool that sends anything over the network, and the thing it sends is the
user's hypothesis. Running `investigate` locally should not acquire an
egress path by side effect, so the feature is opted into on its own.
For the same reason the tool asks rather than being allowed like a local
read, and a failed search is an error rather than an empty result set,
since a validate novelty score cannot tell those apart and would record
a wrong number.

**What it rules out:** bundling an MCP server (Tavily publishes one and
it stays a fine option); implicit searching outside a visible tool call;
and a result cache, which is deferred because caching changes what "the
evidence said" means across a re-run and interacts with pre-registration
in ways that need their own decision.

---

## 2026-08-16: everything zorp writes into a project lives under .zorp/

**Decision:** the `take_note` and `search_notes` tools write to
`.zorp/notes/` instead of `.qkb/`. No migration is provided and no
fallback read of `.qkb/` is kept.

**Why:** `.qkb` is a quecto-era name that the rename missed. Crates,
binaries, env vars, and the config directory all became `zorp`, and the
research stack already writes `.zorp/flavor.toml`, `.zorp/mcp.toml`,
`.zorp/tracks/`, and `.zorp/zorp.duckdb`. A second, differently named
directory appearing in a user's repo the first time an agent takes a
note is a leak of the fork's history into a user's project. Found in the
first UAT (`docs/uat/UAT-report.md`, F2).

**What it rules out:** carrying a compatibility read of `.qkb/`. zorp is
pre-alpha and the notes tools have no install base worth a migration
path; a fallback would keep the old name alive in the code indefinitely
to serve nobody. Notes already written to `.qkb/` stay on disk and stop
being found. The bootstrap decision (2026-08-08) dropped quecto's own
`.qkb/` directory as a session artifact but left the tools that create
it; this finishes that.

---

## 2026-08-15: evolve's search layer is not approved, its measurement discipline is

**Decision:** the `evolve` spec is marked NOT APPROVED and nothing is
built from it. Two rounds of adversarial review, eight reviewers, found
the search layer unsound both times, and the second round showed the
first rewrite had moved the flaw rather than removed it. `erbga` ships on
its own terms as a validated implementation of prior work, off any
critical path.

**Why:** three findings compose into one conclusion. There is no free
inner search, because variation is model-proposed, so the affordability
argument the whole design rests on is false. The framing score is
maximized by an undifferentiated blob, because CPM's objective is
extensive and edge addition was priced free. And two of its three score
terms are identically 1.0 by construction, which is the same defect as
the draft before it under new names. At `V = 20` an exact
clique-partitioning ILP solves the partition to proven optimality in
about 0.2 seconds, so the cut was never the hard part; the framing is,
and there is no cheap search over framings either.

**What survives, and should be built on ordinary `investigate` runs:**
never selecting on the pre-registered metric (breeding toward a metric
and then reporting it is biased upward twice, and pre-registration does
not cover selecting the observation that best clears a fixed test); the
confirmatory stage of `n` passes with the threshold on the mean and nulls
counted as non-passing; refusing to call framing diversity corroboration
when all lines share one model; quorum rather than unanimity for track
death.

**Bugs found in shipped code, worth fixing regardless:**
`TrackStatus::from_str` and `ExperimentStatus::from_str` both have
catch-all arms that silently coerce an unknown status to `Active` and
`Planned`. For a product that must not let a non-result look like a live
result, that is the worst possible default.

**Full writeup:** `docs/superpowers/specs/2026-08-14-zorp-evolve-design.md`,
whose "Where this stands" and "Review record" sections carry the detail.

---

## 2026-08-14: a fifth capability, evolve, searches question framings and never selects on the metric

**Superseded by** the 2026-08-15 entry above, one day later. Nothing
below was built. The search layer described here is not approved and the
spec it points at is marked NOT APPROVED. `erbga` did ship, on its own
terms and wired to nothing. The rest of this entry is left as written,
because the findings against it need something to point at.

**Decision:** zorp gains `evolve`. It searches for a good way to
**decompose** a question, not for an answer. A population of framings
(sub-questions, a weighted "bears on" relation, marked cross-cutting
premises) evolves against a deterministic structural score, and the
partition of any given framing is solved directly rather than evolved.
The pre-registered metric appears nowhere in selection; it is measured
once per island after the search, over n independent synthesis passes,
with the threshold applied to the mean. Output is a distribution and its
dissent, not a number. Ships as an `erbga` crate (the algorithm of Rao,
Janikow, Bhatia, Climer, MWAIS 2018, zorp's author's prior work, used as
the partition solver for large framings and validated against that
paper's benchmarks) plus `zorp-agent/src/evolve/`.

**Why:** breeding a population to maximize a metric and then reporting
that metric is biased upward twice over. Framings that surface
inconvenient evidence score worse and stop breeding, and the maximum of
N noisy evaluations exceeds the truth by about sigma*sqrt(2 ln N).
Pre-registration does not cover this: it stops you moving the test after
seeing data, not selecting the observation that best clears a fixed
test. Separately, the uncertainty in this problem lives in the graph,
which a model invents and which was never revisited, not in the cut,
which is a small solvable problem. So the compute moved to the framings.

**What it rules out:** modularity as the objective, since its resolution
limit does not bind on the source's benchmarks but binds on every
realistic question graph, so CPM with a pre-registered gamma is used
instead. Corroboration as a claim, since all islands share one model;
the property is renamed framing diversity, cross-island evidence reuse
is forbidden for anything feeding a reported result, and component
source overlap is measured rather than assumed. Track death by
unanimity, replaced with a pre-registered quorum. A recorded-only
parameter tier, since when the result depends on a search, search effort
is answer selection, so every input is pre-registered. Also gone: Gene
Repair at this layer (the source's own results show accuracy degrading
monotonically with density with it enabled), and the claim that
evidence cost is bounded by vertex count.

**Full writeup:** `docs/superpowers/specs/2026-08-14-zorp-evolve-design.md`,
whose closing section records what four adversarial reviews changed.

---

## 2026-08-14: stdio MCP reads get a deadline, and unadvertised features are not probed

**Decision:** `StdioTransport` reads its child's stdout on a helper
thread and waits with a deadline, so `timeout_secs` now means the same
thing for stdio that it already meant for the HTTP transports. A blown
deadline is a real `McpError::Timeout` naming the server. The transport
also closes stdin and reaps the child on drop. Separately,
`list_prompt_names` returns empty without sending anything when
`initialize` did not advertise a `prompts` capability; a server that
reports no capabilities block at all is still asked, since "did not
say" is not "does not have".

**Why:** Found by end-to-end UAT. `timeout_secs` was accepted and
silently dropped on the stdio path, and a pipe read cannot carry its own
deadline the way a `ureq` call can. Any stdio server that took a request
and never answered wedged the whole CLI with no output and no way out
but Ctrl-C. That is not a hypothetical shape: zorp asked every server
for `prompts/list` whether or not it offered prompts, and a server is
free to ignore a method it never claimed. The capability gate removes
the common case, the deadline covers the rest, and reaping on drop keeps
a wedged server from outliving the process it was spawned for.

**Ruled out:** non-blocking pipes plus `poll`, which is Unix-only and
buys nothing over a reader thread here.

---

## 2026-08-14: subcommands win over the bare-task positional

**Decision:** the CLI uses `subcommand_precedence_over_arg` instead of
`args_conflicts_with_subcommands`. A task whose first word is a
subcommand name is still reachable with `--`.

**Why:** with the old setting, any global flag before a subcommand made
the trailing task positional swallow the subcommand. `zorp-agent --yes
undo` did not undo anything: it sent the word "undo" to the model as a
task, ran an agent with auto-approval on, printed whatever came back,
and exited 0. Every subcommand was affected, and it failed silently in
the direction that looks like success, which is the worst way for a CLI
to be wrong.

---

## 2026-08-14: kill thresholds carry a direction, and are enforced

**Decision:** a pre-registration now records a threshold direction
(`lower-is-better` or `higher-is-better`) alongside the metric and the
number, and `investigate` compares each recorded attempt against it. A
breach kills the track. `--threshold-direction` is required whenever a
threshold is set, the direction lives in `prereg.md` (so the existing
SHA-256 hash and git commit cover it), and it has its own column in the
`preregistrations` table.

**Why:** the threshold was only ever formatted into a prompt string and
never compared to anything, so a track that badly missed its own
threshold stayed Active. That is the one guarantee the whole product
rests on. A bare number could not be enforced even in principle, since
nothing said which side of it was failure.

**What it rules out:** guessing. A breach is exempt from
`AutoApprove`/`--yes`, because auto-approving the one decision that
exists to stop a run defeats the point. A legacy pre-registration with
no recorded direction is skipped with a loud warning rather than
enforced against an assumed direction, since guessing wrong would kill
healthy tracks.

---

## 2026-08-14: git is the root of trust for pre-registration integrity

**Decision:** rebuilding the evidence store from `prereg.md` files no
longer trusts the files on disk. The rebuild hashes the committed git
blob and compares it against the working tree; a mismatch is an
integrity error rather than a fresh row. A file with no commit behind it
is marked unverified instead of being presented as equivalent to a
committed one. `verify_prereg_integrity` now also checks the recorded
`git_commit_hash`, which was previously written but never read.

**Why:** the recovery path recomputed the hash from whatever was on disk
and stored that as authoritative, so deleting the DuckDB row or
corrupting one byte of the store turned a tampered pre-registration into
a verified one. The tamper-evidence guarantee was defeated by the
recovery path meant to protect it. Two existing tests asserted this
behavior as correct and were rewritten.

---

## 2026-08-14: the vector library is opt-in, not part of research

**Decision:** LanceDB moves behind a non-default `library` feature in
`zorp-track`, with a matching opt-in feature in `zorp-agent` that
`research` deliberately does not enable. `Project::library` opens
lazily, and `validate` skips the embed-and-insert step when the feature
is off.

**Why:** it was a write-only sink. `validate` wrote cited sources into
it, nothing ever read them back, and the citations `co-write` actually
uses come from the DuckDB `validations` columns. It cost roughly 390 of
`zorp-track`'s dependencies (the whole arrow and datafusion tree) for no
behavior. It stays available rather than deleted, because a retrieval
story is a plausible future.

---

## 2026-08-14: measurement code fails loudly instead of guessing

**Decision:** `zorp-eval` gained three honest non-result states rather
than folding unevaluable runs into pass or fail. An unreadable trace
records `trace_unavailable` and skips contract evaluation entirely,
malformed lines inside a valid trace are skipped and counted in a new
`runs.trace_malformed_lines` column, and ordering predicates over
seq-less events report `unevaluable`. Unknown predicate ids are a
load-time hard error. The unimplemented LLM grader and the `eval`
subcommand now return not-implemented errors instead of reporting
success.

**Why:** every one of these paths previously produced a confident,
recorded result from evidence that was never actually evaluated. A
truncated final trace line became "all contracts failed"; a typo in a
contract id became a permanent violation or a silent pass. For a harness
whose only purpose is trustworthy measurement, a fabricated result is
worse than a missing one.

---

## 2026-08-14: command policy analyzes substitutions and redirect targets

**Decision:** the run_command denylist now recurses into `$(...)`,
`<(...)`, and `>(...)` bodies the same way it already did for `sh -c`
payloads, tokenizes redirect operators as distinct tokens and checks
their targets, and denies destructive `rm` whose targets escape the
repository root. Unbalanced substitution syntax fails closed. `>
/dev/null` is now explicitly allowed, where the old substring check
denied it.

**Why:** `$` was an ordinary word character to the tokenizer, so
`echo $(sudo rm -rf /)` parsed as a call to `echo`, resolved to Ask, and
ran under `--yes`. The redirect check matched four literal spellings and
missed `> ~/.ssh/authorized_keys`. The root-rm guard matched only a bare
`/`, so `rm -rf /*` passed. These were holes in an otherwise careful
fail-closed design, not a missing design.

---

## 2026-08-14: CI covers the research stack, and the lockfile is committed

**Decision:** `Cargo.lock` is tracked and CI builds with `--locked`. The
research stack (`zorp-track` plus `zorp-agent --features research`) gets
its own job, running nightly and on pull requests that touch it, while
the per-PR fast path still excludes `zorp-track`. Added a macOS matrix
leg and a `cargo fmt --check` gate. `panic = "abort"` is gone from the
release profile.

**Why:** an entire crate and a feature-gated surface could stop
compiling while main stayed green, which is exactly what "excluded from
CI" means over time. An untracked lockfile made builds
non-reproducible and degraded cache hits. `panic = "abort"` silently
disabled the `catch_unwind` guard around subagent execution in every
release build, so a subagent panic killed the whole process in
production while passing in tests.

---

## 2026-08-13: paper rebuilt as a real arXiv preprint, with a bibliography

**Decision:** the paper is now built through a proper LaTeX toolchain
rather than pandoc's defaults. `docs/paper/arxiv-template.tex` is a
pandoc template implementing the standard arXiv preprint presentation
(Times via `newtx`, ruled abstract block, numbered sections, small-caps
running head), `docs/paper/references.bib` is a real bibliography, and
`docs/paper/Makefile` runs the full pandoc, pdflatex, bibtex, pdflatex,
pdflatex cycle and cleans up after itself. Figures were regenerated in
Times to match the body text, in a restrained academic style rather
than a marketing one. The paper itself was rewritten: a Design
Principles section now states the three commitments the system is
organized around, a Limitations section states what the tamper-evidence
guarantee does and does not cover (it is scoped to the pre-registration
file and inherits git's trust model), and claims are cited throughout.

**Why:** the previous draft had no citations at all, which no venue
would take seriously, and its Helvetica-on-pandoc-defaults presentation
read as a rendered README rather than a paper. The bibliography also
does real argumentative work: zorp's central mechanism, committing a
falsification threshold before gathering evidence, is the
pre-registration literature's answer to undisclosed analytic
flexibility, and the paper is stronger for saying so and citing it than
for presenting the idea as novel. The two AI-Scientist references were
verified against arxiv.org directly, since they carry the paper's
framing; `docs/paper/README.md` flags that the rest need a verification
pass before submission.

---

## 2026-08-13: paper corrected to zorp-landing's real branding and arXiv formatting

**Decision:** the first paper draft (below) used the wrong logo (the
purple-on-dark favicon glyph) and an assumed dark/purple color scheme
that doesn't match zorp-landing. Corrected: the header and title page
now use the real zorp mark (a node-and-edge "Z", navy with two
electric-blue accent nodes), and all three generated figures were
recolored to zorp-landing's actual palette
(`zorp-landing/src/styles/tokens.css`: light theme, `--z-fg` navy,
`--z-accent` electric blue). The paper's language was also brought in
line with the live site (`zorp-landing/src/config/site.ts`): opens with
the site's own hook ("Answers are cheap. Evidence is not."), names the
kill threshold the way the site does, and documents the real six-table
evidence store (`tracks`, `preregistrations`, `experiments`, `metrics`,
`checkpoints`, `validations`) rather than a general two-store
description. Test and line counts were re-verified directly against the
repo rather than trusted from the landing page, which turned out to be
stale in places (a screenshot asset showed a different tagline and an
older test count than the current site config); the paper now cites
538 passing tests and 24,965 lines, both freshly confirmed against
HEAD. Formatting changed to a single-column arXiv-preprint style:
numbered sections, colored running header, boxed abstract. Also, per
house style (`CLAUDE.md`), all em/en dashes used as punctuation were
rewritten as periods, commas, colons, or plain hyphenated compounds.

**Why:** a systems paper about zorp representing zorp with the wrong
brand mark and colors undermines the credibility it's trying to
establish. Trusting a stale marketing asset over the actual repository
state would have repeated the same kind of unverified-claim mistake the
paper explicitly argues against.

---

## 2026-08-13: zorp's own arXiv-style systems paper written, first draft

**Decision:** `docs/paper/zorp-paper.md` is a first draft of the systems
paper about zorp itself, scoped in `docs/paper/README.md` since
2026-08-09. Covers architecture, the `zorp-track` foundation, and the
four capabilities, all grounded in what's actually built and tested as
of this writing (real test counts and LOC, not aspirational numbers).
Figures (layered architecture, the four-capability pipeline, test
counts) are generated from real repo state by `docs/paper/make_figures.py`,
not mocked up. The header logo is redrawn from
`zorp-landing/public/favicon.svg`. Built to PDF via
`pandoc --pdf-engine=xelatex`; both the Markdown source and the rendered
`zorp-paper.pdf` are committed.

**Why:** the paper needs a real eval story before it's submittable
anywhere ranked (see `docs/paper/venues.md`), which doesn't exist yet.
This draft is explicit about that gap: it reports the design and what's
tested, and lists the comparative evaluation against AI-Scientist-v2 as
future work rather than fabricating numbers to fill the gap. Posting to
arXiv as a preprint doesn't need review or a deadline, so this draft can
go out regardless of the ranked-venue timeline in `venues.md`.

---

## 2026-08-13: README/CONTRIBUTING default to excluding zorp-track, and default-run fixes the ambiguous zorp-agent binary

**Decision:** README and CONTRIBUTING now lead with
`cargo build --workspace --exclude zorp-track` / `cargo test --workspace
--exclude zorp-track` instead of the plain `--workspace` forms, with a
note on the 20-30 minute cold build if `zorp-track` is included. README
also gained a short section on connecting an MCP tool for
`validate`/`deliver`, since neither can run without one and the failure
mode gave no pointer to the fix. Separately, `zorp-agent/Cargo.toml`
now sets `default-run = "zorp-agent"`, since the test-only
`stub_search_mcp_server` binary made `cargo run -p zorp-agent -- "<task>"`
(the exact command in the README) fail with an ambiguous-binary error.
`src/main.rs`'s one-shot path also now prints a hint to set
`ZORP_API_KEY` when a request fails with no key configured, instead of
just surfacing the raw HTTP status.

**Why:** usability testing of the public-release candidate found that a
first-time user following the README verbatim hit all four of these in
the first few minutes: an unexplained 20-30 minute build, a broken
documented command, an unrunnable flagship capability with no
documented fix, and an opaque 401 with no actionable next step. Each is
a small, targeted fix; none change behavior for anyone already working
around them.

## 2026-08-13: CI excludes zorp-track from the default workspace test run

**Decision:** `.github/workflows/ci.yml` runs
`cargo test --workspace --exclude zorp-track` instead of
`cargo test --workspace`, and no longer runs
`cargo test -p zorp-agent --features research` at all. Both remain
required locally before considering Rust changes done (see CLAUDE.md);
CI just doesn't enforce them yet.

**Why:** `zorp-track` is the only workspace crate depending on `duckdb`
(bundled — compiles DuckDB's C++ amalgamation from source) and
`lancedb` (pulls in Arrow and DataFusion transitively). On a cold
cargo cache, compiling it took CI past 20+ minutes on GitHub's shared
runners, several times in a row, before `Swatinem/rust-cache` ever got
a chance to save a cache (it only saves on successful job completion,
so a run cancelled for taking too long guarantees the next run is cold
too). Excluding it keeps CI fast and cheap for every other crate, at
the cost of not catching zorp-track/research-feature regressions in CI
until a better strategy (self-hosted runner, a slower opt-in workflow,
or seeding the cache once) is worth the cost.

**Ruled out (for now):** letting one run finish uncapped to seed the
cache, and requesting a larger/paid runner — both reduce or shift cost
rather than remove it, so scope reduction was chosen instead.

---

## 2026-08-09: deliver's design: huiban-only, academic venues only, checkpoint doesn't kill the track

**Decision:** `deliver` is scoped to academic venue-matching only for v1,
not the broader "right format for any audience" language used elsewhere.
It requires a `draft.md` (from `co-write`) and a huiban-prefixed MCP
tool to be configured, checked the same way `validate` requires a
search-capable tool. The agent uses huiban to find and rank real
conferences and journals fitting the draft's scope, writes the shortlist
to `venues.md`, and checkpoints it. Rejecting the checkpoint does not
kill the track, matching `co-write`'s behavior, not `validate`'s or
`investigate`'s.

**Why:** A non-academic artifact has no equivalent of a "venue" in the
same concrete sense a paper does, and a generic reformatting mechanism
for arbitrary audiences is a different, larger problem than a first
version needs to solve. Requiring huiban specifically, rather than
falling back to generic search, avoids weak or fabricated venue matches
from a tool not built for this. Not killing the track on rejection
matches `co-write`'s reasoning: a shortlist not being good enough isn't
evidence anything upstream failed.

**Ruled out:** A general "format for any audience" mechanism for
non-academic artifacts (would need its own design if it becomes a real
need, not a bolt-on here). A shipped, static venue catalog (already
ruled out earlier in the decision log). Falling back to generic web
search when huiban isn't configured.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-deliver-design.md`

---

## 2026-08-09: co-write's design: grounded drafting, no post-hoc claim-check, rejection doesn't kill the track

**Decision:** `co-write` hands the agent the track's actual recorded
evidence (validate's verdict if present, every metric investigate
recorded) as structured data in the prompt and instructs it to cite only
those figures, rather than drafting freely and then scanning the output
to verify numeric claims afterward. Requires at least one recorded
metric to run at all. The agent's answer is written directly to
`draft.md`, no scored JSON block. Unlike validate and investigate,
rejecting co-write's checkpoint does not kill the track: a draft not
being ready isn't evidence the investigation failed.

**Why:** Grounding at the input side (only real numbers ever reach the
model) is simpler and more reliable than extracting and re-verifying
numeric claims from free-form prose after the fact, which is a much
harder problem with its own false-positive/negative risk. Requiring a
metric to exist keeps co-write from drafting off a validate pass alone,
which is a go/no-go check, not evidence. Not killing the track on
rejection matches the normal expected path once a draft exists: a human
takes over editing `draft.md` directly, or the call runs again.

**Ruled out:** A post-hoc claim-check pass over the drafted prose.
Tamper-evidence hashing of `draft.md` like `prereg.md`'s SHA-256 (a
mtime-based warning only, not an integrity guarantee). Killing the track
on a rejected co-write checkpoint.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-co-write-design.md`

---

## 2026-08-09: investigate's design: CLI-supplied prereg, one attempt per call, checkpoint decides kill

**Decision:** `investigate` takes `--metric-name`/`--kill-threshold` as CLI
arguments (not agent-proposed) the first time it runs for a track, writes
and checkpoints the pre-registration, then runs exactly one attempt per
invocation, records a typed metric via the existing `zorp-track`
experiment tables, and hands the kill/keep decision to a human checkpoint
rather than comparing the metric to the threshold in code.

**Why:** A human-committed threshold is the whole point of
pre-registration; an agent-proposed one would defeat it. One attempt per
invocation keeps every attempt visible at a checkpoint instead of burning
budget inside a single call before a human sees anything. No stored
"kill direction" (above/below is favorable) means no risk of that logic
guessing wrong; the checkpoint prompt shows the human the number and the
threshold and lets them decide, matching the existing "no hard experiment
budget" decision.

**Ruled out:** Multi-attempt loops within a single invocation. Automatic
threshold comparison deciding kill/keep without a human. Requiring a
prior `validate` approval before `investigate` can run (the existing
standalone-capabilities decision already rules this out).

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-investigate-design.md`

---

## 2026-08-09: validate's design: MCP-only search, two-dimension rubric, new embedding env var

**Decision:** `validate` searches through whatever search-capable MCP
servers the user has configured, no built-in search provider. Embeddings
for LanceDB come from a new `ZORP_EMBEDDING_MODEL` env var, hitting the
same `ZORP_BASE_URL` the chat model already uses. Scoring uses two
domain-agnostic dimensions, redundancy (has this already been answered)
and feasibility (can this be investigated), not Catalyst's four
academic-specific dimensions, each requiring a citation from retrieved
evidence or scoring 0.

**Why:** No built-in search provider keeps zorp vendor-neutral and
avoids owning an API key/provider zorp would have to maintain; MCP
already exists for exactly this. Reusing `ZORP_BASE_URL` for embeddings
matches how the rest of zorp is configured, one new env var instead of a
new provider abstraction. Two dimensions instead of four because
"novelty" and "prior-art distance" don't mean anything for a Kafka
migration question; redundancy and feasibility do, for any domain.

**Ruled out:** Shipping a default search provider (scope creep for a
first version, and a provider decision zorp shouldn't own). A structured
sources table (citations as free text for now; revisit only if
co-write's claim-check needs more).

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-validate-design.md`

---

## 2026-08-09: research means investigation, not academia, zorp's scope broadens

**Decision:** zorp targets any question that can be turned into a
defensible answer using evidence, not just academic research. The
primitive: question, investigation, sources, evidence, conflicting
evidence, reasoning, validation, answer or artifact. This covers
technical decisions, product questions, competitive analysis, investment
theses, market sizing, scientific hypotheses, engineering choices, due
diligence, strategy, and ordinary high-stakes personal decisions.
Academic research is one instance of this, not the whole of it. The four
capabilities are renamed to match: validate (unchanged), experiment
becomes investigate, co-write (unchanged), find a venue becomes deliver.
Positioning drops "AI research agent" for "Zorp investigates hard
questions and delivers evidence-backed answers," with the tagline "LLMs
made intelligence cheap. Zorp makes validated intelligence cheap."

**Why:** Stated directly, with the reasoning that "research" read as
academic-paper-only undersells the product and caps the market. The
underlying architecture didn't need to change to support this: the
pre-registration discipline (commit a method and threshold before
evidence exists) was never actually academic-specific, "migrate off
Kafka if a spike test shows 20% latency improvement" is the same shape
as a scientific hypothesis. Only the language was narrow.

**Ruled out:** Rebuilding `zorp-track` or the architecture to support
this. Nothing about the foundation assumed an academic domain; broadening
the scope is a naming and product-language change, not a structural one.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-scope-and-positioning.md`

---

## 2026-08-09: eight decisions from an interview round on the open questions

A short interview to work through the open questions left in the
architecture proposal. Each is small enough to log together; none of
them has a full writeup beyond this entry.

**One binary, not two.** The four capabilities (validate, experiment,
co-write, find a venue) ship as new subcommands on `zorp-agent`, not a
separate `zorp-research` binary. Parallel experiment workers still run
as isolated subprocesses, but by having `zorp-agent` spawn more copies of
itself, not a second program. Keeps one thing to install and learn.

**Pre-registration is always required, not optional.** Every experiment
writes its hypothesis, metric, and a numeric kill threshold as its own
commit before any experiment code runs, the same discipline
lab-engine/Catalyst's idea triage already uses and for the same reason:
it stops the threshold from being quietly moved after seeing results.

**No hard experiment budget.** Catalyst caps experiments at 150 lines of
code, 10 minutes, no GPU. zorp ships sane default guidance but doesn't
hard-enforce it, since zorp is general-purpose and a cap tuned for
Catalyst's small validation experiments could be wrong for what zorp's
users actually run.

**Checkpoints are interactive by default.** The three research-loop
checkpoints (after validate, after experiment, before co-write finalizes)
default to asking a human, the same default `zorp-agent`'s existing
per-tool-call approval gate already uses. An explicit flag allows
unattended full-loop runs.

**Run record metrics are typed key-value pairs, not narrative logs.**
Every experiment attempt records explicit, named, typed values (for
example `accuracy: 0.87`) in DuckDB columns, alongside the free-form
logs. This is what lets the co-write claim-check compare a number the
draft cites against something structured, not an LLM's read of raw
stdout.

**Venue matching calls a live venue API, not a shipped catalog.** Confirmed
using huiban (the conference/journal database used to research zorp's own
venues earlier this session) as the model: query for current deadlines
and rankings rather than shipping and maintaining a dataset that goes
stale between releases.

**Multi-track from day one.** zorp supports multiple concurrent research
investigations from the start, closer to ORR's track model, rather than
one-at-a-time with multi-track added later. Chosen over the YAGNI
default specifically to avoid a data-model migration once real usage
exists.

**Venue matching runs on an abstract and contribution summary, not the
full paper.** Enough signal to match scope and contribution type, and it
can run as soon as co-write has a draft abstract, before the full paper
is finalized.

---

## 2026-08-09: two data stores, split by job, not one general-purpose one

**Decision:** DuckDB for the transactional and analytical record (the
run record: experiment status, stage transitions, metrics as structured
columns), LanceDB for multimodal, semantically searchable content
(literature embeddings, paper text and figures, plots, anything the
validate and find-a-venue capabilities need to search over by meaning
rather than by exact field).

**Why:** They're solving different problems. DuckDB's `duckdb-rs` is
synchronous, modeled on the same interface as `rusqlite` (which
`zorp-agent` already uses for session persistence), with full transaction
support, and it's also a real analytical engine, so aggregating metrics
across many experiment attempts is a native strength, not an afterthought
the way it would be on a plain OLTP store. LanceDB is embedded, built on
Arrow, and handles vector similarity, full-text, and multimodal data
(text, images) in one store, which is exactly what novelty checks and
venue matching need. Neither is a hard dependency on Aviskaar-private
infrastructure; both are embedded, no server, ship inside zorp itself.

**Ruled out:** One store trying to do both jobs. A vector database
forced to also be the transactional state machine, or a relational store
pressed into semantic search, would compromise on whichever job it does
second-best. This also supersedes an earlier verbal suggestion to reuse
`rusqlite` for the run record; DuckDB was chosen instead for the added
analytical capability.

**Async boundary:** LanceDB's Rust API is async (tokio). `duckdb-rs` is
synchronous. Both live above `zorp` core, same as `zorp-mcp` and the
`otel` feature already do, without touching the core's deliberately
synchronous design.

---

## 2026-08-09: zorp's own arXiv paper is about the harness, not a discovery it made

**Decision:** The paper zorp itself publishes to arXiv is a systems paper
describing zorp: its minimal-harness design, its lineage from quecto, and
its evals/benchmarks (including comparisons against heavier frameworks
like Sakana's AI-Scientist-v2 and Aviskaar's own lab-engine/Catalyst
pipeline). It is not a scientific-discovery paper produced by using zorp
to research some unrelated topic.

**Why:** Stated directly. This scopes `docs/paper/` and keeps it separate
from the product itself: what zorp offers users (validate an idea, run
experiments, co-write a paper, find a venue) is general-purpose and not
about zorp. What zorp publishes about itself is a tools paper, the same
genre as the papers describing AI-Scientist-v2 or other agent harnesses.

**Ruled out:** Treating "zorp writes a research paper" as meaning zorp
needs to autonomously produce a novel scientific result end to end before
it counts as done. That's a much larger, different bar than a systems
paper needs, and conflating the two would have quietly inflated scope.

---

## 2026-08-09: zorp's product is four standalone capabilities, human-authored papers only

**Decision:** zorp offers four capabilities that each work standalone,
chained by human checkpoints when used as a full loop: validate an idea
(literature/novelty check), run experiments (staged, sandboxed, every
attempt recorded), co-write a paper (zorp drafts from the run record, a
human edits and is the author of record, zorp never outputs a paper as
"done" on its own), and find a venue (match a finished paper against a
conference/journal catalog). zorp does not take a hard dependency on
Aviskaar-private infrastructure (ORR, lab-engine/Catalyst); an ORR
adapter can be optional and later, not the foundation.

**Why:** Most "AI scientist" agents assume the deliverable is a finished,
autonomously written paper. That's the wrong shape: AI-authored papers
are rejected outright at most venues, so the paper step has to be
collaborative, with the human as author, not a generator with a
claim-check pass bolted on. The standalone-capabilities framing also
matches how the harness will actually get used: someone validating one
idea, or just running an experiment, without wanting the full loop.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-architecture-design.md`.

---

## 2026-08-08: No em dashes or en dashes in repo prose

**Decision:** README, docs, commit messages, and comments in this repo
use plain punctuation (periods, commas, colons, plain hyphens) instead of
em dashes or en dashes, so the writing reads as plainly human as the code
itself.

**Why:** Requested directly, to keep the project's public-facing writing
from reading as AI-generated.

**Recorded in:** `CLAUDE.md` and `AGENTS.md` under "Writing style," so it
applies to all future writing in this repo, not just this pass.

---

## 2026-08-08: README rewritten as a full project front page

**Decision:** README expanded to badges, a why/architecture/getting-started/
status structure, with every command and env var in it checked against
the actual source rather than assumed.

**Why:** The repo will be public eventually and needs a front page that
holds up, not a stub.

---

## 2026-08-08: Harness renamed from quecto to zorp

**Decision:** All crates, binaries, env vars, and CLI/log strings renamed
from `quecto*` / `QUECTO_*` to `zorp*` / `ZORP_*`. quecto's own historical
docs (changelog, UAT reports, issue log) moved to `docs/upstream-quecto/`
and left untouched, since they document quecto's own history under its
own name.

**Why:** Deferred at bootstrap time on purpose (see the entry below), then
done once the base harness was in place and it was clear what was
actually being kept versus rebuilt.

---

## 2026-08-08: quecto vendored as the base harness, AI-Scientist-v2 kept local-only

**Decision:**
- [quecto](https://github.com/adityak74/quecto) (MIT) vendored as a
  source snapshot at the zorp repo root, fresh git history, crate names
  left as `quecto-*` initially (renamed later, see above).
- [AI-Scientist-v2](https://github.com/SakanaAI/AI-Scientist-v2) cloned
  into `reference/`, gitignored, used for design inspiration only and
  never committed.

**Why:** quecto is MIT licensed, so forking and modifying it directly is
safe. AI-Scientist-v2 uses a custom, restrictive "Responsible AI Source
Code License" that shouldn't travel with zorp's public repo, so it stays
local-only.

**Full writeup:** `docs/superpowers/specs/2026-08-08-zorp-bootstrap-design.md`
