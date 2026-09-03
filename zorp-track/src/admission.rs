//! The hypothesis-search admission reading.
//!
//! This is a pure reader over the aryabhatta ledger. It reports the four
//! numbers that must hold before a person may decide to cross the
//! hypothesis-search admission gate. It writes nothing, and deliberately
//! does not build the `ExperimentRow` adapter: that adapter would be the
//! gate crossing itself. See `zorp-track/examples/gate_status.rs`.

use std::collections::{BTreeMap, BTreeSet};

use crate::anomalies::{AnomalyStatus, NoiseReport};
use crate::calibration::{self, CalibrationVerdict, NoGoReason, Tolerance};
use crate::experiment::MetricValue;
use crate::rerun::GateOutcome;
use crate::track::Store;
use crate::TrackError;

/// Minimum reproduced admissions, from the 2026-08-28 entry's minimum
/// support for every non-habituation boredom detector.
pub const REQUIRED_REPRODUCED: usize = 12;

/// Minimum condition keys that actually varied, from the 2026-09-02 entry.
pub const REQUIRED_VARYING_KEYS: usize = 3;

/// What the ledger holds in the terms of the admission gate.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmissionReading {
    pub experiments: usize,
    pub experiments_with_anomaly: usize,
    pub superseded: usize,
    pub spanned_keys: BTreeSet<String>,
    pub spanned_atoms: BTreeSet<(String, String)>,
    pub all_keys: BTreeMap<String, BTreeSet<String>>,
    pub forecasts: usize,
    pub scored: usize,
    pub candidates: usize,
    pub widths: Vec<f64>,
    pub reproduced: usize,
    pub unverifiable: usize,
    /// Values each spanned key took among experiments producing counted rows.
    pub varying_keys: BTreeMap<String, BTreeSet<String>>,
    pub noise: NoiseReport,
    pub calibration: CalibrationVerdict,
}

impl Default for AdmissionReading {
    fn default() -> Self {
        Self {
            experiments: 0,
            experiments_with_anomaly: 0,
            superseded: 0,
            spanned_keys: BTreeSet::new(),
            spanned_atoms: BTreeSet::new(),
            all_keys: BTreeMap::new(),
            forecasts: 0,
            scored: 0,
            candidates: 0,
            widths: Vec::new(),
            reproduced: 0,
            unverifiable: 0,
            varying_keys: BTreeMap::new(),
            noise: NoiseReport::default(),
            calibration: CalibrationVerdict::NoGo(vec![NoGoReason::NotEnoughEvidence {
                n: 0,
                required: calibration::MIN_CALIBRATION_N,
            }]),
        }
    }
}

/// Why the four-number admission gate is not met.
#[derive(Debug, Clone, PartialEq)]
pub enum Shortfall {
    Reproduced {
        have: usize,
        need: usize,
    },
    VaryingKeys {
        have: usize,
        need: usize,
    },
    /// `runs == 0` means the gate never ran. A nonzero value means it
    /// ran and admitted every candidate.
    GateNeverRejected {
        runs: u64,
    },
    Calibration(Vec<NoGoReason>),
}

/// Whether every admission condition holds.
#[derive(Debug, Clone, PartialEq)]
pub enum AdmissionVerdict {
    Met,
    /// Never empty. A failed reading says every condition it is short on.
    NotMet(Vec<Shortfall>),
}

impl AdmissionVerdict {
    pub fn is_met(&self) -> bool {
        matches!(self, Self::Met)
    }

    pub fn shortfalls(&self) -> &[Shortfall] {
        match self {
            Self::Met => &[],
            Self::NotMet(shortfalls) => shortfalls,
        }
    }
}

impl AdmissionReading {
    pub fn varying_key_count(&self) -> usize {
        self.varying_keys
            .values()
            .filter(|values| values.len() >= 2)
            .count()
    }

    /// Report every failing condition, rather than only the first one.
    pub fn verdict(&self) -> AdmissionVerdict {
        let mut shortfalls = Vec::new();
        if self.reproduced < REQUIRED_REPRODUCED {
            shortfalls.push(Shortfall::Reproduced {
                have: self.reproduced,
                need: REQUIRED_REPRODUCED,
            });
        }
        let varying = self.varying_key_count();
        if varying < REQUIRED_VARYING_KEYS {
            shortfalls.push(Shortfall::VaryingKeys {
                have: varying,
                need: REQUIRED_VARYING_KEYS,
            });
        }
        let rejected = self.noise.transient + self.noise.volatile;
        if rejected == 0 {
            shortfalls.push(Shortfall::GateNeverRejected {
                runs: self.noise.total(),
            });
        }
        if let CalibrationVerdict::NoGo(reasons) = &self.calibration {
            shortfalls.push(Shortfall::Calibration(reasons.clone()));
        }
        if shortfalls.is_empty() {
            AdmissionVerdict::Met
        } else {
            AdmissionVerdict::NotMet(shortfalls)
        }
    }
}

