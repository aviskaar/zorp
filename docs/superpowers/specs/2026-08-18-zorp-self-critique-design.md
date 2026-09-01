# critique: audit a draft against the evidence record before it is delivered

**Date:** 2026-08-18
**Status:** built

## Purpose

`co-write` drafts an artifact from a track's recorded evidence. Nothing
between that draft and `deliver` checks whether the draft actually says
what the record supports. The model was told to cite only the figures it
was given, and being told is not the same as being checked.

`critique` is that check. It reads the track's evidence record, finds
claims in `draft.md` that are uncited or that rest on something the
record does not contain, revises them, and records what it found and what
it changed.

This is **not a fifth capability**. The per-capability specs in
`docs/superpowers/specs/` are the architecture record, and four is the
whole set there. That still holds. `critique` has no scope of its own,
gathers nothing, and produces no evidence. It reads the record and edits
the artifact `co-write` produced. It is a gate on that artifact, sitting
in the seam between `co-write` and `deliver`.

## The thing that makes it not worthless

A pass that asks a model "is this draft good?" is worthless, and building
one would have been worse than building nothing. The value is entirely in
comparing what the draft says to what the track actually holds.

So the pass is split in two, and the half that decides is code.

**The evidence ledger** (`critique/ledger.rs`) is built from the run
record by `EvidenceLedger::from_track`: every recorded metric, the
validation verdict and its citations, the pre-registered metric and kill
threshold, and the counts of experiments, metrics, and sources. It is
never handed to the model to extend. It is the complete set of things a
claim is allowed to rest on.

**The model's job is extraction, not judgement.** It is given the draft
and the ledger's evidence keys, and asked to inventory the draft's
factual claims: quote the sentence, name the one key it rests on, or
null. It is explicitly told not to judge style or rate quality.

**The audit** (`critique/audit.rs`) decides, in code:

- `uncited-claim`: the extracted claim rests on nothing in the ledger.
- `evidence-not-in-record`: it cites a key the ledger does not contain.
  This is a set-membership test, not a judgement call.
- `number-not-in-record`: a figure in the draft that the ledger cannot
  account for at any rounding or plausible scaling. This runs on the
  draft text alone, with no model involved at all.

That last one is the anchor. A critic that returns an empty claim list,
whether from laziness, evasion, or a bad day, cannot declare a draft
clean, because the numeric audit runs regardless. There is a test for
exactly that (`an_empty_claim_list_still_leaves_the_number_audit_in_force`).

Two more checks defend against the critic itself:

- An extracted claim whose text is not in the draft is discarded and
  counted. Revising a draft to fix a sentence that was never in it is
  strictly worse than doing nothing.
- The record is snapshotted before the pass runs and re-checked after
  every model turn. See "What it cannot do" below.

## Why the numeric audit is conservative

Every false positive here becomes a revision request against a draft that
was fine, which is the failure mode the brief called out: a pass that
always finds something will always change something. So the scanner skips
headings, ordered-list markers, dates, dotted versions, path and URL
fragments, ordinals, and numbers following a label word ("section 3",
"table 2"). Matching is generous in the other direction: a draft may
round a recorded value to the precision it wrote, may write a recorded
proportion as a percentage, and may quote a number that appears inside a
citation's text, because that was gathered too.

The proportion scaling is asymmetric on purpose. Only a value that could
be a proportion is tried as a percentage, and only a value that could be
a percentage is tried as a proportion, so a recorded `3` does not quietly
license a drafted `300`. Counts of things in the record (experiments,
metrics, sources) match exactly and are never scaled, because a track
with one experiment would otherwise license any drafted `100`.

## Why the loop terminates

Two independent bounds, and neither is "the model says it is done".

1. **A configured round bound.** `--critique-rounds`, else
   `ZORP_CRITIQUE_ROUNDS`, else `DEFAULT_MAX_REVISIONS` (2). The audit
   always runs once, so `0` means "tell me what is wrong and do not touch
   the draft", which is a real request and is honoured rather than read
   as unset.

2. **Strict improvement.** A revision is kept only if it leaves strictly
   fewer findings than the draft it replaced. The first revision that
   does not ends the pass, and the earlier draft stands. Findings are a
   non-negative integer that must strictly decrease, so the loop runs at
   most `min(max_revisions, findings at round 0)` times whatever the
   model returns.

The second bound is what makes "the draft is fine" a reachable outcome
rather than a thing the pass talks itself out of. A clean draft costs one
model call, requests no revision, and writes nothing but the notes.
Rewording that fixes nothing is discarded, not banked.

