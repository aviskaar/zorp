---
status: accepted
date: 2026-08-24
---

# the calibration tolerance is 0.10, set from the first observed curve

**Decision:** the operating tolerance for `CalibrationReport::verdict` is
**0.10**. `Tolerance` stays a newtype with no default and no constant, so
this fixes what callers in this repo should pass and not what the type
permits.

**Why 0.10, and why only now.** `Tolerance`'s doc comment says the design
refuses to fix the number, "since it should be set from the first observed
curve rather than guessed here". Until this week there was no observed
curve. There is one now. Run 7 scored 151 forecasts from
`stealth/ox-alpha` over a crates.io corpus, above `MIN_CALIBRATION_N`, on
two bands both thick enough to judge:

    0.85..0.95   n= 39   stated=0.936   cov=0.949   gap=0.013   judged
    0.96..0.99   n=112   stated=0.976   cov=0.884   gap=0.092   judged

0.10 admits that curve with 0.008 to spare and refuses a band drifting
further. It is also what `MIN_BAND_N` was sized against: twenty rows move a
band by five points, so 0.05 is the tightest number anything here can
resolve at all, and asking for it turns a floor into a coin toss.

**What this does not license.** The number was chosen by a person looking
at a curve, which is what the design asked for, and it is not a licence to
choose a tolerance per report once its gap is known. A tolerance picked to
admit the result in front of it is the boundary problem `bin_boundaries`
exists to prevent, one level up. If a later curve says 0.10 is wrong, the
argument is made from that curve and recorded here, not applied silently
at a call site.

**What the curve says about the model, which is separate.** ox-alpha is
well calibrated below 0.96 and overconfident above it. Run 3 found the same
shape on a disjoint corpus at a different magnitude, so it is a property of
the model rather than of the sample. Whether the miss is concentrated above
a ceiling is written down as a prediction in
`docs/experiments/2026-08-24-calibration-ceiling-preregistration.md` and is
not a finding: the ceiling there was first noticed with the outcomes in
view, so it has to be confirmed on data it did not shape.
