---
status: accepted
date: 2026-08-20
---

# the calibration report scores the last forecast, not every draft

**Decision:** where several expectations exist for one experiment and metric,
the calibration report counts only the last one written before the outcome.
The metrics side already took the first recorded value; this is its mirror,
so one prediction is scored against one result.

**Why:** `expectations` deliberately allows a forecast to be rewritten while
no outcome exists, because revising a belief before observing anything is
legitimate. Scoring every version undoes that. Nine absurdly wide drafts and
one real forecast would read as nine tenths covered, which is precisely the
"buy coverage with wide intervals" failure the report exists to expose. The
mean interval width would have made the padding visible, but visible is not
the same as excluded, and the coverage figure is the number people will
quote.

Found during integration rather than by either author: the module that allows
revision and the module that scores it were written in parallel and neither
could see the other. Caught by a test written to fail first, which counted
2 where it should have counted 1.

**Second decision, recorded because it bounds a claim:** above the |V|
crossover the search backend answers a slightly different question from the
exact one. Confounding is transitive, so the exact answer is a connected
component; modularity rewards assortativity, and a long thin chain scores
better cut in half. Because `erbga` only removes edges, its partition is
always a refinement of the components: it can split a true bundle, never
invent one. So a bundle reported above the crossover is a floor on the
confounding, not the whole of it. This is documented on the function. The fix,
if it ever matters, is a component-preserving objective, not dropping erbga.
