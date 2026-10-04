---
status: accepted
date: 2026-08-22
---

# a band too thin to judge is its own no-go, and never a miss

**Decision:** `CalibrationReport::verdict` now judges a band only when the
band carries enough forecasts for its coverage to mean something. A band
under `required_band_n(confidence)` comes back as
`NoGoReason::BandTooThin { confidence, n, required, observed_coverage }`
and never as `BandOutOfTolerance`, whatever its gap. The bar is
`max(MIN_BAND_N, ceil(1 / (1 - confidence)))`, with `MIN_BAND_N = 20`.
`MIN_CALIBRATION_N` is untouched and still applies to the report as a
whole. Both reasons block the go. Thin is not a pass.

**What was wrong.** `CalibrationBand` has carried `n` since it was
written and `verdict` never read it, so every band was judged against
the tolerance no matter how few rows it held. A real 60 directory run
against `stealth/ox-alpha` produced six bands of between three and
eleven forecasts, and the three row band at a stated 0.96 with one
outcome covered came back as `BandOutOfTolerance` with a gap of 0.627 at
every tolerance up to 0.20. Three rows can only read 0, 1/3, 2/3 or 1,
so the nearest coverage that band can reach to 0.96 is a perfect score,
and a forecaster who is exactly right fails it 11.5% of the time on the
arithmetic alone. The report was manufacturing findings its own data
could not carry, in a shape indistinguishable from a real miss. The
design already refuses to answer on thin evidence, which is what
`MIN_CALIBRATION_N` is; the same reasoning had simply never been applied
one level down.

**Why two floors and not one.** `1 / (1 - confidence)` is the size at
which a perfectly calibrated forecaster expects one miss in the band.
Under it, the band cannot express any coverage between what it states
and a perfect score, which is exactly the arithmetic above. It scales
with the claim, and it has to: 0.96 needs 25 rows, 0.98 needs 50, and
0.99 needs 100, while a flat number would wave all three through. It is
useless at the low end, though, where it asks for two rows at a stated
0.50, so `MIN_BAND_N` holds that end. Twenty is where one row moves the
observed coverage by five points, the tightest tolerance anything here
asks for. It is deliberately under `MIN_CALIBRATION_N`: at fifty, no
report short of the overall minimum could ever hold a band that met the
per-band one, the overall check would become unreachable except for an
empty report, and the test guarding it would stop guarding anything.

**Rejected: a Wilson interval as the test.** Failing a band only when
the interval on its observed coverage excludes the stated confidence is
the statistically standard move, and it makes the go easier, which
settles it. The existing boundary test has 42 of 60 covered at a stated
0.75, whose Wilson interval is [0.575, 0.801] and contains 0.75, so a
band the caller's own tolerance rejects would pass. A correctness fix
for over-reporting must not quietly start under-reporting.

**Rejected: the false alarm rate of the tolerance test itself.** The
honest question is how often a perfect forecaster fails this band at
this tolerance, computed from the binomial. It was written out and
measured before being dropped. It ties thinness to the tolerance, so
tightening the tolerance turns real misses into "too thin", which is
backwards. It is brutal on the existing suite: the 0.75 band above has a
29.6% false alarm rate at a tolerance of 0.05, the central Go fixture at
n = 50 sits at 4.93% against a 5% bar, and the two decisive misses in
`each_band_out_of_tolerance_gets_its_own_reason` (20 of 45 at a stated
0.80, 10 of 45 at 0.95) would be suppressed even though both are
decisive by any test. A rule about the null hypothesis suppresses
findings about the data.

**What changed in what the report says.** Newly raised: any band under
the bar, including ones that used to pass silently, so a report made
entirely of thin bands can no longer return Go. That was the hole. Newly
suppressed: `BandOutOfTolerance` on a thin band, including where the
miss is genuinely decisive. The observed coverage rides along in the new
reason so nothing is hidden, and the caller keeps the whole band list.
What the report refuses to do is call a three row gap a finding.

**A floor, not a proof of power.** Twenty rows at a stated 0.80 still
fails a tolerance of 0.10 about 16% of the time when the forecaster is
exactly right. Getting that under 5% takes fifty rows in that band
alone. This fix removes the bands where the coverage grid is coarser
than the verdict, and it does not certify that the verdict has power
above the bar. A caller who wants that can compute it: `n` and `covered`
are on every band.

**The go got harder, and that is checkable.** A band is silent now only
if it was silent before and clears the bar, so the reason list can only
grow. Every existing test still passes unchanged, including the mutation
test that says deleting the `n < MIN_CALIBRATION_N` check returns Go,
which still catches its own deletion because a band of 49 at a stated
0.80 is judgeable. Deleting the new per-band check, dropping the
`1 / (1 - confidence)` half of the bar, or widening the comparison to
`<=` each fail a test that exists for it.
