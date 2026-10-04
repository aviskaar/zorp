---
status: accepted
date: 2026-08-20
---

# two anomalies with no recorded conditions are not alike

**Decision:** the anomaly-family similarity is the Jaccard overlap of two
deviations' condition sets, and an empty intersection over an empty union
scores 0.0 rather than 1.0.

**Why:** the degenerate case is not a corner. Early in a track almost nothing
records conditions, so scoring empty against empty as a perfect match would
bundle every unconditioned row in the ledger into one enormous family, at every
threshold in the sweep, and it would look like the strongest possible finding
because it would survive the widest possible band. The sweep cannot defend
against that: the family really is stable across all of θ. Empty over empty is
the absence of evidence about whether two things co-occur, not evidence that
they do.

**Second decision:** `boredom_candidates` and `family_candidates` are two calls,
not one. Boredom findings are reads of what never varied and need no gate.
Anomaly families sit behind the calibration gate. One call returning both would
let a caller cross the gate without noticing which half of the result did that,
and the gate is only worth having if crossing it is deliberate.

**Third, recorded because it bounds a claim about the handoff:** code enforces
that the model never chooses which findings to look at, never sees a
model-authored column on the way in, and never supplies the facts a candidate
carries. The brief is generated from the record and generation is
deterministic. Code does **not** enforce that the sentence the model writes
back contains no invented claim. Every candidate therefore travels with the
brief it came from, so a question can always be checked against what produced
it. Stated in the module docs rather than left as an implication, because "the
model may not add invariants of its own" reads like an enforced property and
only most of it is one.
