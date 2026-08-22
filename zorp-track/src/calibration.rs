//! aryabhatta step 3: surprise arithmetic and the calibration report.
//!
//! The report is the decision point. Surprise is arithmetic on a sigma
//! the forecaster asserted, so it means nothing until the intervals
//! those sigmas came from are shown to have real coverage. If stated
//! 80% intervals hold the truth 45% of the time, every anomaly built on
//! top of them is fiction, and the cheapest moment to find that out is
//! before the ledger exists.

use crate::track::Store;
use crate::TrackError;

/// One stated confidence and what the outcomes inside it actually did.
///
/// A band exists only when at least one forecast was made at that
/// stated confidence and has an outcome, so `n` is never zero here.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationBand {
    /// The coverage the forecaster stated, for example 0.80.
    pub confidence: f64,
    /// Forecasts at this stated confidence that have an outcome.
    pub n: usize,
    /// How many of those outcomes fell inside their interval.
    pub covered: usize,
    /// `covered / n`. The other half of the calibration curve.
    pub observed_coverage: f64,
    /// Mean width of the intervals in this band. Coverage alone can be
    /// bought by predicting everything, so it is never reported alone.
    pub mean_interval_width: f64,
}

/// Empirical coverage against stated confidence, over every forecast in
/// the record that has an outcome.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalibrationReport {
    /// Forecasts with an outcome, across all bands.
    pub n: usize,
    /// How many of those outcomes fell inside their interval.
    pub covered: usize,
    /// Mean interval width across all bands, `None` when `n` is zero.
    pub mean_interval_width: Option<f64>,
    /// One entry per distinct stated confidence, in ascending order.
    pub bands: Vec<CalibrationBand>,
}

impl CalibrationReport {
    /// `covered / n`, or `None` when nothing has been forecast.
    ///
    /// Not a field, because it is derived from two that are, and not a
    /// bare `f64`, because 0/0 with no forecasts in the record would
    /// report as coverage of zero and read like total failure.
    pub fn observed_coverage(&self) -> Option<f64> {
        if self.n == 0 {
            return None;
        }
        Some(self.covered as f64 / self.n as f64)
    }

    /// The calibration curve: stated confidence against observed
    /// coverage, ascending by stated confidence.
    ///
    /// A well calibrated forecaster puts these points on the diagonal.
    /// The spec's failure case, 80% intervals holding the truth 45% of
    /// the time, is the point (0.80, 0.45) sitting well below it. Read
    /// the curve next to `mean_interval_width`, since the cheapest way
    /// to move a point up is to widen the interval until it says
    /// nothing.
    pub fn curve(&self) -> Vec<(f64, f64)> {
        self.bands
            .iter()
            .map(|b| (b.confidence, b.observed_coverage))
            .collect()
    }
}

/// A band still accumulating rows. Kept out of the public API because
/// a half-counted band is not a result.
struct OpenBand {
    confidence: f64,
    n: usize,
    covered: usize,
    width_sum: f64,
}

impl OpenBand {
    fn new(confidence: f64) -> Self {
        OpenBand {
            confidence,
            n: 0,
            covered: 0,
            width_sum: 0.0,
        }
    }

    /// Divide once, at the end. A band is only ever closed with at
    /// least one row in it, so neither division is 0/0.
    fn close(self) -> CalibrationBand {
        CalibrationBand {
            confidence: self.confidence,
            n: self.n,
            covered: self.covered,
            observed_coverage: self.covered as f64 / self.n as f64,
            mean_interval_width: self.width_sum / self.n as f64,
        }
    }
}

/// Every expectation that has an outcome, one row each.
///
/// The join is on experiment and metric key, which is what makes an
/// outcome the outcome of that forecast. Only the first numeric metric
/// recorded under a key counts: the integrity rule means every metric
/// matching an expectation was recorded after it, so the first one is
/// the result the forecast was actually about, and a rerun that
/// re-records the same key later cannot dilute the record with copies
/// of one forecast.
///
/// Only the last forecast counts, mirroring the first outcome.
/// `expectations.rs` lets a forecast be rewritten while no outcome
/// exists, because revising a belief before observing anything is
/// legitimate. Scoring every version would undo that: nine absurdly
/// wide drafts and one real forecast would read as nine tenths covered,
/// which is the "buy coverage with wide intervals" failure this report
/// exists to expose. Taking the last forecast against the first outcome
/// scores one prediction against one result.
///
/// The `ORDER BY` ends in the primary key, so the row order is total
/// and does not depend on how the planner happened to build the join.
/// The float sum in the caller reads rows in this order.
const CALIBRATION_SQL: &str = "\
SELECT e.confidence, e.interval_low, e.interval_high, o.value_number \
FROM ( \
    SELECT experiment_id, metric_key, confidence, interval_low, interval_high, seq, id FROM ( \
        SELECT experiment_id, metric_key, confidence, interval_low, interval_high, seq, id, \
               ROW_NUMBER() OVER (PARTITION BY experiment_id, metric_key ORDER BY seq DESC, id DESC) AS rn \
        FROM expectations \
    ) ranked WHERE rn = 1 \
) e \
JOIN ( \
    SELECT experiment_id, metric_key, value_number FROM ( \
        SELECT experiment_id, metric_key, value_number, \
               ROW_NUMBER() OVER (PARTITION BY experiment_id, metric_key ORDER BY seq) AS rn \
        FROM metrics \
        WHERE value_type = 'number' AND value_number IS NOT NULL \
    ) ranked WHERE rn = 1 \
) o ON o.experiment_id = e.experiment_id AND o.metric_key = e.metric_key \
ORDER BY e.confidence, e.experiment_id, e.metric_key, e.seq, e.id";

impl Store {
    /// The calibration report: a pure read over every expectation that
    /// has a recorded outcome.
    ///
    /// Writes nothing. That is integrity rule 4, and it is what lets
    /// this be run at any time, including in the middle of an
    /// investigation, without becoming part of the record it measures.
    ///
    /// An outcome is a numeric metric recorded for the same experiment
    /// under the same key. Only numeric metrics can be inside or
    /// outside an interval, so text and boolean metrics are not
    /// outcomes and an expectation waiting on one is simply not counted
    /// yet.
    ///
    /// Coverage is inclusive at both ends: an outcome landing exactly
    /// on a bound is covered. The forecaster wrote the bound, and
    /// nobody states an interval meaning to exclude its own edge.
    pub fn calibration_report(&self) -> Result<CalibrationReport, TrackError> {
        let mut stmt = self.conn.prepare(CALIBRATION_SQL)?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, f64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })?;

        let mut report = CalibrationReport::default();
        // Counts are integers and the coverage fractions are divided
        // once at the end, so no coverage figure depends on the order
        // rows arrive in. The widths are the one running float sum
        // here, which is why the query fixes a total order rather than
        // leaving it to the planner.
        let mut width_sum = 0.0_f64;
        let mut open: Option<OpenBand> = None;

        for row in rows {
            let (confidence, interval_low, interval_high, observed) = row?;
            let covered = observed >= interval_low && observed <= interval_high;
            let width = interval_high - interval_low;

            report.n += 1;
            report.covered += usize::from(covered);
            width_sum += width;

            // Bands are compared on the bit pattern rather than with
            // `==`, so a stored NaN confidence groups with itself
            // instead of opening a fresh band on every row.
            let same_band = open
                .as_ref()
                .is_some_and(|b| b.confidence.to_bits() == confidence.to_bits());
            if !same_band {
                if let Some(finished) = open.take() {
                    report.bands.push(finished.close());
                }
                open = Some(OpenBand::new(confidence));
            }
            if let Some(b) = open.as_mut() {
                b.n += 1;
                b.covered += usize::from(covered);
                b.width_sum += width;
            }
        }
        if let Some(finished) = open.take() {
            report.bands.push(finished.close());
        }
        if report.n > 0 {
            report.mean_interval_width = Some(width_sum / report.n as f64);
        }
        Ok(report)
    }
}

