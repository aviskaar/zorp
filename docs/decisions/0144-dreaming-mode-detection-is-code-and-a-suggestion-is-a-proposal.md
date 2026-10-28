---
status: accepted
date: 2026-10-27
---

# Dreaming mode: detection is code, and a suggestion is a proposal, never a memory

**Status is `accepted`.** The design below was written spec-and-ADR-first
(#296) and approved before any code; it is now built. The one departure from
the original sketch is that the default phraser writes the suggestion from the
pattern's own code-derived summary with no model call (`TemplatePhraser`), so
Dreaming mode works offline and on a build with no model configured. A
model-backed phraser is a drop-in behind the same `Phraser` trait and frames
the evidence with the fenced, nonce-marked block described below; the trait is
the seam, and nothing about the rules changes with it.

**Decision:** add a Dreaming mode. While zorp is idle, a background
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

**How it is built.** The detectors and the pass live in
`zorp-agent/src/dream/`; the `suggestions` table and its methods are in
`zorp-agent/src/session.rs`. The CLI is `zorp-agent dreams` (list, `run`,
`accept <id>`, `dismiss <id>`). `zorp-web` exposes `GET /api/dreams`,
`POST /api/dreams/run`, and `/api/dreams/:id/{accept,dismiss}` in every build,
with the "While you were away" card in the sidebar (`web/src/dreams.ts`); the
automatic pass is a worker beside the recall sweep, off unless `ZORP_DREAM` is
set, with `ZORP_DREAM_SWEEP_SECS` for its interval. `no_tool_starts_a_dream_pass`
in `zorp-agent/src/agent.rs` pins that no model tool reaches any of it, and
`zorp-agent/tests/dream_isolation.rs` pins that the wording never lands in
`messages`, `task`, or the recall/memory code. See
`docs/superpowers/specs/2026-10-27-dreaming-mode-design.md` for the design.
