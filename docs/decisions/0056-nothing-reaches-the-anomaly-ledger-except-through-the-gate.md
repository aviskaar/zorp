---
status: accepted
date: 2026-08-20
---

# nothing reaches the anomaly ledger except through the gate

**Decision:** `anomalies` has exactly one writer, `record_gate_verdict`, and it
takes a whole `GateVerdict` rather than loose numbers. There is no argument
list a caller could assemble by hand, so the only way to produce a ledger row
is to have run the re-run gate.

The same call always writes a `gate_runs` row, admitted or not. Rejections are
counted rather than discarded, which is the only way the noisy TV rate becomes
measurable: a rejection that leaves no row cannot be counted afterwards, and a
system that cannot see its own noise floor cannot tell a quiet environment from
a blind one. Making it one call rather than two means no caller can record an
admission and forget the rejection.

`unverifiable` is admitted and flagged but is not counted as noise. A replay
that could not be performed says nothing about how noisy the measurement is,
and folding it into the rate would let a broken harness read as a clean one.
For the same reason `noise_rate()` returns `None` rather than `0.0` when the
gate has never run: zero looks like the best possible result, and "we have not
looked" is not a result at all.

**Second decision, recorded because the spec says otherwise:** the spec lists
four classification rules for the gate and the code has three branches. The
rule "repeats fall outside on opposite sides is volatile" was written as its
own check first, and the mutation that deletes it leaves every test green: a
repeat on the opposite side can satisfy neither the transient branch nor the
reproduced branch, so it already falls through to volatile. The branch was
removed rather than kept for symmetry with the spec. A branch that can never
change an answer implies a distinction that does not exist, and the next person
to read it would look for the case it handles and not find one. The spec's
table is still right about what the gate decides; it just states in four rules
what takes three to compute.
