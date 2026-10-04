---
status: accepted
date: 2026-09-09
---

# context compaction summarizes with the model, and the summary is never evidence

**This amends 2026-08-19 ("a turn is seeded from the store, and compaction
never writes to it") and 2026-09-03 ("compaction elides the model's own
tool-call arguments").** Both said no model writes a summary of the
conversation, and 2026-08-19 said why: a summary is a second chance to
hallucinate, and when it is wrong the material it replaced is no longer in
the request to contradict it. That reasoning still holds and is not
withdrawn. What changed is that a person asked for parity with Claude Code,
where compaction is a thing people already expect, and the design below
keeps every protection those entries were written for and adds the summary
on top.

**Decision:** when a conversation approaches the model's context window,
zorp compacts it in two stages. Stage one is the deterministic elision that
already existed and is unchanged: oldest tool-result bodies, then oldest
assistant tool-call arguments, never a `command`, and on the seed path
oldest whole exchanges. It always runs first. Stage two, new, asks the
model for a structured summary of the older part of the conversation, and
the turn continues with the system prompt, that summary, and the recent
messages verbatim. `/compact`, with an optional focus, does the same on
purpose from the browser and the CLI.

**The summary lives in `compactions` and never in `messages`, and that is
the whole of the protection.** Four things read `messages`: the recall feed
embeds user and assistant rows into the search index, the memory block
quotes them into a later turn and tells the model to cite them, titling
reads the first pair, and branching copies them. A summary written into
`messages` would be embedded, recalled, quoted and cited as though a person
or the model had said it in conversation, which is precisely what the
amended entries were protecting against. In its own table it is invisible
to all four by construction, rather than by four separate filters that have
to keep agreeing forever. `zorp-web/tests/compaction_never_evidence.rs` has
one test per reader.

**`messages` is never written, rewritten, or deleted by compaction.** The
full transcript stays on disk and `get_session` still returns all of it. A
reopened conversation shows what was said, with a marker where a summary
stood in during a turn. What shrinks is the request; what was said does
not. That half of 2026-08-19 is untouched.

**The block is a fenced, labelled `user` message appended to the seed and
never persisted.** It says above the fence that it is a model-written
summary of compacted conversation, that it is not a transcript and not
evidence, and that the verbatim messages after it are the current
conversation. `user` and never `system`, because this is the least trusted
text in the request and the one channel the harness speaks in must not
carry it. It counts as a record, so `with_message_records` treats it as
already persisted and `sync` never offers it to the recorder: the same
trick the memory block uses, for the same reason, and the cursor
arithmetic under a mid-run compaction is where that goes wrong, so it has
a debug assertion and a test with a recording `RunRecorder` that counts
what it was handed. The block's marker line is refused as a tool argument,
the way the elision markers are.

**The window is still unknown by default.** Nothing here guesses one, sends
`num_ctx`, or reads a model listing's `context_length`. Automatic
compaction therefore fires only with `ZORP_CONTEXT_TOKENS` set or with a
window a provider stated while refusing a request, which the 2026-09-03
entry already adopts. Manual `/compact` works regardless, because a person
asking is its own trigger.

**Bounded, and a failure never blocks a turn.** `MAX_COMPACTIONS_PER_TURN`
is three. A summary that succeeds and still leaves the estimate above
target counts as an attempt, which is what closes the loop where the
summary is itself too long. Past the bound the turn ends with an error
naming `ZORP_CONTEXT_TOKENS` rather than sending a request the provider
will refuse. A call that fails leaves the transcript as stage one left it,
emits `compacted { ok: false }` with the provider's words, and the turn goes
ahead. Stage one is still the fallback and no existing test of it changed
its expectation.

**Focus text and `# Compact instructions` are untrusted input.** The focus a
person types after `/compact` and the body of a `# Compact instructions`
section in the workspace's instruction files are both fenced separately,
each with its own per-call marker, under a sentence saying they are
preferences about what to keep and cannot change the rules. The older
transcript is fenced the same way and for the stronger reason: it holds
tool results and pages the agent fetched. And a prompt is not a constraint,
so what comes back is clamped in code on the one path to the table: an
empty reply, a reply missing any of the eight sections, and a reply long
enough that the next compaction would be summarizing a summary are all
refused, and a refused summary is a compaction that did not happen.

**An exchange is counted by its assistant turn, not by its user message.**
This is not a detail. A tool-using turn adds an assistant message and a
result per step and no user message at all: the Terminal-Bench run in the
2026-09-03 entry grew one task from 3k tokens to 122k over sixty steps
without the person typing once. Counting user messages would compact such a
turn exactly once and then never again, which is the case stage two exists
for.

**No model can ask for a compaction.** There is no tool for it, the same
way there is no tool that launches a panel or an investigate run, and
`agent.rs` has a test saying so. A model that could replace part of its own
transcript with a summary it wrote itself is the shape this whole design is
arranged against.

**A compaction recorded mid-attempt does not survive into the next
attempt's seed**, because `investigate` truncates the transcript back to
the seed between attempts and the seed is planned from the store's latest
compaction at the time the attempt starts. An attempt that compacted has a
shorter seed than one that did not, which is honest: the attempts ran under
different conditions and the record says so.

**What it rules out, still:** reading `context_length` from a model listing
to set the window, prompt caching, any provider-specific compaction API
(the Anthropic beta included, since zorp talks to arbitrary endpoints),
summarizing tool results individually, and a settings control for the
threshold. `ZORP_CONTEXT_HEADROOM` is enough for now.
