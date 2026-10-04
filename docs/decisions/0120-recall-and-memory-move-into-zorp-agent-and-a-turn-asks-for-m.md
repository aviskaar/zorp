---
status: accepted
date: 2026-09-10
---

# recall and memory move into zorp-agent, and a turn asks for memory one message at a time

**This extends 2026-08-22 ("conversation text goes to a loopback address or
it goes nowhere").** Nothing in that entry is withdrawn, and no line of the
four layers that enforce it moved.

**Decision:** the chunker, the fingerprint, the indexer worker and the search
readers now live in `zorp-agent/src/recall.rs` and
`zorp-agent/src/memory.rs`, behind non-default `recall` and `memory` features
matching `zorp-web`'s. `zorp-web/src/recall.rs` and `memory.rs` are a few
lines of `pub use` each, so nothing in that crate had to move.

They did not go into `zorp-recall`. That crate depends on no other workspace
member, the way `zorp-search` and `zorp-skill` do, and the store the chunker
reads is in `zorp-agent`: pointing `zorp-recall` at it would invert the
dependency and pull the whole agent into an embedding crate. `zorp-agent` is
where the store is, so that is where the reader of the store goes.

**It is a lift and not a copy, and a test says so.**
`there_is_one_chunker_and_one_fingerprint` fails if a second `chunks_for` or
a second computing `fingerprint` appears anywhere in the workspace. Two
chunkers do not announce themselves: the index just quietly holds two
conventions from the day they disagree.

**A turn asks for memory per message, with a flag, and not with a mode.**
`--recall` on a one shot, the same shape as the browser's tick box. A REPL
toggle was the other option and is worse for the same reason a mode is
always worse here: the requirement is that nothing is recalled unless the
person asked for it on that message, and a toggle is a thing you can forget
you left on.

**An assistant line says it came from a model, on every surface.** One
function, `recall::attribution`, because this was written out twice and one
of the two copies had already stopped saying it.

**What it rules out:** a second chunker, a remote embedder, and a recall the
model can ask for. The model cannot turn any of this on, and there is still
no fallback when the local embedder is missing: the terminal says so and
searches nothing.
