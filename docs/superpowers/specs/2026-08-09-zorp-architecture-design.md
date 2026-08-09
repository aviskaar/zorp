# zorp architecture

**Date:** 2026-08-09
**Status:** approved, capability names and scope amended same-day

**Amended by** `2026-08-09-zorp-scope-and-positioning.md`: the four
capability names below (validate, experiment, co-write, find a venue)
are superseded by validate, investigate, co-write, deliver, and their
scope broadens from academic research to any evidence-based question.
The architecture itself (one binary, `zorp-track` as the shared
foundation, the checkpoint pattern, the data-store split) is unchanged
and this document is still the source of truth for it. Read the
amendment for current names and scope; this document for structure.

## Purpose

This is the durable record of zorp's product architecture: what it does,
why it's shaped this way, and how the pieces relate. It supersedes the
external artifact used to iterate on this design; that artifact was
always a working sketch, this spec is the source of truth. The
foundation layer this architecture depends on is specced separately in
`docs/superpowers/specs/2026-08-09-zorp-track-foundation-design.md`. Each
of the four capabilities below gets its own implementation spec in turn.

## What zorp is

A general-purpose research harness with four standalone capabilities.
Each one works alone; a "full loop" is the four of them chained together
with a human checkpoint between each step, not a separate mode.

- **Validate**: given an idea, search the literature and come back with a
  novelty and feasibility read. Someone can stop here.
- **Experiment**: given a validated (or user-supplied) idea, plan a
  sequence of attempts and run them sandboxed. Every attempt is recorded,
  not just the best one.
- **Co-write**: zorp drafts paper sections from the experiment record; a
  human edits and is the author of record. zorp never presents a paper as
  finished on its own, because AI-authored papers are rejected outright
  at most venues.
- **Find a venue**: match a finished paper's abstract and contribution
  against a live catalog of conferences and journals, ranked, with
  deadlines.

## Why this shape

Most "AI scientist" systems (Sakana's AI-Scientist-v2, and to a lesser
extent Aviskaar's own lab-engine/Catalyst) assume the deliverable is a
finished, autonomously produced paper. That assumption doesn't survive
contact with how research actually gets published: a paper an AI wrote
end to end isn't submittable most places, so treating "write the paper"
as an autonomous step and bolting a fact-check onto the end is solving
the wrong problem. The paper step has to be collaborative from the
start, with the human as author.

The four-standalone-capabilities framing follows from how the harness
will actually get used: someone validating a single idea, or just
running one experiment, without wanting the full loop. Forcing everything
through one pipeline would serve the full-loop case at the expense of
the much more common partial one.

zorp does not take a hard dependency on Aviskaar-private infrastructure.
Open Research Review (ORR, `github.com/aviskaar/open-research-review`)
and lab-engine/Catalyst both do real, overlapping work, tracks and
experiment state in ORR's case, a working idea-to-paper pipeline in
Catalyst's, but ORR is a private dependency and Catalyst is built
specifically to bootstrap Aviskaar's own sub-projects. zorp has to work
for someone who has never heard of either, so it owns its own run record
rather than requiring one of them.

## Structure

One binary. `zorp-agent` gains subcommands for all four capabilities
rather than a separate `zorp-research` binary shelling out to it.
Parallel experiment workers are still isolated subprocesses, spawned as
more copies of `zorp-agent` itself, not a second program. One thing to
install, learn, and support.

Underneath the four capabilities sits `zorp-track` (see the foundation
spec): the multi-track data model, the run record, and the checkpoint
primitive. None of the four capabilities exist without it, and it
doesn't know about any of them; they're built on top of it, not into it.

```
zorp-agent (one binary)
  validate | experiment | co-write | find-a-venue   <- new subcommands
  -------------------------------------------------
  zorp-track                                         <- foundation, specced separately
    tracks, run record (DuckDB), library (LanceDB), checkpoints
  -------------------------------------------------
  existing zorp-agent: tools, sandbox, trust, verify, sessions, MCP
```

## The checkpoint pattern

Three checkpoints in a full loop: after validate, after experiment,
before co-write finalizes. Each is interactive by default, the same
default `zorp-agent`'s existing per-tool-call approval gate already
uses, with an explicit flag for unattended runs. This isn't a new
concept bolted on, it's the same pattern at a coarser granularity: a
pluggable decision point that asks a human unless told not to.

## Data, briefly

Two stores, split by job (full detail in the foundation spec and
`docs/DECISIONS.md`): DuckDB for the transactional and analytical run
record (typed, structured metrics, not narrative logs, so the co-write
claim check has something exact to compare against), LanceDB for
multimodal, semantically searchable content (literature, figures, plots).
Pre-registration, a hypothesis, a metric, and a numeric kill threshold,
committed as a human-readable file before any experiment code runs, is
always required, the same discipline Catalyst's idea triage already uses
and for the same reason: it stops the threshold from being quietly moved
after seeing results.

## Scope discipline

- No hard experiment budget. Catalyst caps experiments at 150 lines of
  code, 10 minutes, no GPU; zorp ships guidance, not enforcement, since a
  cap tuned for Catalyst's validation experiments could be wrong for
  zorp's actual users.
- Multiple concurrent research investigations (tracks) are supported from
  day one. Only one track is actively worked at a time per session; true
  concurrent execution across tracks is a later feature, not designed
  into this pass.
- Venue matching runs on an abstract and contribution summary, not the
  full paper, so it can run before the draft is finished, and queries a
  live venue API (the model used, huiban, is what was used to research
  zorp's own venues) rather than a catalog zorp ships and maintains.
- No Hypermemory integration yet. Memory stays local to each track's own
  stores. Real feature, later one.

## Out of scope for this spec

- The internal design of `zorp-track` (schema, file layout, integrity
  checking): `docs/superpowers/specs/2026-08-09-zorp-track-foundation-design.md`.
- The internal design of validate, experiment, co-write, and find a
  venue themselves: each gets its own spec once `zorp-track` exists.
- zorp's own arXiv paper (a systems paper about zorp, not a discovery it
  made): scoped in `docs/DECISIONS.md`, tracked in `docs/paper/`.

## Provenance

This spec consolidates decisions made across several passes this
session: an initial proposal, a revision after discovering ORR already
covers tracking, a further revision after discovering lab-engine/Catalyst
already covers idea-to-paper generation, a reframing after clarifying the
actual product goal (four standalone capabilities, human-authored
papers), and an eight-decision interview resolving what was still open.
Full history in `docs/DECISIONS.md`, dated 2026-08-09.
