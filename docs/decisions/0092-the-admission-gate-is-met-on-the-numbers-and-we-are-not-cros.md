---
status: accepted
date: 2026-09-01
---

# the admission gate is met on the numbers and we are not crossing it

**Decision:** the hypothesis-search admission gate (2026-08-28: twelve
admitted anomalies spanning at least three condition keys) reads as met
against a real ledger, and we are not treating it as met. Hypothesis
search stays off real data. The gate is underspecified rather than
satisfied.

**The run.** 46 `investigate` attempts with `ZORP_FORECAST=1
ZORP_RERUN_GATE=1 ZORP_RERUN_REPEATS=2`, against a local `gemma4:e4b`,
on ten Fermi-count questions about a copy of this workspace. Three
passes at `--max-steps 12`, then 8, then a partial pass at 20 so one
condition key would take more than one value. 99 experiments, 26
admitted anomalies, 3 spanned keys. `gate_status` prints `GATE MET`.
Numbers and reasoning are on issue #144.

**Why we are not crossing it.** The count was never the scarce thing.
One afternoon against a small local model produced twice the required
anomalies and would produce twelve more tomorrow. What the gate was
meant to stand for is not shown by anything in that reading:

- The re-run gate has never rejected anything. 26 runs, 26 admitted,
  noise rate 0.000. `transient` and `volatile` exist in the synthetic
  tests and have never been produced by a real run. A filter that has
  passed everything it has ever seen has not been shown to filter.
- 11 of the 26 admissions are `unverifiable`: the repeat produced no
  number, so the surprise was never checked. Admitting those is right
  on its own terms, since an unmeasured repeat is not evidence of
  noise, but the gate counts them the same as a reproduction and a
  large minority of this ledger is "we could not tell".
- 26 of 36 scored forecasts, 72%, fell outside their interval, with
  widths from 4 to 4950. At 36 scored against `MIN_CALIBRATION_N` of
  50 the calibration report cannot return a verdict. An anomaly is
  defined as an outcome outside a stated interval, and that definition
  is empty while the intervals are not known to have coverage. The
  standing rule already says so: if the stated intervals do not have
  real coverage, stop and do not build the ledger.

**The spanning half is vacuous, and this run proved it.**
`record_conditions` writes exactly three keys on every attempt
(`model`, `checkpoint_mode`, `max_steps`), so any single admitted
anomaly spans all three. The requirement was satisfied on attempt one
and could not have failed. Varying `--max-steps` across passes moved
`spanned_atoms` from 3 to 5 and moved the verdict not at all. This is
why `gate_status` prints the atoms beside the keys.

**What this rules out.** Reading the gate as a go signal, and any
change that makes the count easier to reach. Two further conditions
look missing and are both computable from what the ledger already
holds: a nonzero noise rate, so the re-run gate has been shown to
reject something before its admissions are trusted, and a calibration
verdict that is not `NotEnoughEvidence`. Neither is written here as
decided, because that is a design decision and this entry is a
reading. See issue #144.

**Also recorded:** 8 of 46 attempts died with `no result object in the
agent's answer, fenced or bare`, and this is not the defect the
2026-09-01 parser widening fixed. Those attempts stopped after a step
or two and answered in prose. A small model quitting and a model
skipping its backticks produce the same error text, and only the
second one is a parser problem.
