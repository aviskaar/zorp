---
status: accepted
date: 2026-08-22
---

# one HTTP agent, so the streaming path cannot be the unbounded one

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
