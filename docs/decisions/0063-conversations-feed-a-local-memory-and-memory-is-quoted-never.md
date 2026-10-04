---
status: accepted
date: 2026-08-22
---

# conversations feed a local memory, and memory is quoted, never summarized

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
