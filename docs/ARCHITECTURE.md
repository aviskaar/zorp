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

Four is still the whole set. A fifth capability, `evolve`, was designed,
reviewed, and not approved. See "What's open" below before assuming it
exists.

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

## What's built: the four capabilities

**validate** (is this question worth investigating): search via
whatever MCP tools are configured, score redundancy and feasibility with
required citations, checkpoint before moving on. It refuses to run
unless a connected MCP tool looks search-capable, decided by a search
verb in the tool's name (search, fetch, query, browse, find, lookup,
retrieve), so a server that cannot search does not satisfy the gate. See
[`superpowers/specs/2026-08-09-zorp-validate-design.md`](superpowers/specs/2026-08-09-zorp-validate-design.md).

**investigate** (gather evidence through staged, pre-registered
attempts): CLI-supplied metric name, kill threshold, and threshold
direction, required together on the first call for a track and refused
if they change afterward; one attempt per invocation; typed metrics
stored in the run record. An attempt that breaches the threshold kills
the track in code, without consulting the checkpoint, so `--yes` cannot
wave a breach through. Every other outcome goes to a human checkpoint
that decides kill or keep. See
[`superpowers/specs/2026-08-09-zorp-investigate-design.md`](superpowers/specs/2026-08-09-zorp-investigate-design.md).

**co-write** (draft the artifact from recorded evidence): hands the
agent the track's recorded evidence (validate's verdict if present,
every metric investigate recorded) as structured data and instructs it
to cite only those figures; writes directly to `draft.md`, a human
remains author of record. Requires at least one recorded metric, and
refuses on a killed track. Rejecting the checkpoint does not kill the
track. See
[`superpowers/specs/2026-08-09-zorp-co-write-design.md`](superpowers/specs/2026-08-09-zorp-co-write-design.md).

**critique** is a gate on co-write's artifact, not a fifth capability. It
has no scope of its own and produces no evidence: it audits `draft.md`
against the track's own evidence record, flags claims that are uncited or
that cite evidence the record does not contain plus figures the record
cannot account for, and revises within a configured round bound. The
audit is code; the model only inventories claims. It cannot move the Kill
Threshold or anything else pre-registered, and it is refused on a killed
track. See
[`superpowers/specs/2026-08-18-zorp-self-critique-design.md`](superpowers/specs/2026-08-18-zorp-self-critique-design.md).

**deliver** (get the finished artifact into the right form): scoped to
academic venue-matching for v1, refuses on a killed track, requires
`draft.md` (from co-write) and a huiban-prefixed MCP tool to be
configured, checked the same way validate requires a search-capable
tool. The missing-draft check runs first, so a user with neither gets
told to run co-write. Uses huiban to find and rank real conferences and
journals fitting the draft's scope, writes the shortlist to `venues.md`,
and checkpoints it. Rejecting the checkpoint does not kill the track,
matching co-write's behavior. See
[`superpowers/specs/2026-08-09-zorp-deliver-design.md`](superpowers/specs/2026-08-09-zorp-deliver-design.md).

## In the workspace, off the capability path

`erbga` is a sixth workspace member. It ships no binary, declares no
dependencies, and nothing in zorp depends on it. It is a standalone
implementation of published prior work (Rao, Janikow, Bhatia, Climer,
MWAIS 2018): a genetic algorithm for graph community detection,
validated against that work's four benchmarks. It came out of the
`evolve` design below, which was not approved, and it is kept on its own
terms as a validated artifact rather than wired into the research stack.
Don't wire it in without a decision that says to. See
[`../erbga/README.md`](../erbga/README.md) and [`DECISIONS.md`](DECISIONS.md),
2026-08-15.

## What's open

All four capabilities, validate, investigate, co-write, and deliver, are
built and tested. No part of the core architecture is waiting on a
design.

**A fifth capability was proposed and turned down.** `evolve` would have
run a population search over question framings. Two rounds of
adversarial review, eight reviewers, found its search layer unsound both
times, and the spec
[`superpowers/specs/2026-08-14-zorp-evolve-design.md`](superpowers/specs/2026-08-14-zorp-evolve-design.md)
is marked NOT APPROVED. Nothing was built from it: there is no
`zorp-agent/src/evolve/`. Its measurement discipline did survive review,
and the decision says to build that on ordinary `investigate` runs
rather than as a capability: never selecting on the pre-registered
metric, a confirmatory stage of `n` passes with the threshold on the
mean, refusing to call framing diversity corroboration, and track death
by quorum rather than unanimity. None of that is built yet. See
[`DECISIONS.md`](DECISIONS.md), 2026-08-14 and 2026-08-15.

Still worth knowing about, even though it's not part of the core
architecture:

- Memory stays local for now, with no Hypermemory dependency yet.
- zorp's own arXiv paper is written and builds. It is a systems paper
  about zorp itself, unaffected by the scope broadening, and it is
  tracked separately from the four capabilities. The source is
  `paper/zorp-paper.md` (433 lines), alongside a LaTeX template, a
  bibliography, figures generated from repository state, a Makefile, and
  a committed `zorp-paper.pdf`. What is open is the rest of it: it has
  not been posted to arXiv, it has no comparative evaluation (the paper
  says so, and states what one would require), most `references.bib`
  entries still need checking against their published records, and its
  test and line counts are pinned to commit `fd07e81`. See
  [`paper/README.md`](paper/README.md).
- The behavior of the shipped binary is checked by hand as well as by
  the test suite. Those runs are recorded in [`uat/`](uat/).
