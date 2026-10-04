---
status: accepted
date: 2026-08-15
---

# evolve's search layer is not approved, its measurement discipline is

**Decision:** the `evolve` spec is marked NOT APPROVED and nothing is
built from it. Two rounds of adversarial review, eight reviewers, found
the search layer unsound both times, and the second round showed the
first rewrite had moved the flaw rather than removed it. `erbga` ships on
its own terms as a validated implementation of prior work, off any
critical path.

**Why:** three findings compose into one conclusion. There is no free
inner search, because variation is model-proposed, so the affordability
argument the whole design rests on is false. The framing score is
maximized by an undifferentiated blob, because CPM's objective is
extensive and edge addition was priced free. And two of its three score
terms are identically 1.0 by construction, which is the same defect as
the draft before it under new names. At `V = 20` an exact
clique-partitioning ILP solves the partition to proven optimality in
about 0.2 seconds, so the cut was never the hard part; the framing is,
and there is no cheap search over framings either.

**What survives, and should be built on ordinary `investigate` runs:**
never selecting on the pre-registered metric (breeding toward a metric
and then reporting it is biased upward twice, and pre-registration does
not cover selecting the observation that best clears a fixed test); the
confirmatory stage of `n` passes with the threshold on the mean and nulls
counted as non-passing; refusing to call framing diversity corroboration
when all lines share one model; quorum rather than unanimity for track
death.

**Bugs found in shipped code, worth fixing regardless:**
`TrackStatus::from_str` and `ExperimentStatus::from_str` both have
catch-all arms that silently coerce an unknown status to `Active` and
`Planned`. For a product that must not let a non-result look like a live
result, that is the worst possible default.

**Full writeup:** `docs/superpowers/specs/2026-08-14-zorp-evolve-design.md`,
whose "Where this stands" and "Review record" sections carry the detail.
