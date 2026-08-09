# Architecture

## Preliminary proposal, not final

**[zorp Architecture Proposal](https://claude.ai/code/artifact/5153c897-f2c9-4184-9992-b74028d03e37)**,
last updated 2026-08-09.

This is a preliminary proposal, not a decision. It lays out zorp's
product as four standalone capabilities, validate, experiment, co-write,
find a venue, chained by human checkpoints when used as a full loop, each
built on the existing `zorp-agent` harness rather than a new orchestrator.
It has not been through a real design pass (brainstorming, then a spec in
`docs/superpowers/specs/`, then a plan), and open questions in it are
still open. Treat it as a working sketch to react to, not a spec to
implement against.

Decisions that are locked in, and don't wait on the rest of this
proposal being resolved, are logged in [`DECISIONS.md`](DECISIONS.md):
the data-store split (DuckDB for the transactional and analytical run
record, LanceDB for multimodal semantic search), and the scope of zorp's
own arXiv paper (a systems paper about zorp itself, see
[`paper/`](paper/)).
