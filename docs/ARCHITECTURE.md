# Architecture

## Approved

**[zorp architecture](superpowers/specs/2026-08-09-zorp-architecture-design.md)**
is the current, approved design: four standalone capabilities, one
binary (`zorp-agent`), a shared foundation (`zorp-track`, specced
separately and built) underneath all four.

**[zorp scope and positioning](superpowers/specs/2026-08-09-zorp-scope-and-positioning.md)**
amends the capability names and what zorp is actually for: validate,
investigate, co-write, deliver, targeting any evidence-based question,
not academic research specifically. Read this one for current names and
scope; the architecture spec above for structure, which this doesn't
change.

This was iterated through an external artifact during design, but that
was always a working sketch, not the durable record; the specs above are
the source of truth now. Decisions behind them are in
[`DECISIONS.md`](DECISIONS.md), including the ones earlier rounds of
this design got wrong before ORR and lab-engine/Catalyst were factored
in, and before the scope broadened past academia.

## What's built

`zorp-track`: the multi-track data model, DuckDB run record, git-backed
pre-registration with tamper evidence, index rebuild, typed
experiments/metrics, the checkpoint primitive, and LanceDB provisioning.
Wired into `zorp-agent` behind an optional `research` feature. See
[`superpowers/specs/2026-08-09-zorp-track-foundation-design.md`](superpowers/specs/2026-08-09-zorp-track-foundation-design.md).

## What's built (continued)

**validate** (is this question worth investigating): search via
whatever MCP tools are configured, score redundancy and feasibility with
required citations, checkpoint before moving on. See
[`superpowers/specs/2026-08-09-zorp-validate-design.md`](superpowers/specs/2026-08-09-zorp-validate-design.md).

**investigate** (gather evidence through staged, pre-registered
attempts): CLI-supplied metric name and kill threshold, one attempt per
invocation, typed metrics stored in the run record, human checkpoint
decides kill or keep. See
[`superpowers/specs/2026-08-09-zorp-investigate-design.md`](superpowers/specs/2026-08-09-zorp-investigate-design.md).

**co-write** (draft the artifact from recorded evidence): hands the
agent the track's recorded evidence (validate's verdict if present,
every metric investigate recorded) as structured data and instructs it
to cite only those figures; writes directly to `draft.md`, a human
remains author of record. Rejecting the checkpoint does not kill the
track. See
[`superpowers/specs/2026-08-09-zorp-co-write-design.md`](superpowers/specs/2026-08-09-zorp-co-write-design.md).

**deliver** (get the finished artifact into the right form): scoped to
academic venue-matching for v1, requires `draft.md` (from co-write) and
a huiban-prefixed MCP tool to be configured, checked the same way
validate requires a search-capable tool. Uses huiban to find and rank
real conferences and journals fitting the draft's scope, writes the
shortlist to `venues.md`, and checkpoints it. Rejecting the checkpoint
does not kill the track, matching co-write's behavior. See
[`superpowers/specs/2026-08-09-zorp-deliver-design.md`](superpowers/specs/2026-08-09-zorp-deliver-design.md).

## What's open

All four capabilities, validate, investigate, co-write, and deliver, are
built and tested. Nothing is left open in the core architecture.

Still worth knowing about, even though it's not part of the core
architecture: memory staying local for now (no Hypermemory dependency
yet), and the scope of zorp's own arXiv paper (a systems paper about
zorp itself, unaffected by the scope broadening, see
[`paper/`](paper/)), which is tracked separately from the four
capabilities and remains not yet written.
