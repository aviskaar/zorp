//! Reports the four-number hypothesis-search admission reading.
//!
//! The 2026-09-02 gate needs reproduced admissions, condition keys that
//! varied, a re-run rejection, and a calibration Go. Crossing it remains a
//! person's decision, recorded in `docs/DECISIONS.md`. This prints a reading
//! and never acts on one. In particular it does not build `ExperimentRow`:
//! a reader that quietly grew that adapter would be the gate crossing itself.
//!
//! Opens the store directly rather than through `Project::open`, which writes
//! while rebuilding and checking a project. A reading must not write.

use std::path::PathBuf;

use zorp_track::admission::{
    AdmissionVerdict, Shortfall, REQUIRED_REPRODUCED, REQUIRED_VARYING_KEYS,
};
use zorp_track::calibration::{CalibrationVerdict, NoGoReason, Tolerance};
use zorp_track::Store;

fn print_reasons(reasons: &[NoGoReason]) {
    for reason in reasons {
        println!("    {reason:?}");
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
    // 0.10 is the standing tolerance. See docs/DECISIONS.md, 2026-08-24.
    let reading = store.admission_reading(Tolerance::new(0.10)?)?;

    println!("ledger: {}", db.display());
    println!(
        "  experiments            {} ({} produced a reproduced anomaly)",
        reading.experiments, reading.experiments_with_anomaly
    );
    println!(
        "  reproduced admissions  {} (need {})",
        reading.reproduced, REQUIRED_REPRODUCED
    );
    println!(
        "  unverifiable admitted  {} (reported, not counted)",
        reading.unverifiable
    );
    println!(
        "  superseded             {} (reported, not counted)",
        reading.superseded
    );
    println!(
        "  spanned condition keys {}: {}",
        reading.spanned_keys.len(),
        reading
            .spanned_keys
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  spanned atoms          {}", reading.spanned_atoms.len());
    for (key, value) in &reading.spanned_atoms {
        println!("    {key}={value}");
    }
    println!(
        "  varying condition keys {} (need {})",
        reading.varying_key_count(),
        REQUIRED_VARYING_KEYS
    );
    for (key, values) in &reading.varying_keys {
        if values.len() >= 2 {
            println!(
                "    {key}: {}",
                values.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }
    for key in &reading.spanned_keys {
        if reading
            .varying_keys
            .get(key)
            .is_some_and(|values| values.len() == 1)
        {
            let value = reading.varying_keys[key].iter().next().unwrap();
            println!("    {key}: did not vary ({value})");
        }
    }
    println!("\npipeline in front of the gate:");
    println!("  forecasts recorded     {}", reading.forecasts);
    println!("  of those, scored       {}", reading.scored);
    println!(
        "  outside the interval   {} (candidates, not anomalies)",
        reading.candidates
    );
    if !reading.widths.is_empty() {
        let mut widths = reading.widths.clone();
        widths.sort_by(f64::total_cmp);
        let mean = widths.iter().sum::<f64>() / widths.len() as f64;
        println!(
            "  stated interval width  mean {mean:.1}, min {:.1}, median {:.1}, max {:.1}",
            widths[0],
            widths[widths.len() / 2],
            widths[widths.len() - 1]
        );
    }
    println!("\ngate runs:");
    match reading.noise.noise_rate() {
        Some(rate) => println!(
            "  {} runs, {} admitted, noise rate {rate:.3}",
            reading.noise.total(),
            reading.noise.admitted()
        ),
        None => println!("  the gate has never run, so there is no noise rate"),
    }
    println!(
        "  reproduced {} transient {} volatile {} unverifiable {}",
        reading.noise.reproduced,
        reading.noise.transient,
        reading.noise.volatile,
        reading.noise.unverifiable
    );
    println!(
        "  rejections             {}",
        reading.noise.transient + reading.noise.volatile
    );
    println!("\ncalibration:");
    match &reading.calibration {
        CalibrationVerdict::Go => println!("  Go"),
        CalibrationVerdict::NoGo(reasons) => {
            println!("  NoGo");
            print_reasons(reasons);
        }
    }
    println!("\nconditions recorded anywhere, and how many values each took:");
    for (key, values) in &reading.all_keys {
        println!("  {key}: {} distinct", values.len());
        for value in values {
            println!("    {value}");
        }
    }
    match reading.verdict() {
        AdmissionVerdict::Met => {
            println!("\nGATE MET on the four numbers: reproduced admissions, varying condition");
            println!("keys, a re-run rejection, and a calibration Go. Crossing it is still a");
            println!("person's decision, recorded in docs/DECISIONS.md, and it wants a");
            println!("permutation null first: the 2026-08-28 entry says silence on unstructured");
            println!("data is a property to demand of a real run, not one the synthetic tests");
            println!("promise.");
        }
        AdmissionVerdict::NotMet(shortfalls) => {
            println!("\nGATE NOT MET.");
            for shortfall in shortfalls {
                match shortfall {
                    Shortfall::Reproduced { have, need } => {
                        println!("  reproduced admissions: {have} (need {need})")
                    }
                    Shortfall::VaryingKeys { have, need } => {
                        println!("  varying condition keys: {have} (need {need})")
                    }
                    Shortfall::GateNeverRejected { runs: 0 } => println!("  re-run gate never ran"),
                    Shortfall::GateNeverRejected { runs } => {
                        println!("  re-run gate admitted all {runs} candidates")
                    }
                    Shortfall::Calibration(reasons) => {
                        println!("  calibration is NoGo:");
                        print_reasons(&reasons);
                    }
                }
            }
        }
    }
    Ok(())
}
