---
status: superseded
date: 2026-08-14
---

# a fifth capability, evolve, searches question framings and never selects on the metric

**Superseded by** the 2026-08-15 entry above, one day later. Nothing
below was built. The search layer described here is not approved and the
spec it points at is marked NOT APPROVED. `erbga` did ship, on its own
terms and wired to nothing. The rest of this entry is left as written,
because the findings against it need something to point at.

**Decision:** zorp gains `evolve`. It searches for a good way to
**decompose** a question, not for an answer. A population of framings
(sub-questions, a weighted "bears on" relation, marked cross-cutting
premises) evolves against a deterministic structural score, and the
partition of any given framing is solved directly rather than evolved.
The pre-registered metric appears nowhere in selection; it is measured
once per island after the search, over n independent synthesis passes,
with the threshold applied to the mean. Output is a distribution and its
dissent, not a number. Ships as an `erbga` crate (the algorithm of Rao,
Janikow, Bhatia, Climer, MWAIS 2018, zorp's author's prior work, used as
the partition solver for large framings and validated against that
paper's benchmarks) plus `zorp-agent/src/evolve/`.

**Why:** breeding a population to maximize a metric and then reporting
that metric is biased upward twice over. Framings that surface
inconvenient evidence score worse and stop breeding, and the maximum of
N noisy evaluations exceeds the truth by about sigma*sqrt(2 ln N).
Pre-registration does not cover this: it stops you moving the test after
seeing data, not selecting the observation that best clears a fixed
test. Separately, the uncertainty in this problem lives in the graph,
which a model invents and which was never revisited, not in the cut,
which is a small solvable problem. So the compute moved to the framings.

**What it rules out:** modularity as the objective, since its resolution
limit does not bind on the source's benchmarks but binds on every
realistic question graph, so CPM with a pre-registered gamma is used
instead. Corroboration as a claim, since all islands share one model;
the property is renamed framing diversity, cross-island evidence reuse
is forbidden for anything feeding a reported result, and component
source overlap is measured rather than assumed. Track death by
unanimity, replaced with a pre-registered quorum. A recorded-only
parameter tier, since when the result depends on a search, search effort
is answer selection, so every input is pre-registered. Also gone: Gene
Repair at this layer (the source's own results show accuracy degrading
monotonically with density with it enabled), and the claim that
evidence cost is bounded by vertex count.

**Full writeup:** `docs/superpowers/specs/2026-08-14-zorp-evolve-design.md`,
whose closing section records what four adversarial reviews changed.
