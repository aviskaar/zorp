---
status: accepted
date: 2026-08-17
---

# the web event stream belongs to the session, not to the turn

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