/// Render a condition value as the atom text a vocabulary would use.
pub fn atom_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) if n.fract() == 0.0 => format!("{n:.0}"),
        MetricValue::Number(n) => format!("{n}"),
        MetricValue::Text(s) => s.clone(),
        MetricValue::Bool(b) => b.to_string(),
    }
}

impl Store {
    /// Read the hypothesis-search admission gate without changing the ledger.
    pub fn admission_reading(&self, tolerance: Tolerance) -> Result<AdmissionReading, TrackError> {
        let mut reading = AdmissionReading {
            noise: self.noise_report()?,
            calibration: self.calibration_report()?.verdict(tolerance),
            ..AdmissionReading::default()
        };

        for track in self.list_tracks()? {
            let mut producing = BTreeSet::new();
            for anomaly in self.anomalies_for_track(&track.id)? {
                match (anomaly.gate_outcome, anomaly.status) {
                    (
                        GateOutcome::Reproduced,
                        AnomalyStatus::Unexplained | AnomalyStatus::Explained,
                    ) => {
                        reading.reproduced += 1;
                        producing.insert(anomaly.experiment_id);
                    }
                    (
                        GateOutcome::Unverifiable,
                        AnomalyStatus::Unexplained | AnomalyStatus::Explained,
                    ) => {
                        reading.unverifiable += 1;
                    }
                    (_, AnomalyStatus::Superseded) => reading.superseded += 1,
                    _ => {}
                }
            }
            for experiment in self.experiments_for(&track.id)? {
                reading.experiments += 1;
                let produced = producing.contains(&experiment.id);
                reading.experiments_with_anomaly += usize::from(produced);

                let metrics = self.metrics_for(&experiment.id)?;
                // Select only the numeric forecast columns. In particular,
                // do not call `expectations_for`: it reads `assumptions`, a
                // model-authored column that cannot feed this judgement.
                let mut statement = self.conn.prepare(
                    "SELECT metric_key, interval_low, interval_high FROM expectations \
                     WHERE experiment_id = ? ORDER BY seq",
                )?;
                let expectations = statement.query_map(duckdb::params![experiment.id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, f64>(2)?,
                    ))
                })?;
                for expectation in expectations {
                    let (metric_key, interval_low, interval_high) = expectation?;
                    reading.forecasts += 1;
                    let observed =
                        metrics
                            .iter()
                            .find_map(|(key, value)| match (key == &metric_key, value) {
                                (true, MetricValue::Number(n)) => Some(*n),
                                _ => None,
                            });
                    if let Some(observed) = observed {
                        reading.scored += 1;
                        reading.widths.push(interval_high - interval_low);
                        if observed < interval_low || observed > interval_high {
                            reading.candidates += 1;
                        }
                    }
                }
                for condition in self.conditions_for(&experiment.id)? {
                    let key = condition.condition_key;
                    let value = atom_value(&condition.value);
                    reading
                        .all_keys
                        .entry(key.clone())
                        .or_default()
                        .insert(value.clone());
                    if produced {
                        reading.spanned_keys.insert(key.clone());
                        reading.spanned_atoms.insert((key.clone(), value.clone()));
                        reading.varying_keys.entry(key).or_default().insert(value);
                    }
                }
            }
        }
        Ok(reading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::table_counts;
    use tempfile::tempdir;

    /// A gate verdict with the numbers that do not matter here filled in.
    fn verdict(
        experiment_id: &str,
        metric_key: &str,
        outcome: GateOutcome,
    ) -> crate::rerun::GateVerdict {
        crate::rerun::GateVerdict {
            outcome,
            track_id: "t".into(),
            experiment_id: experiment_id.into(),
            metric_key: metric_key.into(),
            expectation_id: format!("e-{metric_key}"),
            expected_value: 0.8,
            interval_low: 0.7,
            interval_high: 0.9,
            observed_value: 0.5,
            surprise_sigma: 1.0,
            repeats: Vec::new(),
            divergences: Vec::new(),
        }
    }

    fn ready() -> AdmissionReading {
        AdmissionReading {
            reproduced: REQUIRED_REPRODUCED,
            varying_keys: [
                ("a".into(), ["1".into(), "2".into()].into_iter().collect()),
                ("b".into(), ["1".into(), "2".into()].into_iter().collect()),
                ("c".into(), ["1".into(), "2".into()].into_iter().collect()),
            ]
            .into_iter()
            .collect(),
            noise: NoiseReport {
                transient: 1,
                ..NoiseReport::default()
            },
            calibration: CalibrationVerdict::Go,
            ..AdmissionReading::default()
        }
    }

    #[test]
    fn all_four_conditions_hold() {
        assert!(ready().verdict().is_met());
    }

    #[test]
    fn each_shortfall_is_reported() {
        let mut reproduced = ready();
        reproduced.reproduced = 0;
        assert_eq!(reproduced.verdict().shortfalls().len(), 1);
        let mut varying = ready();
        varying.varying_keys.clear();
        assert_eq!(varying.verdict().shortfalls().len(), 1);
        let mut never = ready();
        never.noise = NoiseReport::default();
        assert_eq!(
            never.verdict().shortfalls(),
            &[Shortfall::GateNeverRejected { runs: 0 }]
        );
        let mut admitted = ready();
        admitted.noise = NoiseReport {
            reproduced: 1,
            ..NoiseReport::default()
        };
        assert_eq!(
            admitted.verdict().shortfalls(),
            &[Shortfall::GateNeverRejected { runs: 1 }]
        );
        let mut calibration = ready();
        calibration.calibration =
            CalibrationVerdict::NoGo(vec![NoGoReason::NotEnoughEvidence { n: 0, required: 50 }]);
        assert!(matches!(
            calibration.verdict().shortfalls(),
            [Shortfall::Calibration(_)]
        ));
    }

    #[test]
    fn every_failure_is_reported_and_one_value_does_not_vary() {
        let mut reading = AdmissionReading::default();
        reading
            .varying_keys
            .insert("model".into(), ["same".into()].into_iter().collect());
        assert_eq!(reading.varying_key_count(), 0);
        assert_eq!(reading.verdict().shortfalls().len(), 4);
    }

    #[test]
    fn integral_condition_reads_as_written() {
        assert_eq!(atom_value(&MetricValue::Number(8.0)), "8");
    }

    #[test]
    fn an_empty_ledger_has_every_shortfall_and_writes_nothing() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let before = table_counts(&store);
        let reading = store
            .admission_reading(Tolerance::new(0.10).unwrap())
            .unwrap();
        assert_eq!(table_counts(&store), before);
        let verdict = reading.verdict();
        let shortfalls = verdict.shortfalls();
        assert_eq!(shortfalls.len(), 4);
        assert!(matches!(shortfalls[3], Shortfall::Calibration(_)));
        assert!(matches!(reading.calibration, CalibrationVerdict::NoGo(_)));
    }

    #[test]
    fn conditions_on_reproducing_experiments_make_the_spanned_and_varying_sets() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t", "hypothesis").unwrap();
        for (index, value) in ["one", "two"].iter().enumerate() {
            let experiment = store.create_experiment("t", "p").unwrap();
            store
                .record_condition(&experiment.id, "model", &MetricValue::Text((*value).into()))
                .unwrap();
            store
                .record_condition(&experiment.id, "fixed", &MetricValue::Text("x".into()))
                .unwrap();
            store
                .record_gate_verdict(&verdict(
                    &experiment.id,
                    &format!("m{index}"),
                    GateOutcome::Reproduced,
                ))
                .unwrap();
        }
        let reading = store
            .admission_reading(Tolerance::new(0.10).unwrap())
            .unwrap();
        assert_eq!(reading.reproduced, 2);
        assert_eq!(reading.spanned_keys.len(), 2);
        assert_eq!(reading.varying_key_count(), 1);
        assert_eq!(reading.varying_keys["fixed"].len(), 1);
    }

    /// The ledger admits unverifiable rows and keeps superseded ones, and
    /// the reading counts neither toward the twelve. This is the split
    /// the 2026-09-01 run showed: 26 admitted, 15 of them reproduced.
    #[test]
    fn unverifiable_and_superseded_rows_are_reported_and_not_counted() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t", "hypothesis").unwrap();
        let experiment = store.create_experiment("t", "p").unwrap();
        store
            .record_gate_verdict(&verdict(&experiment.id, "a", GateOutcome::Unverifiable))
            .unwrap();
        let later_moot = store
            .record_gate_verdict(&verdict(&experiment.id, "b", GateOutcome::Reproduced))
            .unwrap()
            .unwrap();
        store
            .set_anomaly_status(&later_moot.id, AnomalyStatus::Superseded, None)
            .unwrap();
        // Rejected, so counted in the noise report and never a row.
        assert!(store
            .record_gate_verdict(&verdict(&experiment.id, "c", GateOutcome::Transient))
            .unwrap()
            .is_none());

        let reading = store
            .admission_reading(Tolerance::new(0.10).unwrap())
            .unwrap();
        assert_eq!(reading.reproduced, 0);
        assert_eq!(reading.unverifiable, 1);
        assert_eq!(reading.superseded, 1);
        assert_eq!(reading.experiments_with_anomaly, 0);
        assert_eq!(reading.noise.transient, 1);
        let verdict = reading.verdict();
        assert!(verdict
            .shortfalls()
            .iter()
            .any(|s| matches!(s, Shortfall::Reproduced { have: 0, need: 12 })));
        // One rejection is enough for the third condition.
        assert!(!verdict
            .shortfalls()
            .iter()
            .any(|s| matches!(s, Shortfall::GateNeverRejected { .. })));
    }
}
