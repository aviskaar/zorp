# zorp scope and positioning: research as investigation, not academia

**Date:** 2026-08-09
**Status:** approved

## Purpose

This amends `docs/superpowers/specs/2026-08-09-zorp-architecture-design.md`.
That spec's architecture (one binary, `zorp-track` as the shared
foundation, four capabilities chained by checkpoints, two data stores)
does not change. What changes is the scope of "research" the product
targets, the names of the four capabilities, and how zorp describes
itself. This spec is the current source of truth for those three things;
the architecture spec stays the source of truth for the underlying
structure.

## The redefinition

Research, for zorp's purposes, is not academic research. It is:

**Turning an uncertain question into a defensible answer using
evidence.**

The primitive is the same regardless of domain: question, investigation,
sources, evidence, conflicting evidence, reasoning, validation,
answer or artifact.

That covers a much wider range of questions than a paper pipeline
implies: technical decisions (should we migrate off Kafka), product
questions (what are users complaining about), competitive analysis,
investment theses, market sizing, scientific hypotheses, engineering
architecture choices, due diligence on a vendor or company, market entry
strategy, whether a research idea is novel, and ordinary high-stakes
personal decisions (moving abroad while working remotely). Academic
research is one instance of this primitive, not the whole of it.

## Why this doesn't require rebuilding anything

The `zorp-track` foundation, built and tested before this reframing, was
already general. Pre-registration, a hypothesis, a metric, and a kill
threshold, committed before evidence is gathered, is not an academic
construct. "We'll migrate off Kafka if a spike test shows a 20% latency
improvement" is the same shape as a scientific hypothesis, and the
existing tamper-evidence mechanism (a git-committed, human-readable
file, hash-verified on load) protects both identically. Tracks,
experiments, typed metrics, and checkpoints carry no domain assumption
in their schema or their code. Nothing in `zorp-track` needed to change
for this reframing; only the language describing what it's for did.

## The four capabilities, renamed

Same architecture, same relationship to each other (standalone, chained
by human checkpoints for a full loop), broader scope and better names:

- **validate** (unchanged name): is this question worth investigating?
  Has it already been answered, is there enough signal, what's the
  real scope of the uncertainty. A cheap filter, now understood to
  apply to any question, not just a scientific idea.

- **investigate** (renamed from experiment): gather evidence through
  staged, pre-registered attempts. The pre-registration discipline
  (commit the method and the threshold before results exist) is
  unchanged. What counts as an "attempt" broadens: a code benchmark, a
  literature synthesis pass, a set of interviews synthesized, a
  records or background check, a competitive teardown. All of it still
  runs sandboxed through `zorp-agent`, and every attempt still gets
  recorded, not just the winner.

- **co-write** (unchanged name): zorp drafts the artifact from the
  evidence record; a human is the author of record, for every artifact
  type, not only papers. The reasoning generalizes: a decision memo
  someone acts on, or a due-diligence package with legal or financial
  stakes, deserves the same accountability an academic paper does,
  arguably more. The claim-check pass generalizes too, and becomes more
  valuable outside academia: every claim in the draft must trace to
  recorded evidence, and evidence that conflicts with the draft's
  conclusion must be surfaced, not silently dropped. This is the
  "conflicting evidence, reasoning, validation" segment of the
  question-to-answer primitive.

- **deliver** (renamed from find a venue): once there's a finished
  artifact, determine the right format and audience for it. For an
  academic question, that's still matching a paper against a
  conference or journal catalog. For a technical decision, it's
  producing a stakeholder-ready memo instead of a paper draft. Same
  slot in the pipeline, format-aware instead of academia-only.

## Positioning

Not "AI research agent." That phrase reads as an academic-paper agent,
and undersells what the product actually targets. Working language:

**Zorp investigates hard questions and delivers evidence-backed
answers.**

Category can remain Research as a Service, with research defined
explicitly as investigation, the primitive above, not academic
publishing.

**Tagline:** LLMs made intelligence cheap. Zorp makes validated
intelligence cheap.

## What stays out of scope for now

This spec does not change any of the following, already decided
elsewhere and still correct:

- One binary (`zorp-agent` gains subcommands for all four capabilities).
- The checkpoint pattern (interactive by default, `AutoApprove` for
  unattended runs).
- The data-store split (DuckDB for the transactional and analytical
  run record, LanceDB for multimodal semantic search).
- No hard dependency on Aviskaar-private infrastructure (ORR,
  lab-engine/Catalyst).
- zorp's own arXiv paper is a systems paper about zorp itself, not a
  discovery made by using it. That paper's scope is unaffected by this
  reframing; if anything, "zorp is general-purpose evidence
  infrastructure, evaluated here on X" is a stronger systems-paper
  framing than a narrower academic-only claim would have been.

## Follow-up work implied, not designed here

- README's "Why zorp" section, its status/roadmap checklist (which
  currently names "validate / experiment / co-write / find a venue"),
  and its tagline need updating to match this spec. Small, mechanical,
  can happen immediately.
- `docs/ARCHITECTURE.md` should point at this spec alongside the
  architecture spec.
- The individual capability specs (still unwritten; each of the four
  gets its own spec once `zorp-track` exists, which it now does) should
  be written against this broader scope from the start, not written
  narrowly and broadened later.