/// Why a surprise figure does not exist for an expectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undefined {
    /// The stated confidence is not strictly between 0 and 1.
    Confidence,
    /// The interval has zero width. The forecaster claimed to know the
    /// outcome exactly.
    ZeroWidthInterval,
    /// `interval_high` is below `interval_low`.
    InvertedInterval,
    /// One of the numbers is NaN or infinite.
    NotFinite,
}

/// The two-sided critical value for a central interval of stated
/// coverage `confidence`.
///
/// A central interval leaves `(1 - confidence) / 2` in each tail, so the
/// value wanted is the standard normal quantile at `(1 + confidence) / 2`.
/// At 0.80 that is the quantile at 0.90, which is the 1.2816 the spec
/// names.
///
/// `Err` for anything outside the open interval (0, 1), NaN included.
/// The quantile runs to infinity at both ends, so a confidence of 0 or 1
/// has no finite critical value, and the comparison below is written so
/// that NaN fails it rather than falling through.
pub fn z_for_confidence(confidence: f64) -> Result<f64, Undefined> {
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(Undefined::Confidence);
    }
    Ok(probit((1.0 + confidence) / 2.0))
}

/// The standard deviation a stated interval implies, per the spec:
///
/// ```text
/// sigma = (interval_high - interval_low) / (2 * z(confidence))
/// ```
///
/// This is the forecaster's own assertion read back as a scale. Nothing
/// here checks that the assertion is any good. That is the calibration
/// report's job, and until it has run, every sigma computed here is
/// advisory.
///
/// A zero-width interval is refused rather than divided by. The
/// arithmetic would give infinity for any miss and 0/0 for an exact
/// hit, so the two most different outcomes a forecast can have would
/// come back as `inf` and `NaN`. Both then propagate: a NaN loses every
/// comparison it is put through, so a forecast that claimed certainty
/// and was wrong would sort as the least surprising thing in the
/// ledger. Naming the case makes the caller decide what to do with a
/// forecaster who claims to know the answer exactly, which is a
/// judgement about the forecast rather than about the outcome.
///
/// The same reasoning covers an inverted interval and a NaN or infinite
/// bound. None of them is a scale, so none of them gets to look like
/// one.
pub fn sigma(interval_low: f64, interval_high: f64, confidence: f64) -> Result<f64, Undefined> {
    let z = z_for_confidence(confidence)?;
    if !interval_low.is_finite() || !interval_high.is_finite() {
        return Err(Undefined::NotFinite);
    }
    let width = interval_high - interval_low;
    // A width can still overflow to infinity between two finite bounds
    // at opposite ends of the range.
    if !width.is_finite() {
        return Err(Undefined::NotFinite);
    }
    if width < 0.0 {
        return Err(Undefined::InvertedInterval);
    }
    if width == 0.0 {
        return Err(Undefined::ZeroWidthInterval);
    }
    Ok(width / (2.0 * z))
}

/// How far an outcome landed from its forecast, in units of the sigma
/// that forecast implied, per the spec:
///
/// ```text
/// surprise = |observed - expected_value| / sigma
/// ```
///
/// Unsigned on purpose. Which side of the forecast an outcome fell on
/// is a separate question, and the re-run gate is the thing that asks
/// it, since `reproduced` means outside the interval on the same side.
///
/// `Err` carries whatever made the sigma undefined, plus `NotFinite` if
/// the outcome itself is NaN or infinite. A number is not returned for
/// an outcome that is not a number.
pub fn surprise(
    observed: f64,
    expected_value: f64,
    interval_low: f64,
    interval_high: f64,
    confidence: f64,
) -> Result<f64, Undefined> {
    let s = sigma(interval_low, interval_high, confidence)?;
    if !observed.is_finite() || !expected_value.is_finite() {
        return Err(Undefined::NotFinite);
    }
    Ok((observed - expected_value).abs() / s)
}

