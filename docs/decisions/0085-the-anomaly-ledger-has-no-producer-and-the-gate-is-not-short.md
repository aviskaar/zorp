---
status: accepted
date: 2026-09-01
---

# the anomaly ledger has no producer, and the gate is not short of data

**Decision:** the hypothesis-search admission gate (2026-08-28) is read by
`cargo run -p zorp-track --example gate_status -- <project-root>`, an
example rather than a command, because aryabhatta ships no CLI on purpose
and reading the ledger back was already "Rust against the library". It
prints the reading and never acts on it: crossing the gate stays a
person's decision, and the example deliberately does not build the
`ExperimentRow` adapter, since a reader that quietly grew one would be
the crossing itself.

**What the first real corpus run says.** The 2026-08-31 entry added
`max_steps` so a real run could reach three condition keys, and a 20
attempt crates.io-style corpus then ran against local Ollama with
`ZORP_FORECAST=1`. Reading that ledger:

    experiments            22 (0 produced an admitted anomaly)
    admitted anomalies     0 (need 12)
    spanned condition keys 0 (need 3)
    forecasts recorded     19
    of those, scored       11
    outside the interval   0
    stated interval width  mean 11.5, min 4.0, median 10.0, max 23.0
    gate runs              0

The condition half of the 2026-08-31 fix works: `model` took 2 values,
`max_steps` took 3, `checkpoint_mode` 1. The gate is nevertheless not
short of data in the way the 2026-08-28 entry assumed.

**Two blockers, and only one was known.** The known one is volume: 11
scored forecasts against `MIN_CALIBRATION_N` of 50. The new one is that
**nothing outside a test has ever called the gate**. Every caller of
`Store::record_gate_verdict` and of `Store::rerun_gate` is a test module
in `zorp-track`. `investigate` records conditions, a forecast and an
outcome, and stops there. So the `anomalies` table has no producer at
all, and no number of further attempts would put a row in it. This
sharpens the CLAUDE.md line that "`investigate` is what writes to it":
true of `conditions` and `expectations`, not true of `anomalies`.

**And a third fact worth recording before it gets explained away.** Of
11 scored forecasts, zero fell outside their stated interval, and that
is not the artifact of vacuous intervals it would usually be: the median
stated width is 10 points on a 0 to 100 scale. Whether that is a well
calibrated small model or a corpus of questions too easy to separate is
not answerable at n=11 and is not guessed at here.

**Ruled out:** wiring `rerun_gate` into `investigate` as part of this
reading. The gate replays an experiment, so it costs model calls, it
interacts with checkpoint semantics, and it writes to a pre-registered
evidence record. That is its own decision and wants its own entry, not a
change smuggled in under a diagnostic.

Also ruled out: counting `superseded` anomalies toward the 12. They are
admitted rows later made moot, and letting them make up a shortfall
would let the ledger reach the gate by having its mind changed.

**Ruled out for the corpus run itself:** treating the 10 of 20 attempts
that failed as noise. Every failure was the `ollama-small` flavor and
every one was `could not score the attempt: no fenced JSON block found
in the agent's answer`. That is an output-contract problem with one
model, and running the corpus again without fixing it would spend half
the model calls to learn nothing.
