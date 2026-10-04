---
status: accepted
date: 2026-09-02
---

# the admission gate is four numbers read from the ledger, and each one was missing from the last reading

**Decision:** the hypothesis-search admission gate is four numbers read
from the ledger. There must be at least 12 reproduced, non-superseded
admissions; at least 3 condition keys that each took at least two values
among their producing experiments; at least one transient or volatile
re-run result; and a calibration Go at a stated tolerance. All four must
hold. Crossing remains a person's decision, recorded here when it happens.

**Why:** 15 reproduced rows were present in the 2026-09-01 reading, but
11 more admitted rows were unverifiable. A repeat that produced no number
did not check the surprise, so it cannot support the count.

The three spanned keys all appeared on every attempt. Their presence was
vacuous. A key that took one value separates nothing, while differing
values are the condition atoms the search actually uses.

The re-run gate had 26 runs and rejected none. A filter that admitted
everything it saw had not shown that it filters.

The forecast intervals had no calibration verdict. There were 36 scored
forecasts, below 50, and 26 fell outside their interval. Surprise has no
meaning while the stated intervals lack demonstrated coverage.

**Read against the 2026-09-01 ledger,** the restated gate is not met,
on three conditions of four. 15 reproduced admissions hold against the
12. One condition key of three varied, since only `max_steps` took more
than one value. The re-run gate admitted all 26 candidates. Calibration
is a no-go twice over: 36 scored forecasts against the 50 required, and
a band at a stated confidence of 0.72 that covered 0.28. The one
condition that held is the count, which was never the scarce thing.

**What it ruled out:** loosening any condition; counting unverifiable
admissions; reading "not NotEnoughEvidence" as enough for calibration;
and building the `ExperimentRow` adapter.

**Not changed:** `GateOutcome::admits`, `NoiseReport::noise_rate`, and
`record_gate_verdict` keep their ledger semantics. The 2026-08-28
algorithm and synthetic validation stand. A real run still needs a
permutation null. `zorp-track/src/admission.rs` and
`zorp-track/examples/gate_status.rs` read the gate. See issue #144.