/// The inverse of the standard normal cumulative distribution, by
/// Peter Acklam's rational approximation (public domain).
///
/// A closed form does not exist, so this is an approximation and its
/// error bound is part of the API: relative error below 1.15e-9 over
/// the whole open interval (0, 1). That is four orders of magnitude
/// tighter than anything the calibration report can resolve, since the
/// report's inputs are forecaster-asserted intervals.
///
/// Two rational functions in three pieces. The central piece is fitted
/// in `p - 0.5`, and the two tail pieces in `sqrt(-2 ln p)`, because a
/// polynomial in `p` cannot follow the quantile as it runs off to
/// infinity at the ends. The published breakpoint is 0.02425. Using the
/// central piece past it is what makes z(0.99) come out as 2.5727
/// instead of 2.5758, which is an error large enough to matter.
fn probit(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.38357751867269e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const BREAK_LOW: f64 = 0.02425;
    const BREAK_HIGH: f64 = 1.0 - BREAK_LOW;

    // The tail branches are one expression evaluated on the smaller of
    // p and 1 - p, then negated for the upper tail. Writing it once
    // keeps the two tails exactly symmetric, which matters because a
    // central interval reads both of them.
    let tail = |t: f64| -> f64 {
        let q = (-2.0 * t.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };

    if p < BREAK_LOW {
        tail(p)
    } else if p > BREAK_HIGH {
        -tail(1.0 - p)
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    }
}

/// Forecasts with outcomes below which the question is not yet
/// answerable.
///
/// From the spec: "coverage within tolerance of the stated band, over n
/// of at least 50." The number is in the design, so it is a constant
/// here. The tolerance beside it is not, and `Tolerance` says why.
pub const MIN_CALIBRATION_N: usize = 50;

/// The smallest a band can be and still be judged at all, whatever it
/// states.
///
/// One row moves a band's observed coverage by `1/n`. At twenty rows
/// that is five points, which is the tightest tolerance anything in
/// this repo asks for. Below twenty a single covered-or-not moves the
/// band by more than the whole tolerance, so the number being compared
/// is the arithmetic of small integers and not a coverage rate.
///
/// Deliberately under `MIN_CALIBRATION_N`. The two minimums answer
/// different questions and each has to be able to fail on its own. Set
/// to fifty, no report short of the overall minimum could ever have a
/// band that met the per-band one, the overall check would stop being
/// reachable for anything but an empty report, and the test that
/// guards it would stop guarding anything.
///
/// This is a floor, not a proof of power. A band of twenty at a stated
/// 0.80 still fails a tolerance of 0.10 about 16% of the time when the
/// forecaster is exactly right. Getting that under 5% takes fifty rows
/// in that band alone, and a report is free to demand more than this
/// floor: `n` and `covered` are on every band.
pub const MIN_BAND_N: usize = 20;

/// How many scored forecasts a band at this stated confidence needs
/// before its coverage means anything.
///
/// Two floors, and the larger wins. `MIN_BAND_N` holds the low
/// confidence end. The other is `1 / (1 - confidence)`, the size at
/// which a perfectly calibrated forecaster expects one miss in the
/// band. Under it the band cannot express any coverage between what it
/// states and a perfect score: three forecasts at a stated 0.96 can
/// only come out 0, 1/3, 2/3 or 1, so the nearest value that band can
/// reach to 0.96 is 1.0, and a forecaster who is exactly right fails it
/// 11.5% of the time on the arithmetic alone. Twenty five rows is where
/// 0.96 gets something to land on. Ninety nine needs a hundred, which
/// is the honest price of stating 99% coverage.
///
/// A confidence outside (0, 1) is not a confidence and gets
/// `usize::MAX`: no number of rows makes it judgeable, and
/// `z_for_confidence` refuses the same numbers. `verdict` never asks,
/// because it rejects such a band before it gets here.
pub fn required_band_n(confidence: f64) -> usize {
    // Written so NaN fails it rather than falling through, the same way
    // `z_for_confidence` is written.
    if !(confidence > 0.0 && confidence < 1.0) {
        return usize::MAX;
    }
    // A float to integer cast saturates at the bounds, which is the
    // answer wanted here: a confidence one representable step below 1.0
    // asks for more rows than a record will ever hold.
    let one_expected_miss = (1.0 / (1.0 - confidence)).ceil() as usize;
    one_expected_miss.max(MIN_BAND_N)
}

/// The tolerance a verdict is judged against.
///
/// A newtype rather than a bare `f64` because the design refuses to fix
/// this number: "the specific tolerance is deliberately left to the
/// implementation plan, since it should be set from the first observed
/// curve rather than guessed here." So there is no default, no
/// constant, and nothing to reach for absent-mindedly. Whoever asks for
/// a verdict has to state the number they are judging against, and a
/// number that could only produce one answer is refused rather than
/// quietly accepted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance(f64);

impl Tolerance {
    /// A tolerance in (0.0, 1.0].
    ///
    /// Zero is refused because no forecaster lands on its stated
    /// coverage exactly, so a zero tolerance is a no-go wearing the
    /// costume of a measurement. Anything above one is refused because
    /// it admits every curve that can exist, including 80% intervals
    /// holding the truth never, which is a go wearing the same costume.
    pub fn new(value: f64) -> Result<Self, TrackError> {
        if !value.is_finite() || value <= 0.0 || value > 1.0 {
            return Err(TrackError::Malformed {
                what: "calibration tolerance",
                detail: format!("expected a finite value in (0.0, 1.0], got {value}"),
            });
        }
        Ok(Tolerance(value))
    }

    /// The number, so a report can say what it was judged against.
    pub fn get(self) -> f64 {
        self.0
    }
}

/// Whether surprise can be trusted, and if not, why not.
///
/// This is the go/no-go the design calls the decision point. It is
/// computed, not enforced: nothing in `rerun` or `anomalies` consults
/// it, and it cannot, because the ledger has to be buildable in order
/// to be measured. It exists so the decision is made against a number
/// instead of a feeling, and so "we checked" means something a reader
/// can re-run.
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationVerdict {
    /// Every band sits within tolerance, over enough forecasts overall
    /// and enough in every band to say so.
    Go,
    /// Do not trust surprise. Never empty: a no-go always says why.
    NoGo(Vec<NoGoReason>),
}

impl CalibrationVerdict {
    /// True only for `Go`. A helper so callers do not pattern-match
    /// and accidentally treat a no-go with an empty reason list as a
    /// pass; `verdict` never builds one, and this cannot be fooled.
    pub fn is_go(&self) -> bool {
        matches!(self, CalibrationVerdict::Go)
    }

    /// Why the answer was no. Empty for `Go`.
    pub fn reasons(&self) -> &[NoGoReason] {
        match self {
            CalibrationVerdict::Go => &[],
            CalibrationVerdict::NoGo(reasons) => reasons,
        }
    }
}

/// One reason surprise cannot be trusted.
#[derive(Debug, Clone, PartialEq)]
pub enum NoGoReason {
    /// Too few scored forecasts to answer the question either way.
    ///
    /// Distinct from a demonstrated miss on purpose. "We have not
    /// measured this yet" and "we measured it and it failed" are
    /// different states, and collapsing them would let an empty record
    /// read as a failing one, or worse, a passing one.
    NotEnoughEvidence { n: usize, required: usize },
    /// A band's observed coverage is further from its stated
    /// confidence than the tolerance allows. The spec's example, 80%
    /// intervals holding the truth 45% of the time, is this reason
    /// with a gap of 0.35.
    BandOutOfTolerance {
        confidence: f64,
        observed_coverage: f64,
        gap: f64,
        tolerance: f64,
    },
    /// A band does not hold enough forecasts for its coverage to be
    /// judged, so no claim is made about it in either direction.
    ///
    /// The same distinction `NotEnoughEvidence` draws, one band down.
    /// "We measured this band and it missed" and "this band is three
    /// rows" are different states, and a report that calls the second
    /// one a miss is manufacturing findings its own data cannot carry.
    /// This is not a pass: it blocks the go exactly as a miss does.
    ///
    /// `observed_coverage` is carried so nothing is hidden. A caller
    /// can still see that a thin band looks terrible. What the report
    /// refuses to do is call that a finding.
    BandTooThin {
        confidence: f64,
        n: usize,
        required: usize,
        observed_coverage: f64,
    },
    /// The report carries a number that cannot be compared, so no
    /// answer is possible. Never silently treated as a pass: a NaN
    /// loses every comparison, including the one that would have
    /// caught it, which is exactly how a corrupt record would
    /// otherwise report as well calibrated.
    Unjudgeable { detail: String },
}

impl CalibrationReport {
    /// The go/no-go, judged against a tolerance the caller states.
    ///
    /// Both conditions from the design have to hold: every band within
    /// tolerance, and `n` at least `MIN_CALIBRATION_N`. Every failing
    /// band is reported rather than the first one, because a curve that
    /// misses at 80% and at 99% is a different problem from one that
    /// misses at 80% alone, and the caller cannot see that from a
    /// single reason.
    ///
    /// A third condition sits under the first, because the design's own
    /// reason for `MIN_CALIBRATION_N` applies to a band as much as to a
    /// report. A band holding fewer rows than `required_band_n` asks
    /// for is `BandTooThin` and never `BandOutOfTolerance`, whatever
    /// its gap. Both block the go. The distinction is the point: a gap
    /// computed over three rows is arithmetic about three rows, and
    /// reporting it as a demonstrated miss makes it look exactly like
    /// one. This can only add reasons, never remove them, so it makes a
    /// go harder to reach and never easier.
    ///
    /// An empty report is `NotEnoughEvidence`, never `Go`. A report
    /// nothing has been written to is the case most likely to be asked
    /// about by accident, and answering "well calibrated" to it would
    /// be the worst possible wrong answer.
    pub fn verdict(&self, tolerance: Tolerance) -> CalibrationVerdict {
        let mut reasons = Vec::new();

        if self.n < MIN_CALIBRATION_N {
            reasons.push(NoGoReason::NotEnoughEvidence {
                n: self.n,
                required: MIN_CALIBRATION_N,
            });
        }

        if let Some(width) = self.mean_interval_width {
            if !width.is_finite() {
                reasons.push(NoGoReason::Unjudgeable {
                    detail: format!("mean interval width is {width}"),
                });
            }
        }

        for band in &self.bands {
            let gap = (band.observed_coverage - band.confidence).abs();
            // Finiteness is checked before the comparison, not after.
            // `NaN > tolerance` is false, so a NaN band would slip past
            // the tolerance test and contribute nothing to `reasons`,
            // and a report made entirely of NaN bands would return Go.
            if !gap.is_finite() {
                reasons.push(NoGoReason::Unjudgeable {
                    detail: format!(
                        "band at stated confidence {} has an uncomparable gap",
                        band.confidence
                    ),
                });
                continue;
            }
            // A stated confidence outside (0, 1) is not a coverage
            // anything can be measured against, and `z_for_confidence`
            // refuses the same numbers. Left alone such a band passes
            // on a gap of zero: one stating 1.0 with every outcome
            // inside its own interval would read as perfect
            // calibration.
            if !(band.confidence > 0.0 && band.confidence < 1.0) {
                reasons.push(NoGoReason::Unjudgeable {
                    detail: format!(
                        "band states a confidence of {}, which is not in (0, 1)",
                        band.confidence
                    ),
                });
                continue;
            }
            // Thinness is checked before the tolerance, because a band
            // too thin to judge cannot demonstrate a miss either. The
            // gap is still arithmetic, it is just arithmetic about
            // three rows, and reporting it as a finding makes it
            // indistinguishable from a real one.
            let required = required_band_n(band.confidence);
            if band.n < required {
                reasons.push(NoGoReason::BandTooThin {
                    confidence: band.confidence,
                    n: band.n,
                    required,
                    observed_coverage: band.observed_coverage,
                });
                continue;
            }
            if gap > tolerance.get() {
                reasons.push(NoGoReason::BandOutOfTolerance {
                    confidence: band.confidence,
                    observed_coverage: band.observed_coverage,
                    gap,
                    tolerance: tolerance.get(),
                });
            }
        }

        if reasons.is_empty() {
            CalibrationVerdict::Go
        } else {
            CalibrationVerdict::NoGo(reasons)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::MetricValue;
    use crate::track::Store;
    use tempfile::tempdir;

    fn open_store(dir: &std::path::Path) -> Store {
        Store::open(&dir.join("zorp.duckdb")).unwrap()
    }

    /// The two-sided 80% critical value, written out rather than taken
    /// from `z_for_confidence`. A fixture that asked the code under
    /// test where to put the interval would agree with it no matter
    /// what it said.
    const Z80: f64 = 1.281551565545;

    /// A seeded linear congruential generator, written here so the
    /// samples are identical on every machine and every run. No
    /// external crate, and nothing iterating a HashMap.
    struct Lcg(u64);

    impl Lcg {
        /// Knuth's MMIX multiplier and increment.
        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0
        }

        /// A uniform strictly inside (0, 1). The high bits are the ones
        /// used, because an LCG's low bits cycle with short periods,
        /// and the half-step keeps the value off both ends, which
        /// matters because the transform below takes its logarithm.
        fn next_unit(&mut self) -> f64 {
            ((self.next_u64() >> 11) as f64 + 0.5) / 9007199254740992.0
        }

        /// One standard normal deviate, by the Box-Muller transform.
        ///
        /// Box-Muller rather than the inverse CDF on purpose. Drawing
        /// the samples through `probit` would make the samples and the
        /// thing being measured share an approximation, and a shared
        /// error cancels instead of showing up.
        fn next_normal(&mut self) -> f64 {
            let u1 = self.next_unit();
            let u2 = self.next_unit();
            (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
        }
    }

    /// Every table in the database with its row count, ordered by name
    /// so two calls are comparable. A `Vec` rather than a map, so
    /// nothing here depends on hash iteration order.
    fn table_row_counts(store: &Store) -> Vec<(String, i64)> {
        let mut stmt = store
            .conn
            .prepare("SELECT table_name FROM duckdb_tables() ORDER BY table_name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        names
            .into_iter()
            .map(|name| {
                let count: i64 = store
                    .conn
                    .query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                (name, count)
            })
            .collect()
    }

    /// Write one expectation straight into the table.
    ///
    /// Deliberately not the writer in `expectations.rs`. This module
    /// reads the `expectations` table, not that module's API, so the
    /// fixtures here write the table too. The report has to keep
    /// working against rows written by anything, including a run from
    /// an older version of the writer.
    fn insert_expectation(
        store: &Store,
        experiment_id: &str,
        metric_key: &str,
        expected_value: f64,
        interval: (f64, f64),
        confidence: f64,
    ) {
        let id = format!("{experiment_id}-{metric_key}-{}", crate::id::next_seq());
        store
            .conn
            .execute(
                "INSERT INTO expectations (id, experiment_id, metric_key, expected_value, interval_low, interval_high, confidence, assumptions, recorded_at, seq) \
                 SELECT ?, ?, ?, ?, ?, ?, ?, NULL, 0, COALESCE(MAX(seq), -1) + 1 FROM expectations WHERE experiment_id = ?",
                duckdb::params![
                    id,
                    experiment_id,
                    metric_key,
                    expected_value,
                    interval.0,
                    interval.1,
                    confidence,
                    experiment_id
                ],
            )
            .unwrap();
    }

    /// The two-sided central-interval critical values every statistics
    /// table prints. The spec names z(0.80) = 1.2816 directly.
    #[test]
    fn z_matches_published_two_sided_critical_values() {
        for (confidence, expected) in [
            (0.80, 1.2816),
            (0.90, 1.6449),
            (0.95, 1.9600),
            (0.99, 2.5758),
        ] {
            let z = z_for_confidence(confidence).expect("a confidence in (0, 1) has a z");
            assert!(
                (z - expected).abs() < 1e-4,
                "z({confidence}) = {z}, published value is {expected}"
            );
        }
    }

    /// The same check at full precision, including two deep-tail bands,
    /// so the accuracy claim in `probit`'s comment is a tested claim
    /// rather than a quoted one.
    ///
    /// The bound is 1e-8 absolute. Acklam's claim is 1.15e-9 relative,
    /// which at z = 3.89 is 4.5e-9 absolute, and the worst error these
    /// six bands actually show is 1.4e-9, at z(0.80). Tightening this to
    /// 1e-12 fails, which is how the number above was measured.
    #[test]
    fn z_holds_to_the_published_error_bound() {
        for (confidence, expected) in [
            (0.80, 1.281551565545),
            (0.90, 1.644853626951),
            (0.95, 1.959963984540),
            (0.99, 2.575829303549),
            (0.999, 3.290526731492),
            (0.9999, 3.890591886413),
        ] {
            let z = z_for_confidence(confidence).expect("a confidence in (0, 1) has a z");
            assert!(
                (z - expected).abs() < 1e-8,
                "z({confidence}) = {z}, true value is {expected}, error {}",
                (z - expected).abs()
            );
        }
    }

    /// A confidence outside (0, 1) has no critical value. Saying so is
    /// the point: the quantile runs to infinity at the ends, so the
    /// alternative is handing back an infinity or a NaN that then
    /// travels into every sigma computed from it.
    #[test]
    fn a_confidence_outside_the_unit_interval_has_no_z() {
        for confidence in [0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(
                z_for_confidence(confidence),
                Err(Undefined::Confidence),
                "confidence {confidence} should have no z"
            );
        }
    }

    /// A 95% interval two z wide is one sigma wide by construction, and
    /// where the interval sits on the line does not change that. Both
    /// cases here are the same width, one centred on zero and one not.
    #[test]
    fn sigma_is_the_half_width_in_critical_values() {
        let z95 = 1.959963984540;
        for (low, high) in [(-z95, z95), (0.0, 2.0 * z95), (100.0, 100.0 + 2.0 * z95)] {
            let s = sigma(low, high, 0.95).expect("a positive width has a sigma");
            assert!(
                (s - 1.0).abs() < 1e-8,
                "sigma([{low}, {high}], 0.95) = {s}, expected 1.0"
            );
        }
    }

    /// The degenerate intervals, each named rather than silently turned
    /// into a number. A zero-width interval is the one that matters: it
    /// claims the outcome is known exactly, so every deviation from it
    /// is infinitely surprising and an exact hit is 0/0.
    #[test]
    fn a_degenerate_interval_has_no_sigma() {
        let cases = [
            ((5.0, 5.0), Undefined::ZeroWidthInterval),
            ((0.0, 0.0), Undefined::ZeroWidthInterval),
            ((10.0, 2.0), Undefined::InvertedInterval),
            ((f64::NAN, 1.0), Undefined::NotFinite),
            ((0.0, f64::NAN), Undefined::NotFinite),
            ((f64::NEG_INFINITY, 1.0), Undefined::NotFinite),
            ((0.0, f64::INFINITY), Undefined::NotFinite),
        ];
        for ((low, high), expected) in cases {
            assert_eq!(
                sigma(low, high, 0.80),
                Err(expected),
                "sigma([{low}, {high}], 0.80)"
            );
        }
    }

    /// Surprise counts sigmas, and it does not care which side of the
    /// forecast the outcome fell on. The first three cases are one
    /// sigma wide, the last is two.
    #[test]
    fn surprise_counts_sigmas_in_either_direction() {
        let z95 = 1.959963984540;
        let cases = [
            (0.0, 0.0, -z95, z95, 0.0),
            (3.0, 0.0, -z95, z95, 3.0),
            (-3.0, 0.0, -z95, z95, 3.0),
            (17.0, 10.0, 10.0 - 2.0 * z95, 10.0 + 2.0 * z95, 3.5),
        ];
        for (observed, expected_value, low, high, want) in cases {
            let s = surprise(observed, expected_value, low, high, 0.95)
                .expect("a positive width has a surprise");
            assert!(
                (s - want).abs() < 1e-8,
                "surprise({observed} against {expected_value} in [{low}, {high}]) = {s}, expected {want}"
            );
        }
    }

    /// The zero-width case again, one level up, because this is where
    /// the bad numbers would actually escape. An exact hit is 0/0 and a
    /// miss is x/0, so a forecaster who claimed certainty would appear
    /// in the ledger as `NaN` and `inf` sigmas. Neither is a count of
    /// anything. A NaN outcome gets the same treatment.
    #[test]
    fn a_forecast_that_claims_certainty_yields_no_surprise_figure() {
        assert_eq!(
            surprise(5.0, 5.0, 5.0, 5.0, 0.80),
            Err(Undefined::ZeroWidthInterval),
            "an exact hit on a zero-width interval"
        );
        assert_eq!(
            surprise(9.0, 5.0, 5.0, 5.0, 0.80),
            Err(Undefined::ZeroWidthInterval),
            "a miss against a zero-width interval"
        );
        assert_eq!(
            surprise(f64::NAN, 5.0, 4.0, 6.0, 0.80),
            Err(Undefined::NotFinite),
            "an outcome that is not a number"
        );
    }

    /// A record with no forecasts in it has no coverage, which is not
    /// the same as coverage of zero. The report says so rather than
    /// reporting a 0.0 that reads like total failure.
    #[test]
    fn an_empty_record_has_no_coverage_rather_than_zero_coverage() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 0);
        assert_eq!(report.covered, 0);
        assert_eq!(report.observed_coverage(), None);
        assert_eq!(report.mean_interval_width, None);
        assert!(report.bands.is_empty());
    }

    /// Two forecasts at the same stated confidence, one outcome inside
    /// its interval and one outside. The answer is known before the
    /// query runs: n of 2, covered of 1, coverage of exactly one half.
    #[test]
    fn coverage_counts_the_outcomes_that_landed_inside_their_interval() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let hit = store.create_experiment("t1", "t1-prereg").unwrap();
        let miss = store.create_experiment("t1", "t1-prereg").unwrap();

        insert_expectation(&store, &hit.id, "accuracy", 0.5, (0.4, 0.6), 0.80);
        store
            .record_metric(&hit.id, "accuracy", MetricValue::Number(0.55))
            .unwrap();
        insert_expectation(&store, &miss.id, "accuracy", 0.5, (0.4, 0.6), 0.80);
        store
            .record_metric(&miss.id, "accuracy", MetricValue::Number(0.9))
            .unwrap();

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 2);
        assert_eq!(report.covered, 1);
        assert_eq!(report.observed_coverage(), Some(0.5));
    }

    /// A revised forecast counts once, and the version that counts is
    /// the last one standing when the outcome landed.
    ///
    /// `expectations.rs` allows a forecast to be rewritten while no
    /// outcome exists, on the grounds that revising a belief before
    /// observing anything is legitimate. That freedom has to stop at
    /// this query or it becomes a way to buy coverage: write one
    /// hopeless interval and nine absurdly wide ones and nine tenths of
    /// the record reads as covered. Counting only the last forecast is
    /// the mirror of taking the first outcome, and between them one
    /// forecast is scored against one result.
    #[test]
    fn a_revised_forecast_is_scored_once_on_its_final_version() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        // A first guess so wide it cannot miss, then the real forecast.
        insert_expectation(&store, &exp.id, "accuracy", 0.5, (0.0, 100.0), 0.80);
        insert_expectation(&store, &exp.id, "accuracy", 0.8, (0.7, 0.9), 0.80);
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.5))
            .unwrap();

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 1, "one forecast about one metric is one row");
        assert_eq!(
            report.covered, 0,
            "0.5 is outside the forecast that stood, so this is a miss"
        );
    }

    /// The curve itself: stated confidence against what was observed at
    /// it. The two bands are written interleaved, so grouping them is
    /// real work rather than an artifact of insertion order, and the
    /// answer is known in advance: two of three at 0.80, two of two at
    /// 0.95.
    #[test]
    fn the_curve_is_one_point_per_stated_confidence_in_ascending_order() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();

        // (stated confidence, observed value); the interval is always
        // [0.4, 0.6], so 0.55 is covered and 0.9 is not.
        let plan = [
            (0.95, 0.55),
            (0.80, 0.55),
            (0.95, 0.55),
            (0.80, 0.9),
            (0.80, 0.55),
        ];
        for (confidence, observed) in plan {
            let exp = store.create_experiment("t1", "t1-prereg").unwrap();
            insert_expectation(&store, &exp.id, "accuracy", 0.5, (0.4, 0.6), confidence);
            store
                .record_metric(&exp.id, "accuracy", MetricValue::Number(observed))
                .unwrap();
        }

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 5);
        assert_eq!(report.covered, 4);
        assert_eq!(report.bands.len(), 2);
        assert_eq!(report.bands[0].confidence, 0.80);
        assert_eq!(report.bands[0].n, 3);
        assert_eq!(report.bands[0].covered, 2);
        assert_eq!(report.bands[0].observed_coverage, 2.0 / 3.0);
        assert_eq!(report.bands[1].confidence, 0.95);
        assert_eq!(report.bands[1].n, 2);
        assert_eq!(report.bands[1].covered, 2);
        assert_eq!(report.bands[1].observed_coverage, 1.0);
        assert_eq!(report.curve(), vec![(0.80, 2.0 / 3.0), (0.95, 1.0)]);
    }

    /// Perfect coverage, bought. One forecast is a quarter wide and the
    /// other is sixty-four times that, and both contain the outcome, so
    /// the curve reads 1.0 at a stated 0.80 and says nothing useful.
    /// The mean width is the column that gives it away.
    ///
    /// The widths are exact in binary, so the expected mean is an exact
    /// comparison rather than a tolerance.
    #[test]
    fn mean_interval_width_shows_coverage_that_was_bought() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();

        for (low, high) in [(0.0, 0.25), (-8.0, 8.0)] {
            let exp = store.create_experiment("t1", "t1-prereg").unwrap();
            insert_expectation(&store, &exp.id, "accuracy", 0.125, (low, high), 0.80);
            store
                .record_metric(&exp.id, "accuracy", MetricValue::Number(0.1))
                .unwrap();
        }

        let report = store.calibration_report().unwrap();

        assert_eq!(report.observed_coverage(), Some(1.0));
        assert_eq!(report.mean_interval_width, Some(8.125));
        assert_eq!(report.bands.len(), 1);
        assert_eq!(report.bands[0].mean_interval_width, 8.125);
    }

    /// Record 400 forecasts from a forecaster who is telling the truth:
    /// outcomes drawn from a standard normal, intervals set to the
    /// central 80% of exactly that distribution.
    ///
    /// Correct coverage is known before the query runs. It is 0.80 by
    /// construction, and the exact number of covered draws is counted
    /// in the loop from the same comparison the report uses, so the
    /// first assertion pins the report's arithmetic and the second
    /// pins the construction. The seed is fixed, so neither one can
    /// pass on one run and fail on the next.
    #[test]
    fn an_honest_forecaster_lands_on_the_diagonal() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let mut rng = Lcg(20260819);
        let mut drawn_inside = 0usize;

        for _ in 0..400 {
            let observed = rng.next_normal();
            // Inclusive at both ends, the same comparison the report
            // makes, so the count below cannot disagree with it over a
            // draw that lands on a bound.
            if (-Z80..=Z80).contains(&observed) {
                drawn_inside += 1;
            }
            let exp = store.create_experiment("t1", "t1-prereg").unwrap();
            insert_expectation(&store, &exp.id, "accuracy", 0.0, (-Z80, Z80), 0.80);
            store
                .record_metric(&exp.id, "accuracy", MetricValue::Number(observed))
                .unwrap();
        }

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 400);
        assert_eq!(report.covered, drawn_inside);
        let coverage = report
            .observed_coverage()
            .expect("400 forecasts have coverage");
        // Three standard errors at n = 400 and p = 0.8 is 0.06. The
        // seed is fixed, so this either passes always or never.
        assert!(
            (coverage - 0.80).abs() < 0.06,
            "a truthful 80% forecaster read as {coverage}"
        );
        assert_eq!(report.bands.len(), 1);
        // A tolerance, not an equality, and the reason is worth
        // knowing: 400 identical widths summed and divided by 400 does
        // not return the width bit for bit, because each partial sum is
        // rounded. The error here is about 2e-14. The two-row width
        // test above can assert exactly because that sum is exact.
        assert!(
            (report.bands[0].mean_interval_width - 2.0 * Z80).abs() < 1e-12,
            "mean width read as {}",
            report.bands[0].mean_interval_width
        );
    }

    /// The same 400 draws, but the forecaster states 80% and writes
    /// intervals half as wide as an 80% interval is.
    ///
    /// Correct coverage is again known in advance: the central half of
    /// an 80% band is z = 0.6408, and a standard normal falls inside
    /// that about 47.8% of the time. This is the spec's failure case,
    /// the one the whole report exists to catch, and it is caught with
    /// the mean width cut exactly in half rather than raised.
    #[test]
    fn an_overconfident_forecaster_falls_below_the_diagonal() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let mut rng = Lcg(20260819);
        let half = Z80 / 2.0;
        let mut drawn_inside = 0usize;

        for _ in 0..400 {
            let observed = rng.next_normal();
            if (-half..=half).contains(&observed) {
                drawn_inside += 1;
            }
            let exp = store.create_experiment("t1", "t1-prereg").unwrap();
            insert_expectation(&store, &exp.id, "accuracy", 0.0, (-half, half), 0.80);
            store
                .record_metric(&exp.id, "accuracy", MetricValue::Number(observed))
                .unwrap();
        }

        let report = store.calibration_report().unwrap();

        assert_eq!(report.covered, drawn_inside);
        let coverage = report
            .observed_coverage()
            .expect("400 forecasts have coverage");
        assert!(
            (coverage - 0.478).abs() < 0.06,
            "a half-width 80% forecaster read as {coverage}"
        );
        assert_eq!(report.curve().len(), 1);
        assert_eq!(report.curve()[0].0, 0.80);
        assert!(
            report.curve()[0].1 < 0.60,
            "the curve should sit well below the stated band"
        );
        // Half the width of the honest run, to within the rounding a
        // 400-term float sum carries. See the note in that test.
        let mean = report
            .mean_interval_width
            .expect("400 forecasts have a width");
        assert!(
            (mean - Z80).abs() < 1e-12,
            "mean width read as {mean}, expected half of {}",
            2.0 * Z80
        );
    }

    /// Integrity rule 4: detectors and reports read, they do not write.
    /// Counting every row in every table is a blunt check, and blunt is
    /// what is wanted, since it also catches a write to a table this
    /// module has no business touching.
    #[test]
    fn the_report_writes_nothing() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        insert_expectation(&store, &exp.id, "accuracy", 0.5, (0.4, 0.6), 0.80);
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.55))
            .unwrap();

        let before = table_row_counts(&store);
        store.calibration_report().unwrap();
        let after = table_row_counts(&store);

        assert_eq!(before, after);
        // Guard against the counts being empty, which would make the
        // comparison above true for the wrong reason.
        assert!(before.iter().any(|(_, count)| *count > 0));
    }

    /// Only a number can be inside or outside an interval. A text
    /// metric under the forecast key is not an outcome, and the
    /// forecast waits rather than being scored against something that
    /// cannot be compared.
    #[test]
    fn a_text_metric_is_not_an_outcome() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        insert_expectation(&store, &exp.id, "accuracy", 0.5, (0.4, 0.6), 0.80);
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Text("about half".into()))
            .unwrap();

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 0);
        assert!(report.bands.is_empty());
    }

    /// A forecast whose experiment has not produced that metric yet is
    /// not counted. Counting it would score a forecast that has not
    /// been tested, and the direction of the error is the bad one: an
    /// untested forecast would read as a miss.
    #[test]
    fn a_forecast_still_waiting_for_its_outcome_is_not_counted() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        insert_expectation(&store, &exp.id, "accuracy", 0.5, (0.4, 0.6), 0.80);
        // A different metric, and the right metric on a different
        // experiment. Neither is this forecast's outcome.
        store
            .record_metric(&exp.id, "latency_ms", MetricValue::Number(0.55))
            .unwrap();
        let other = store.create_experiment("t1", "t1-prereg").unwrap();
        store
            .record_metric(&other.id, "accuracy", MetricValue::Number(0.55))
            .unwrap();

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 0);
    }

    /// A metric key recorded more than once for one experiment scores
    /// the forecast against the first value recorded, not the last and
    /// not once per row. The first is the result the forecast was made
    /// about; later rows are reruns, and letting them in would let one
    /// forecast weigh as many.
    #[test]
    fn the_first_recorded_value_is_the_outcome() {
        let dir = tempdir().unwrap();
        let store = open_store(dir.path());
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        insert_expectation(&store, &exp.id, "accuracy", 0.5, (0.4, 0.6), 0.80);
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.9))
            .unwrap();
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.55))
            .unwrap();
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.5))
            .unwrap();

        let report = store.calibration_report().unwrap();

        assert_eq!(report.n, 1, "one forecast, one row");
        assert_eq!(report.covered, 0, "the first value, 0.9, is outside");
    }

    // ----- the go/no-go verdict -----

    /// A report with the bands and counts a caller would have, built
    /// directly rather than through the store. The verdict is
    /// arithmetic over a report, so driving it through DuckDB would be
    /// testing the reader instead of the thing under test.
    fn report(n: usize, covered: usize, bands: Vec<(f64, usize, usize, f64)>) -> CalibrationReport {
        CalibrationReport {
            n,
            covered,
            mean_interval_width: Some(0.2),
            bands: bands
                .into_iter()
                .map(
                    |(confidence, bn, bcov, observed_coverage)| CalibrationBand {
                        confidence,
                        n: bn,
                        covered: bcov,
                        observed_coverage,
                        mean_interval_width: 0.2,
                    },
                )
                .collect(),
        }
    }

    fn tol(v: f64) -> Tolerance {
        Tolerance::new(v).expect("test tolerance is in range")
    }

    /// Deleting the `n < MIN_CALIBRATION_N` check makes this return Go.
    #[test]
    fn a_report_with_too_few_forecasts_is_not_a_go() {
        let r = report(49, 40, vec![(0.80, 49, 40, 0.80)]);

        let v = r.verdict(tol(0.10));

        assert!(!v.is_go(), "49 forecasts is below the stated minimum of 50");
        assert_eq!(
            v.reasons(),
            [NoGoReason::NotEnoughEvidence {
                n: 49,
                required: 50
            }]
        );
    }

    /// The boundary is inclusive, so exactly the minimum passes. Paired
    /// with the test above: together they pin the comparison to `<`,
    /// and widening it to `<=` fails this one.
    #[test]
    fn exactly_the_minimum_number_of_forecasts_is_enough() {
        let r = report(50, 40, vec![(0.80, 50, 40, 0.80)]);

        assert!(r.verdict(tol(0.10)).is_go());
    }

    /// The design's own failure case: 80% intervals holding the truth
    /// 45% of the time. Deleting the tolerance comparison makes this a
    /// Go.
    #[test]
    fn the_designs_failure_case_is_a_no_go() {
        let r = report(60, 27, vec![(0.80, 60, 27, 0.45)]);

        let v = r.verdict(tol(0.10));

        assert!(!v.is_go());
        match &v.reasons()[0] {
            NoGoReason::BandOutOfTolerance {
                gap, confidence, ..
            } => {
                assert_eq!(*confidence, 0.80);
                assert!((gap - 0.35).abs() < 1e-12, "gap was {gap}");
            }
            other => panic!("wrong reason: {other:?}"),
        }
    }

    /// A gap exactly on the tolerance is inside it. The gap is taken
    /// from the same subtraction the code performs, so this really does
    /// sit on the boundary rather than near it, and the second half
    /// proves it by moving one representable step and failing.
    #[test]
    fn a_gap_exactly_on_the_tolerance_is_allowed() {
        let r = report(60, 42, vec![(0.75, 60, 42, 0.70)]);
        let gap = 0.75_f64 - 0.70_f64;

        assert!(
            r.verdict(Tolerance::new(gap).unwrap()).is_go(),
            "a gap equal to the tolerance is within it"
        );
        assert!(
            !r.verdict(Tolerance::new(gap - f64::EPSILON).unwrap())
                .is_go(),
            "one step tighter and the same report fails"
        );
    }

    /// Every failing band is reported, not just the first one.
    #[test]
    fn each_band_out_of_tolerance_gets_its_own_reason() {
        let r = report(90, 30, vec![(0.80, 45, 20, 0.44), (0.95, 45, 10, 0.22)]);

        let v = r.verdict(tol(0.10));

        assert_eq!(v.reasons().len(), 2, "two bands missed, two reasons");
    }

    /// The case the finiteness check exists for. A NaN gap loses every
    /// comparison, so without that check this returns Go on a report
    /// made entirely of uncomparable numbers.
    #[test]
    fn a_band_that_cannot_be_compared_is_never_a_go() {
        let r = report(60, 30, vec![(0.80, 60, 30, f64::NAN)]);

        let v = r.verdict(tol(0.10));

        assert!(
            !v.is_go(),
            "a NaN coverage must not slip past the tolerance test"
        );
        assert!(matches!(v.reasons()[0], NoGoReason::Unjudgeable { .. }));
    }

    /// Coverage can be bought with uselessly wide intervals, so a mean
    /// width that is not a number is not something to pass a verdict
    /// over.
    #[test]
    fn a_non_finite_mean_interval_width_is_not_a_go() {
        let mut r = report(60, 48, vec![(0.80, 60, 48, 0.80)]);
        r.mean_interval_width = Some(f64::INFINITY);

        let v = r.verdict(tol(0.10));

        assert!(!v.is_go());
        assert!(matches!(v.reasons()[0], NoGoReason::Unjudgeable { .. }));
    }

    /// The worst available wrong answer. An empty record is what
    /// somebody asking the question by accident will be holding.
    #[test]
    fn an_empty_report_is_not_a_go() {
        let v = CalibrationReport::default().verdict(tol(0.10));

        assert!(
            !v.is_go(),
            "nothing measured is not the same as measured and fine"
        );
        assert_eq!(
            v.reasons(),
            [NoGoReason::NotEnoughEvidence { n: 0, required: 50 }]
        );
    }

    /// A tolerance that could only ever produce one answer is refused,
    /// so it cannot be smuggled in wearing the costume of a
    /// measurement.
    #[test]
    fn a_tolerance_that_decides_nothing_is_refused() {
        for bad in [0.0, -0.1, 1.5, f64::NAN, f64::INFINITY] {
            assert!(
                Tolerance::new(bad).is_err(),
                "{bad} should not be usable as a tolerance"
            );
        }
        for good in [f64::EPSILON, 0.05, 1.0] {
            assert!(Tolerance::new(good).is_ok(), "{good} should be usable");
        }
    }

    /// `reasons()` is empty for a Go, so a caller iterating reasons
    /// cannot read a pass as a failure that forgot to explain itself.
    #[test]
    fn a_go_carries_no_reasons() {
        let v = report(50, 40, vec![(0.80, 50, 40, 0.80)]).verdict(tol(0.10));

        assert!(v.is_go());
        assert!(v.reasons().is_empty());
    }

    // ----- bands too thin to judge -----

    /// The six band run that exposed this, scored at the loosest
    /// tolerance a verdict can be asked for. Three forecasts at a
    /// stated 0.96 with one covered used to come back as a
    /// demonstrated miss with a gap of 0.627. Three rows can only ever
    /// read 0, 1/3, 2/3 or 1, so the nearest coverage that band can
    /// reach to 0.96 is a perfect score, and a forecaster who is
    /// exactly right fails it 11.5% of the time by arithmetic alone.
    /// Not one band here carries the rows to demonstrate anything.
    #[test]
    fn the_run_that_exposed_this_reports_thin_bands_and_no_misses() {
        let r = report(
            35,
            31,
            vec![
                (0.93, 3, 3, 1.0),
                (0.95, 5, 5, 1.0),
                (0.96, 3, 1, 1.0 / 3.0),
                (0.97, 11, 10, 10.0 / 11.0),
                (0.98, 9, 8, 8.0 / 9.0),
                (0.99, 4, 4, 1.0),
            ],
        );

        let v = r.verdict(tol(0.20));

        assert!(!v.is_go());
        assert!(
            !v.reasons()
                .iter()
                .any(|r| matches!(r, NoGoReason::BandOutOfTolerance { .. })),
            "no band here has the rows to show a miss: {:?}",
            v.reasons()
        );
        assert_eq!(
            v.reasons()
                .iter()
                .filter(|r| matches!(r, NoGoReason::BandTooThin { .. }))
                .count(),
            6,
            "every band is too thin to judge"
        );
        assert!(
            v.reasons().contains(&NoGoReason::BandTooThin {
                confidence: 0.96,
                n: 3,
                required: 25,
                observed_coverage: 1.0 / 3.0,
            }),
            "the thin band says how many rows it would have taken: {:?}",
            v.reasons()
        );
    }

    /// Thin is not a pass and thick is not a reprieve. The band with
    /// the rows to show a miss still shows one, in the same report as
    /// the band without them.
    #[test]
    fn a_band_with_the_rows_to_show_a_miss_still_shows_one() {
        let r = report(63, 28, vec![(0.80, 60, 27, 0.45), (0.96, 3, 1, 1.0 / 3.0)]);

        let v = r.verdict(tol(0.20));

        assert_eq!(v.reasons().len(), 2, "{:?}", v.reasons());
        assert!(matches!(
            v.reasons()[0],
            NoGoReason::BandOutOfTolerance {
                confidence: 0.80,
                ..
            }
        ));
        assert!(matches!(
            v.reasons()[1],
            NoGoReason::BandTooThin {
                confidence: 0.96,
                ..
            }
        ));
    }

    /// Deleting the per-band minimum in `verdict` makes this return Go.
    /// Fifty forecasts, so the overall minimum is met, spread over five
    /// bands of ten that each land exactly on their stated confidence.
    /// Every gap is zero and none of it means anything: at ten rows the
    /// coverage moves in tenths, and landing on the diagonal is what a
    /// coarse grid does, not what a calibrated forecaster proves.
    #[test]
    fn a_report_made_entirely_of_thin_bands_is_not_a_go() {
        let r = report(
            50,
            35,
            vec![
                (0.50, 10, 5, 0.50),
                (0.60, 10, 6, 0.60),
                (0.70, 10, 7, 0.70),
                (0.80, 10, 8, 0.80),
                (0.90, 10, 9, 0.90),
            ],
        );

        let v = r.verdict(tol(0.10));

        assert!(!v.is_go(), "five bands of ten prove nothing at all");
        assert_eq!(v.reasons().len(), 5);
        assert!(v
            .reasons()
            .iter()
            .all(|r| matches!(r, NoGoReason::BandTooThin { required: 20, .. })));
    }

    /// The per-band boundary is inclusive, so a band carrying exactly
    /// the rows required is judged and can fail. Paired with the test
    /// below: together they pin the comparison to `<`, and widening it
    /// to `<=` fails this one.
    #[test]
    fn a_band_with_exactly_the_rows_required_is_judged() {
        let r = report(60, 32, vec![(0.60, 40, 24, 0.60), (0.80, 20, 8, 0.40)]);

        let v = r.verdict(tol(0.10));

        assert_eq!(v.reasons().len(), 1, "{:?}", v.reasons());
        assert!(
            matches!(v.reasons()[0], NoGoReason::BandOutOfTolerance { .. }),
            "twenty rows at a stated 0.80 is enough to show a miss"
        );
    }

    /// One row short of the requirement and the same miss is no longer
    /// a finding. This is the case the fix suppresses: a gap of 0.43
    /// that used to read as a demonstrated miss now reads as a band
    /// nobody has gathered enough of. It still blocks the go.
    #[test]
    fn one_row_short_and_the_same_band_is_not_judged() {
        let r = report(
            59,
            31,
            vec![(0.60, 40, 24, 0.60), (0.80, 19, 7, 7.0 / 19.0)],
        );

        let v = r.verdict(tol(0.10));

        assert!(!v.is_go());
        assert_eq!(
            v.reasons(),
            [NoGoReason::BandTooThin {
                confidence: 0.80,
                n: 19,
                required: 20,
                observed_coverage: 7.0 / 19.0,
            }]
        );
    }

    /// Deleting the `1 / (1 - confidence)` half of `required_band_n`
    /// makes this return Go. Sixty forecasts clears both flat minimums
    /// and every outcome landed inside its interval, but a 0.99 band at
    /// sixty rows can only read 59/60 or 1.0, and the stated 0.99 falls
    /// between them. A hundred rows is where one expected miss fits.
    #[test]
    fn a_high_confidence_band_needs_more_rows_than_the_flat_floor() {
        let r = report(60, 60, vec![(0.99, 60, 60, 1.0)]);

        let v = r.verdict(tol(0.10));

        assert_eq!(
            v.reasons(),
            [NoGoReason::BandTooThin {
                confidence: 0.99,
                n: 60,
                required: 100,
                observed_coverage: 1.0,
            }]
        );
    }

    /// Deleting the range check on the stated confidence makes this
    /// return Go. A band claiming coverage of 1.0 with every outcome
    /// inside its own interval has a gap of zero, so it passes the
    /// tolerance test while asserting something no interval can mean.
    /// `z_for_confidence` refuses the same number.
    #[test]
    fn a_stated_confidence_that_is_not_a_confidence_is_never_a_go() {
        for bad in [1.0, 0.0, -0.5, 2.0] {
            let r = report(60, 60, vec![(bad, 60, 60, bad)]);

            let v = r.verdict(tol(0.10));

            assert!(!v.is_go(), "{bad} is not a stated confidence");
            assert!(
                matches!(v.reasons()[0], NoGoReason::Unjudgeable { .. }),
                "{bad} gave {:?}",
                v.reasons()
            );
        }
    }

    /// What a band needs rises with what it claims. The numbers are
    /// written out rather than derived, so a change to the formula has
    /// to be argued for here.
    #[test]
    fn the_rows_a_band_needs_rise_with_its_stated_confidence() {
        assert_eq!(required_band_n(0.50), MIN_BAND_N);
        assert_eq!(required_band_n(0.90), MIN_BAND_N);
        assert_eq!(required_band_n(0.95), 20);
        assert_eq!(required_band_n(0.96), 25);
        assert_eq!(required_band_n(0.97), 34);
        assert_eq!(required_band_n(0.98), 50);
        assert_eq!(required_band_n(0.99), 100);
        assert_eq!(required_band_n(0.999), 1000);
        for not_a_confidence in [0.0, 1.0, -0.1, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(
                required_band_n(not_a_confidence),
                usize::MAX,
                "{not_a_confidence} is not a stated confidence"
            );
        }
    }

    /// The per-band rule is additional, not a replacement. A report
    /// short of the overall minimum still says so, and says it on its
    /// own when every band in it is thick enough to judge.
    #[test]
    fn the_overall_minimum_still_stands_on_its_own() {
        let r = report(49, 40, vec![(0.80, 49, 40, 0.80)]);

        assert_eq!(
            r.verdict(tol(0.10)).reasons(),
            [NoGoReason::NotEnoughEvidence {
                n: 49,
                required: 50
            }],
            "a band of 49 is judgeable, so the only complaint is the total"
        );
    }
}
