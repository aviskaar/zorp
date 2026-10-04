---
status: accepted
date: 2026-08-19
---

# a turn is seeded from the store, and compaction never writes to it

**Amended by 2026-09-09**, which adds a model-written summary as a second
stage after the elision below. The reasoning here is not withdrawn: read
that entry for how the summary is kept out of the record it would otherwise
become part of.

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
