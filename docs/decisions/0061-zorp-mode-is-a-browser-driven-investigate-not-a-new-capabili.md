---
status: superseded
date: 2026-08-21
---

# Zorp mode is a browser-driven investigate, not a new capability

**Superseded in part by** 2026-09-12, which keeps auto-approve as the
default but lets a person opt into answering checkpoints in the browser.
Everything else here still holds.

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
