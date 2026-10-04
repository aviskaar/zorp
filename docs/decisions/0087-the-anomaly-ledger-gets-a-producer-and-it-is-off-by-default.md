---
status: accepted
date: 2026-09-01
---

# the anomaly ledger gets a producer, and it is off by default

The entry below found that nothing outside a test had ever called
`rerun_gate`, so the `anomalies` table had no producer and no number of
attempts would have put a row in it. `zorp-agent/src/investigate/gate.rs`
is that producer.

It runs after an attempt records its outcome. If the outcome fell inside
its own stated interval, or there was no forecast, or the outcome was not
a number, nothing happens and nothing is said. Otherwise the attempt is
repeated and the repeats go to `Store::rerun_gate`, which classifies
them. `Reproduced` and `Unverifiable` admit, `Transient` and `Volatile`
are counted as noise rather than discarded, so the noise rate stays
readable afterwards.

Five things were decided and are worth having written down.

**It is off by default**, behind `ZORP_RERUN_GATE`, with the count in
`ZORP_RERUN_REPEATS` defaulting to 2 and capped at 5. A gated attempt
costs one full agent run per repeat, so the default triples the price of
any attempt that surprises its forecast. This is the more expensive of
the two opt-ins and `ZORP_FORECAST` is off for the same reason. A ledger
nobody paid for is empty, and empty is the honest state for a record
nobody has fed. Two repeats and not one because `classify` measures
spread across the repeats alone: one repeat has zero spread and can never
fail the agreement check, so it can reach `Reproduced` but can never be
contradicted.

**It runs before the kill threshold is enforced.** A breach kills the
track and `investigate::run` refuses to start on a killed track, so
repeats after the kill could not run at all. It is also right on its own
terms: a breach is the largest deviation an attempt can produce, and a
ledger that excluded exactly those would hold only the anomalies that
were not bad enough to matter, which is a selection effect in the worst
available direction.

**A repeat starts where the original started.** `Agent::run` appends to
the transcript rather than resetting it, which was found the hard way, so
`gate_inner` truncates back to the seed before each repeat. Without that,
`Reproduced` would stop meaning "it happened again" and start meaning "it
was shown its own previous answer". `a_surprise_that_repeats_puts_a_row_in_the_anomaly_ledger`
asserts the transcript length is unchanged after a one-repeat gate, which
fails if the truncation is deleted.

**A repeat that produces no number is still handed to the gate**, which
makes the verdict `Unverifiable`, which is admitted and flagged. Failing
to look is not evidence that there was nothing to see, and dropping the
repeat would turn a broken replay into a clean `Reproduced`.

**No model takes part in the decision.** `classify` is arithmetic and
equality, `gate_candidate` is two reads and a comparison, and nothing
here asks a model whether something was an anomaly. The model's only job
is running the repeat exactly as it ran the original. Same split
`critique` uses, same one integrity rule 5 protects.

A failed gate never fails the attempt. The outcome is already recorded
and a replay that goes wrong must not throw away a result that was
earned.

Ruled out: gating on surprise alone. Most prediction error in a software
environment is a changed default or a mis-parsed file, and the ones that
reproduce forever are flaky tests. Also ruled out: asking a model whether
a deviation was interesting, for the reason above.
