---
status: accepted
date: 2026-08-22
---

# a calibration band is a bin of adjacent confidences, sized by what it can judge

**Decision:** `CalibrationReport` no longer makes one band per distinct
stated confidence. Rows arrive ascending by stated confidence and are
gathered into bins until a bin holds `required_band_n` of its own mean,
at which point it closes. A band's `confidence` is the mean of the
confidences its forecasts stated, not a bin edge, because the comparison
has to be against what was actually claimed. `CalibrationBand` gained
`parts`, one entry per distinct stated confidence pooled into the bin,
so a reader can see everything that went into that mean. Two rules hold
the binner up. A whole group of identical stated confidences moves
together, so no boundary depends on the order rows were written in. And
a leftover tail too small to close joins the bin before it rather than
being dropped.

**What was wrong.** `required_band_n` arrived the same day and is right,
but nothing had asked what it does to a forecaster that writes free-form
numbers. A registry run against `stealth/ox-alpha` produced 35 scored
forecasts on six confidences between 0.93 and 0.99, the thickest holding
eleven. Every band was `BandTooThin`, so nothing at all could be
concluded, and the run would have needed roughly five hundred usable
attempts to fix that. The data was not the problem: pooled, those same
rows say a stated 0.968 against an observed 0.886, a forecaster
overconfident by about eight points, which is a finding. The report was
refusing to state a conclusion its own rows carried, purely because of
where it had drawn its lines.

**Why not fixed bins.** They were tried first. Bins of `[0, 0.7)`,
`[0.7, 0.85)`, `[0.85, 0.95)`, `[0.95, 1)` split that run into three and
thirty two, and the bin of three is `BandTooThin` at every tolerance
forever. One stray forecast at 0.93 would hold the verdict hostage for
the life of the record. A grid fixed in advance cannot know where the
rows will land, so it is a bet on the forecaster's habits.

**What this makes easier, said plainly.** A go that no tolerance could
reach is now reachable, so this is a loosening, and a loosening of a
measurement needs an argument. The argument is that what it removes is
silence rather than a finding: every band in the motivating run was
`BandTooThin`, which is the report saying it has not measured anything.
A group carrying the rows its own requirement asks for still closes its
own bin, so a demonstrated miss is not pooled away by a neighbour that
could have stood alone.

**What it can still hide.** Pooling averages, and the pooled gap is the
row-weighted mean of the gaps that went into it. Two groups miscalibrated
in the same direction cannot cancel, and uniform overconfidence, the
failure this whole report exists to catch, is exactly that case. What can
cancel is a bin holding a group that over-covers and a group that
under-covers, and there the smaller group can carry real weight: 19 rows
at a stated 0.50 that all landed inside, pooled with 20 at a stated 0.90
covering 0.60, read as a gap of 0.09 where the second group alone would
have been a miss. Three things bound it. Bins are the narrowest the
requirement allows, so only a group that arithmetically cannot be judged
alone is ever absorbed. `parts` puts every pooled group on the page with
its own `n` and `covered`, so a pass is auditable rather than asserted.
And no scheme that never drops a forecast can avoid pooling the groups
that cannot be judged; the alternative is to drop them, which biases the
curve silently and is worse.

**What is not negotiable.** The boundaries are computed by
`bin_boundaries`, which is handed the stated confidences and nothing
else. A boundary chosen with the outcomes in view would be fitted to the
answer, and the coverage it then reported would be a property of the fit.
A test flips every outcome and asserts the bins do not move; the
signature is what makes that true and the test is what keeps it true.
Every scored row lands in exactly one bin, with a test that the bins' `n`
sums to the report's `n`. And the binner closes a bin on exactly the
predicate `verdict` judges it by, through the same `required_band_n` and
the same mean, because a bin the binner called finished coming back as
`BandTooThin` would be the report contradicting itself.

**What did not change.** `verdict` is untouched. `MIN_CALIBRATION_N`,
`MIN_BAND_N`, `required_band_n`, the thinness check before the tolerance
check, the NaN handling before every comparison, and the empty report
answering `NotEnoughEvidence` all still hold, with their tests. A report
too small to fill one bin is one band that `verdict` calls
`BandTooThin`, exactly as before.

**Worth knowing.** Written out, the closing rule `n >= ceil(1 / (1 -
mean))` is just `sum(1 - c_i) >= 1`: a bin is thick enough when a correct
forecaster expects at least one miss in it. That form is monotone in
adding rows, which is why merging a tail into a closed bin can never
reopen it, and it is why the greedy left to right pass is well defined.