One degenerate case is closed explicitly: an empty answer would audit
perfectly clean, for the worst possible reason. An empty revision is
never an improvement.

## Why the critique is recorded

A revision that silently rewrites the draft destroys the thing zorp
exists to provide. Three places, in decreasing order of authority:

- **`critiques` rows in the run record** (`zorp-track/src/critique.rs`).
  One row per audited draft: round number, the SHA-256 of that draft, the
  findings as JSON, and whether that draft was the one carried forward.
  The draft text itself is not copied in, because `draft.md` on disk is
  already the source of truth for it and two copies is one too many. A
  clean pass writes a row with zero findings: "the pass ran and found
  nothing" and "the pass never ran" are different facts.
- **`draft.pre-critique.md`**, written beside `draft.md` whenever the
  pass changed anything. `diff` of the two is exactly what this pass
  changed. It holds the draft as the pass found it, so a second pass
  overwrites it with the first pass's output rather than the original.
  The `critiques` rows are where the whole history lives.
- **`critique.md`**, the readable rendering: every round, every finding
  with the sentence it is about, and whether each revision was kept or
  discarded and why.

Plus a `critique` checkpoint, whose prompt reports the finding counts and
the draft's line count before and after, so a pass that quietly deleted
most of the draft is visible at the point a human is asked about it. Like
`co-write` and `deliver`, rejecting the checkpoint does not kill the
track.

## What it cannot do

The pass reads the record and writes the artifact. It never calls
`write_prereg`, `record_metric`, `create_experiment`, or
`set_track_status`. That is a property of the code, and properties of
code drift, so it is also enforced at runtime.

`RecordSnapshot` captures the track status, the hypothesis, the
pre-registration row, the recorded metrics, the experiment count, and the
latest validation id before anything runs, and re-checks all of it after
every model turn, plus `verify_prereg_integrity` to catch a `prereg.md`
edited on disk while its row sat still. Any movement returns
`CritiqueError::RecordMutated` and the draft is left exactly as it was
found.

The CLI gives the critic no tools at all (`register_builtins_filtered`
with an empty allow-list, and no MCP attachment). The pass is a text task
over a draft and a ledger, both of which arrive in the prompt, so a tool
is not a capability it needs, only one it could reach the record with.
The runtime guard is what is actually load-bearing, and it is tested
against a genuinely write-capable agent whose model calls `write_file` on
`prereg.md` and succeeds. The threshold in the record does not move, and
the draft is not rewritten off the back of that run.

## Honest limits

- **Derived figures read as invented.** A draft that computes "58% faster"
  from a recorded 42 against a recorded 100 states a number the record
  does not contain, and the pass flags it. That is arguably correct (a
  derived figure should show its derivation) but it is a finding on a
  draft that was not lying, and if the revision explains the derivation
  without removing the number, the finding persists. The strict-
  improvement rule stops that from looping, and the residual finding ends
  up in `critique.md` as an unresolved note.
- **Deletion is the cheapest fix.** The finding count falls fastest when
  prose is removed. The revision prompt says so, the empty-revision guard
  closes the worst case, and the checkpoint reports the line delta, but
  nothing stops a model from over-deleting inside those bounds.
- **The claim inventory is only as good as the extractor.** A model that
  fails to notice an uncited assertion produces no finding for it. The
  numeric audit is not subject to this; the claim audit is.
- **"Cites a key in the ledger" is not "the evidence supports the claim".**
  The pass verifies that cited evidence exists. It does not verify that
  the evidence actually implies what the sentence says. That is a real
  gap and it is the obvious next thing.
- **No prose quality judgement at all, deliberately.** Weak hedging,
  overclaiming in words rather than numbers, and bad structure all pass.
  A pass that judged those would be the "is this good?" pass, which is
  the one worth not building.

## Alternatives considered

**Fold it into `co-write` or gate `deliver` on it.** "Before it is
delivered" invites making `deliver` refuse without a critique. That
changes an existing, tested capability's contract and is a product
decision, not an implementation one. A standalone subcommand also lets a
critique run against a hand-edited draft, which a `co-write` flag would
not. Left as a separate decision.

**Store critique findings as metrics.** They would then be part of the
evidence `co-write` reads, and the next draft could cite "3 findings" as
if it were gathered evidence. Findings are commentary on the artifact,
not evidence about the question, and the two must not mix.

**Let the critic score the draft 0 to 100.** A number a model made up
about its own output, with nothing to check it against. That is the vibes
pass wearing a metric's clothes.
