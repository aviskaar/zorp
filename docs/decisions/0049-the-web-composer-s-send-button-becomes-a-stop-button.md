---
status: superseded
date: 2026-08-19
---

# the web composer's send button becomes a stop button

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
