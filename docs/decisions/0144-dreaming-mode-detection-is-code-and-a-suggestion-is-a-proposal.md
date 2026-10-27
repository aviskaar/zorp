---
status: proposed
date: 2026-10-27
---

# Dreaming mode: detection is code, and a suggestion is a proposal, never a memory

**Status is `proposed`, not `accepted`.** This is the spec-and-ADR-first step
the issue asks for (#296): the design is written down and waiting on a
person's approval before any code. Nothing in this PR implements it.

**Decision (proposed):** add a Dreaming mode. While zorp is idle, a background
worker looks over how a person has worked and comes back with suggestions that
would improve their experience: a flavor for a report they ask for every
Monday, filing three conversations about one project together, the fix they
used last time for a command that keeps failing. It is off by default, started
by a person, and reads only the local store and index.

The reason this needs an ADR is that, described loosely, it is exactly the
shape ADR 0063 forbids: an agent writing down what it concluded about the
person and reading it back later as fact. zorp deliberately has no claim table
and no stored sentence a model composed about the person, because the agent's
guesses become its own evidence. Dreaming mode is built to keep that rule, and
here is how.

**Detection is code; the model only words it.** Patterns are found by code
over code-derived columns: message timestamps, which tools ran and their
code-derived status, project membership, and repeated near-identical first
messages by embedding similarity. This is the same split `critique` and the
aryabhatta detectors already use. A detector returns the evidence rows, never
prose. A model is then asked to phrase a suggestion from the pattern and the
verbatim messages it came from, inside the fenced, nonce-marked `user` message
`memory` already uses, so the material it reads is data and not instructions.
No detector reads a column of model-authored text, for the same reason the
search layer may not (2026-08-19): doing so would make the agent's own
speculation tomorrow's observation.

**A suggestion is a proposal, never a memory.** It is shown to the person with
its evidence (links to the exact conversations) and does nothing until they
accept it. Accepting it creates an ordinary thing a person could have made by
hand: a project, a flavor, a skill, a scheduled task, through the same code
paths a person uses. A flavor a dream writes is untrusted until somebody
trusts its hash, exactly as any project flavor is (ADR 0123). A rejected
suggestion is recorded as rejected, so it is not raised again.

**Suggestions never reach the recall index, the memory block, or `messages`.**
They live in their own `suggestions` table, labelled model-authored, kept
apart from evidence the way `sessions.display_title` is kept apart from `task`
(ADR 0069). The table holds the kind, the evidence ids (not copied text), the
model-authored wording, and the state (`new`, `accepted`, `dismissed`).
Nothing that feeds a model reads it. A test like
`there_is_one_chunker_and_one_fingerprint` pins that the suggestion wording
never appears in `messages`, the recall index, or a memory block.

**It stays local, and a person starts it.** The dream pass is a background
worker beside the recall sweep (ADR 0079), gated on `ZORP_DREAM=1` and an
interval setting, off by default. Any model call uses the person's configured
model and the local store and index only. No model tool turns it on, starts a
pass, accepts a suggestion, or dismisses one; a test pins that, the way the
training and investigate guards do. It is listed in `zorp-agent/src/state.rs`
so it can be seen and cleared.

**Why proposed and not built here.** Storing anything a model wrote about the
person is the one move zorp has refused from the start, so turning it on is a
person's call before any code exists to make the suggestions. See
`docs/superpowers/specs/2026-10-27-dreaming-mode-design.md` for the shape this
would take if accepted.
