---
status: accepted
date: 2026-08-19
---

# the browser can stand its approvals down, per session, loudly

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

**Superseded by:** 2026-08-31, "auto-approve loses the banner, gains a
reviewer," for the "what it costs" paragraph only. The banner is gone;
the pill and the transcript line stay.
