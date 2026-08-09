# Architecture

## Preliminary proposal, not final

**[zorp Architecture Proposal](https://claude.ai/code/artifact/5153c897-f2c9-4184-9992-b74028d03e37)**,
last updated 2026-08-09.

This is a preliminary proposal, not a decision. It lays out zorp's
product as four standalone capabilities, validate, experiment, co-write,
find a venue, chained by human checkpoints when used as a full loop, each
built on the existing `zorp-agent` harness rather than a new orchestrator.
It has not been through a real design pass (brainstorming, then a spec in
`docs/superpowers/specs/`, then a plan). Treat it as a working sketch to
react to, not a spec to implement against.

Most of the open questions the proposal originally left unresolved have
since been settled by a short interview; see
[`DECISIONS.md`](DECISIONS.md) for the full list, including:

- One binary (`zorp-agent` gains subcommands), not a separate research
  binary; parallel workers are still isolated subprocesses of that same
  binary.
- Pre-registration (hypothesis, metric, kill threshold, committed before
  any experiment code runs) is always required, not optional.
- No hard experiment budget, soft guidance only.
- Research checkpoints are interactive by default, same pattern as the
  existing tool-call approval gate.
- Run record metrics are typed key-value pairs in DuckDB, not narrative
  logs, so the co-write claim check has something structured to compare
  against.
- Venue matching calls a live venue API (the huiban database used to
  research zorp's own venues), not a catalog zorp ships and maintains.
- zorp supports multiple concurrent research investigations from day
  one, not one-at-a-time with multi-track added later.
- Venue matching runs on an abstract and contribution summary, not the
  full paper.

Also locked in: the data-store split (DuckDB for the transactional and
analytical run record, LanceDB for multimodal semantic search), memory
staying local for now (no Hypermemory dependency yet), and the scope of
zorp's own arXiv paper (a systems paper about zorp itself, see
[`paper/`](paper/)).

What's still genuinely open: the exact multi-track data model, and
whether `zorp-agent`'s subcommands need any restructuring to host four
new capability areas cleanly. Those are implementation-plan questions,
not architecture questions, and belong in a spec once this proposal is
formally brainstormed.
