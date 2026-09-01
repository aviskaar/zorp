//! Reports whether an anomaly ledger has reached the hypothesis-search
//! admission gate, and prints the numbers the gate is stated in.
//!
//! The gate is 12 admitted anomalies spanning at least 3 distinct
//! condition keys (`docs/DECISIONS.md`, 2026-08-28). Crossing it is a
//! person's decision, recorded in that log when it happens, so this
//! prints a reading and never acts on one. In particular it does not
//! build the `ExperimentRow` adapter: the same entry says no code reads
//! the real ledger into hypothesis search, and a reader that quietly
//! grew one would be the gate crossing itself.
//!
//! An example rather than a binary because aryabhatta ships no CLI
//! command on purpose. Nothing here is installed or released.
//!
//!     cargo run -p zorp-track --example gate_status -- <project-root>
//!
//! Opens the store directly instead of going through `Project::open`,
//! which creates directories, rebuilds from prereg files and verifies
//! integrity. A reading should not write to the thing it is reading.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use zorp_track::anomalies::AnomalyStatus;
use zorp_track::experiment::MetricValue;
use zorp_track::Store;

/// Minimum admitted anomalies, from the 2026-08-28 gate.
const REQUIRED_ANOMALIES: usize = 12;
/// Minimum distinct condition keys those anomalies must span.
const REQUIRED_KEYS: usize = 3;

/// What the ledger holds, in the terms the gate is stated in.
#[derive(Debug, Default, PartialEq)]
struct Reading {
    /// Admitted anomalies that still count: unexplained or explained.
    admitted: usize,
    /// Admitted rows later marked moot. Counted, never silently dropped.
    superseded: usize,
    /// Distinct condition keys on the experiments that produced those
    /// anomalies. This is the gate's "spanning" number.
    spanned_keys: BTreeSet<String>,
    /// Distinct condition atoms on those same experiments. Not part of
    /// the gate, but it is what the search would actually get a
    /// vocabulary from, and a key that never varies contributes one atom
    /// and separates nothing.
    spanned_atoms: BTreeSet<(String, String)>,
    /// Condition keys anywhere in the store, for context when the
    /// spanned set is thin.
    all_keys: BTreeMap<String, BTreeSet<String>>,
    /// Experiments recorded, and how many produced an admitted anomaly.
    experiments: usize,
    experiments_with_anomaly: usize,
    /// The pipeline in front of the gate. An anomaly needs a forecast
    /// and then an outcome for the same metric key, so counting the
    /// stages separately says which one the ledger is actually short of.
    forecasts: usize,
    /// Forecasts whose metric key later got an outcome. Only these can
    /// be scored at all.
    scored: usize,
    /// Scored forecasts whose outcome fell outside the stated interval.
    /// These are candidates and nothing more: a candidate becomes an
    /// anomaly only by surviving the re-run gate.
    candidates: usize,
    /// Stated interval widths of the scored forecasts. Zero candidates
    /// means either a well calibrated model or intervals too wide to
    /// exclude anything, and the two want opposite responses, so the
    /// widths are reported rather than left to be guessed at.
    widths: Vec<f64>,
}

impl Reading {
    /// The gate, as a predicate. Both halves must hold.
    fn passes(&self) -> bool {
        self.admitted >= REQUIRED_ANOMALIES && self.spanned_keys.len() >= REQUIRED_KEYS
    }
}

