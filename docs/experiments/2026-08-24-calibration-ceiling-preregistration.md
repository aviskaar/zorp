# Pre-registration: is the overconfidence above a ceiling?

**Registered:** 2026-08-24, before run 13 starts at 19:05 local.
**Status:** open. No data exists for this prediction yet.

## Why this file exists

Run 7 scored 151 forecasts from `stealth/ox-alpha` and the pooled verdict was
NO-GO at tolerance 0.05, GO at 0.10 and 0.20. Unpooling the high band
afterwards showed the miss was not spread across it:

```
@0.95  n=24  cov=1.000  gap=0.050  judged
@0.96  n=18  cov=0.944  gap=0.016  TOO THIN (needs 25)
@0.97  n=39  cov=0.846  gap=0.124  judged
@0.98  n=25  cov=0.920  gap=0.060  TOO THIN (needs 50)
@0.99  n=30  cov=0.867  gap=0.123  TOO THIN (needs 100)
```

and that restricting to forecasts stated at or below 0.96 gave n=57, stated
0.944, coverage 0.947, a gap of 0.004, which would be a GO at every tolerance
tried.

**That number is not evidence and this file is not a claim that it is.** The
0.96 ceiling was chosen after the outcomes were visible, by looking at where
the gap crossed. It is the same error `bin_boundaries` is written to avoid:
a boundary chosen with the hits in view is fitted to the answer. Reported as a
result it would be a postdiction wearing a calibration report's clothes, and
`expectations` has a mutation test precisely because that substitution is easy
to make and hard to see afterwards.

So the ceiling gets written down here, before the next run exists, and is
either confirmed on data it did not shape or it is not confirmed.

## What is fixed, now, before any data

The ceiling is **0.96**. It is not a free parameter to be tuned once results
arrive. If a later analysis reports a different ceiling, that analysis is
exploratory and says so, and it does not discharge this registration.

The restricted set is every scored forecast whose **stated** confidence is
at or below 0.96. Selection uses the stated confidence only, never the
outcome, never the truth, never the interval width.

## Predictions

**H1, model specific.** On a fresh run of `stealth/ox-alpha` over the same
crates.io corpus, the restricted set will show a coverage gap of at most
0.05, with at least 50 scored forecasts in it.

H1 is currently untestable and is registered anyway. ox-alpha's tool calling
is broken: handed a tools array it answers `finish_reason=stop` with zero tool
calls and zero content, which is why run 10 scored 0 of 31. H1 waits for that
to be fixed upstream. Registering it now is the point, because when ox-alpha
comes back the temptation will be to re-derive the ceiling from whatever the
new data shows.

**H2, general.** On any model that yields at least 50 scored forecasts on this
corpus, the restricted set will show a smaller coverage gap than the
unrestricted set.

H2 is the weaker claim and the testable one. It says overconfidence
concentrates at the top of the stated range as a property of this task, rather
than of one model. Run 13 (`cohere/north-mini-code:free`) tests it if it
yields enough.

## What would falsify each

- **H1 fails** if the restricted set's gap exceeds 0.05 on a fresh ox-alpha
  run with n at least 50. It is *not* rescued by moving the ceiling.
- **H2 fails** if the restricted gap is greater than or equal to the
  unrestricted gap on any model reaching n of 50. A single such model
  falsifies it; it is a claim about the task, so one counterexample is enough.
- Neither is confirmed by a run yielding fewer than 50 in the restricted set.
  Too thin to judge is not a pass, in either direction. A gap computed over
  a handful of rows is arithmetic about a handful of rows.

## What this cannot become

If run 13 yields little, this file is not revised to fit what it did yield.
The ceiling does not move, the tolerance does not move, and the minimum n does
not move. A registration that is edited once its outcome is visible has
recorded nothing.