/// Render a condition value as the atom text a vocabulary would use.
///
/// Integral numbers lose the `.0` so `max_steps=8` reads as it was
/// written on the command line rather than as `max_steps=8.0`.
fn atom_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) if n.fract() == 0.0 => format!("{n:.0}"),
        MetricValue::Number(n) => format!("{n}"),
        MetricValue::Text(s) => s.clone(),
        MetricValue::Bool(b) => b.to_string(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".to_string()));
    let db = root.join(".zorp").join("zorp.duckdb");
    if !db.exists() {
        eprintln!("no ledger at {}", db.display());
        std::process::exit(2);
    }
    let store = Store::open(&db)?;

    let mut reading = Reading::default();

    for track in store.list_tracks()? {
        // Which experiments in this track produced an admitted anomaly.
        // A single experiment can produce several, on different metrics,
        // so the experiment set is deduplicated while the anomaly count
        // is not: the gate counts anomalies.
        let mut producing = BTreeSet::new();
        for anomaly in store.anomalies_for_track(&track.id)? {
            match anomaly.status {
                AnomalyStatus::Unexplained | AnomalyStatus::Explained => {
                    reading.admitted += 1;
                    producing.insert(anomaly.experiment_id);
                }
                AnomalyStatus::Superseded => reading.superseded += 1,
            }
        }

        for experiment in store.experiments_for(&track.id)? {
            reading.experiments += 1;
            let produced = producing.contains(&experiment.id);
            if produced {
                reading.experiments_with_anomaly += 1;
            }

            // Forecast, then outcome, then candidate. `assumptions` is
            // model-authored and is not read here, for the reason
            // integrity rule 5 gives: nothing that feeds a judgement may
            // read a column the agent wrote.
            let metrics = store.metrics_for(&experiment.id)?;
            for expectation in store.expectations_for(&experiment.id)? {
                reading.forecasts += 1;
                let observed = metrics.iter().find_map(|(key, value)| {
                    match (key == &expectation.metric_key, value) {
                        (true, MetricValue::Number(n)) => Some(*n),
                        _ => None,
                    }
                });
                if let Some(observed) = observed {
                    reading.scored += 1;
                    reading
                        .widths
                        .push(expectation.interval_high - expectation.interval_low);
                    if observed < expectation.interval_low || observed > expectation.interval_high {
                        reading.candidates += 1;
                    }
                }
            }

            for condition in store.conditions_for(&experiment.id)? {
                let key = condition.condition_key;
                let value = atom_value(&condition.value);
                reading
                    .all_keys
                    .entry(key.clone())
                    .or_default()
                    .insert(value.clone());
                if produced {
                    reading.spanned_keys.insert(key.clone());
                    reading.spanned_atoms.insert((key, value));
                }
            }
        }
    }

    let noise = store.noise_report()?;

    println!("ledger: {}", db.display());
    println!(
        "  experiments            {} ({} produced an admitted anomaly)",
        reading.experiments, reading.experiments_with_anomaly
    );
    println!(
        "  admitted anomalies     {} (need {})",
        reading.admitted, REQUIRED_ANOMALIES
    );
    if reading.superseded > 0 {
        println!("  superseded             {}", reading.superseded);
    }
    println!(
        "  spanned condition keys {} (need {}){}",
        reading.spanned_keys.len(),
        REQUIRED_KEYS,
        if reading.spanned_keys.is_empty() {
            String::new()
        } else {
            format!(
                ": {}",
                reading
                    .spanned_keys
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    println!("  spanned atoms          {}", reading.spanned_atoms.len());
    for (key, value) in &reading.spanned_atoms {
        println!("    {key}={value}");
    }

    println!("\npipeline in front of the gate:");
    println!("  forecasts recorded     {}", reading.forecasts);
    println!("  of those, scored       {}", reading.scored);
    println!(
        "  outside the interval   {} (candidates, not anomalies)",
        reading.candidates
    );
    if !reading.widths.is_empty() {
        let mut sorted = reading.widths.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        println!(
            "  stated interval width  mean {mean:.1}, min {:.1}, median {:.1}, max {:.1}",
            sorted[0],
            sorted[sorted.len() / 2],
            sorted[sorted.len() - 1]
        );
    }
    if reading.scored < zorp_track::calibration::MIN_CALIBRATION_N {
        println!(
            "  too few scored forecasts to judge calibration ({} < {})",
            reading.scored,
            zorp_track::calibration::MIN_CALIBRATION_N
        );
    }

    println!("\ngate runs:");
    match noise.noise_rate() {
        // A gate that never ran has not shown the environment is quiet.
        // It has shown nothing, which is not a noise rate of zero.
        None => println!("  the gate has never run, so there is no noise rate"),
        Some(rate) => println!(
            "  {} runs, {} admitted, noise rate {rate:.3}",
            noise.total(),
            noise.admitted()
        ),
    }
    println!(
        "  reproduced {} transient {} volatile {} unverifiable {}",
        noise.reproduced, noise.transient, noise.volatile, noise.unverifiable
    );

    println!("\nconditions recorded anywhere, and how many values each took:");
    for (key, values) in &reading.all_keys {
        // A key with one value cannot separate anything, whatever the
        // row count is, so the distinct-value count is the useful number
        // rather than how often the key appears.
        println!("  {key}: {} distinct", values.len());
        for value in values {
            println!("    {value}");
        }
    }

    if reading.passes() {
        println!("\nGATE MET on the numbers. Crossing it is still a person's decision,");
        println!("recorded in docs/DECISIONS.md, and it wants a permutation null first:");
        println!("the 2026-08-28 entry says silence on unstructured data is a property");
        println!("to demand of a real run, not one the synthetic tests promise.");
    } else {
        println!("\nGATE NOT MET.");
        if reading.admitted < REQUIRED_ANOMALIES {
            println!(
                "  short {} admitted anomalies",
                REQUIRED_ANOMALIES - reading.admitted
            );
        }
        if reading.spanned_keys.len() < REQUIRED_KEYS {
            println!(
                "  short {} distinct condition keys",
                REQUIRED_KEYS - reading.spanned_keys.len()
            );
        }
        // Distinguish "the runs were unremarkable" from "nothing ever
        // asked". They look the same in an empty ledger and they are not
        // the same problem: the first wants more runs, the second wants
        // a caller.
        if noise.total() == 0 {
            println!(
                "\n  the gate has never run, so no candidate was ever judged. \
                 {} forecast(s) landed outside their interval and are waiting on \
                 a caller, not on more data.",
                reading.candidates
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(admitted: usize, keys: &[&str]) -> Reading {
        Reading {
            admitted,
            spanned_keys: keys.iter().map(|k| k.to_string()).collect(),
            ..Reading::default()
        }
    }

    #[test]
    fn both_halves_of_the_gate_are_required() {
        assert!(reading(12, &["model", "checkpoint_mode", "max_steps"]).passes());
        // Enough anomalies, too few keys. This is the shape the ledger
        // was stuck in before max_steps was recorded (2026-08-31).
        assert!(!reading(40, &["model", "checkpoint_mode"]).passes());
        // Enough keys, too few anomalies.
        assert!(!reading(11, &["model", "checkpoint_mode", "max_steps"]).passes());
    }

    #[test]
    fn superseded_rows_do_not_count_toward_the_gate() {
        let mut r = reading(11, &["model", "checkpoint_mode", "max_steps"]);
        r.superseded = 5;
        assert!(
            !r.passes(),
            "superseded rows must not make up the shortfall"
        );
    }

    #[test]
    fn an_integral_condition_reads_as_it_was_written() {
        assert_eq!(atom_value(&MetricValue::Number(8.0)), "8");
        assert_eq!(atom_value(&MetricValue::Number(0.5)), "0.5");
        assert_eq!(atom_value(&MetricValue::Text("ollama".into())), "ollama");
    }
}
