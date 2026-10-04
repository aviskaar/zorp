//! Where the local-weights measurements will live: perplexity, peak host
//! memory, and parameter counts. See #259.
//!
//! These belong to a runtime, not to an item, and perplexity is neither
//! correct nor incorrect, so they do not fit `bench_results`. They go in a
//! sibling table in the same database, keyed by the same session and the
//! runtime they describe.
//!
//! The rule from the hosted half holds here and is enforced twice. A
//! measurement that did not happen is not a zero. In Rust, an unevaluable
//! row carries a reason and has nowhere to put a number. In SQLite, a
//! `CHECK` refuses a measured row with no value and an unevaluable row with
//! one, so a writer that skips the type still cannot store a zero for a
//! runtime that failed to load. A NaN reaches SQLite as NULL, so a measured
//! NaN is refused by the same check.
//!
//! Nothing writes to this table yet. The runner that starts a local runtime
//! and measures it is the next piece of #259.

use crate::BoxErr;
use rusqlite::Connection;

pub fn init_schema(conn: &Connection) -> Result<(), BoxErr> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bench_local_results (
            id INTEGER PRIMARY KEY,
            session TEXT NOT NULL,
            runtime_id TEXT NOT NULL,
            metric TEXT NOT NULL CHECK (metric IN (
                'perplexity', 'peak_host_memory', 'device_memory',
                'total_parameters', 'active_parameters'
            )),
            outcome TEXT NOT NULL CHECK (outcome IN ('measured', 'unevaluable')),
            value REAL,
            unit TEXT,
            method TEXT,
            reason TEXT,
            CHECK ((outcome = 'measured') = (value IS NOT NULL)),
            CHECK ((outcome = 'unevaluable') = (reason IS NOT NULL)),
            CHECK (outcome = 'unevaluable' OR (unit IS NOT NULL AND method IS NOT NULL)),
            UNIQUE (session, runtime_id, metric)
        )",
        (),
    )?;
    Ok(())
}

/// What a local runtime can be measured for. A closed set, checked by the
/// table as well as by this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Token-level perplexity over a held-out corpus.
    Perplexity,
    /// Peak resident memory of a runtime process this machine started.
    PeakHostMemory,
    /// Accelerator memory. Host memory cannot see it.
    DeviceMemory,
    TotalParameters,
    /// Parameters used per token, derived from the architecture.
    ActiveParameters,
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Perplexity => "perplexity",
            Metric::PeakHostMemory => "peak_host_memory",
            Metric::DeviceMemory => "device_memory",
            Metric::TotalParameters => "total_parameters",
            Metric::ActiveParameters => "active_parameters",
        }
    }

    fn parse(text: &str) -> Option<Metric> {
        Some(match text {
            "perplexity" => Metric::Perplexity,
            "peak_host_memory" => Metric::PeakHostMemory,
            "device_memory" => Metric::DeviceMemory,
            "total_parameters" => Metric::TotalParameters,
            "active_parameters" => Metric::ActiveParameters,
            _ => return None,
        })
    }
}

/// How one measurement ended. There is no variant that holds a number and
/// a reason, and none that holds neither.
#[derive(Debug, Clone, PartialEq)]
pub enum Measurement {
    /// A value, its unit, and how it was obtained (for example
    /// `proc_pid_rusage lifetime_max_phys_footprint`, or `derived` for a
    /// count read from the architecture), because the same column measured
    /// two ways is two different numbers.
    Measured {
        value: f64,
        unit: String,
        method: String,
    },
    /// Why there is no value: the runtime did not load, exposed no
    /// log-probabilities, was not our process, and so on.
    Unevaluable { reason: String },
}

pub struct LocalRecord<'a> {
    pub session: &'a str,
    pub runtime_id: &'a str,
    pub metric: Metric,
    pub measurement: Measurement,
}

pub fn insert(conn: &Connection, r: &LocalRecord) -> Result<(), BoxErr> {
    let (outcome, value, unit, method, reason) = match &r.measurement {
        Measurement::Measured {
            value,
            unit,
            method,
        } => (
            "measured",
            Some(*value),
            Some(unit.as_str()),
            Some(method.as_str()),
            None,
        ),
        Measurement::Unevaluable { reason } => {
            ("unevaluable", None, None, None, Some(reason.as_str()))
        }
    };
    conn.execute(
        "INSERT INTO bench_local_results (session, runtime_id, metric, outcome, value, unit, method, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            r.session,
            r.runtime_id,
            r.metric.as_str(),
            outcome,
            value,
            unit,
            method,
            reason
        ],
    )?;
    Ok(())
}

/// One stored measurement, read back.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalRow {
    pub runtime_id: String,
    pub metric: Metric,
    pub measurement: Measurement,
}

/// Every measurement for one session, by runtime and then metric.
pub fn read(conn: &Connection, session: &str) -> Result<Vec<LocalRow>, BoxErr> {
    let mut statement = conn.prepare(
        "SELECT runtime_id, metric, outcome, value, unit, method, reason
         FROM bench_local_results WHERE session = ?1
         ORDER BY runtime_id, metric",
    )?;
    let rows = statement.query_map([session], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<f64>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (runtime_id, metric, outcome, value, unit, method, reason) = row?;
        let metric = Metric::parse(&metric)
            .ok_or_else(|| BoxErr::from(format!("unknown metric {metric:?}")))?;
        let measurement = match (outcome.as_str(), value, unit, method, reason) {
            ("measured", Some(value), Some(unit), Some(method), None) => Measurement::Measured {
                value,
                unit,
                method,
            },
            ("unevaluable", None, _, _, Some(reason)) => Measurement::Unevaluable { reason },
            _ => {
                return Err(format!(
                    "{runtime_id} {}: a row the table should have refused",
                    metric.as_str()
                )
                .into())
            }
        };
        out.push(LocalRow {
            runtime_id,
            metric,
            measurement,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn raw(conn: &Connection, outcome: &str, value: Option<f64>, reason: Option<&str>) -> bool {
        conn.execute(
            "INSERT INTO bench_local_results (session, runtime_id, metric, outcome, value, unit, method, reason)
             VALUES ('s', 'r', 'perplexity', ?1, ?2, 'ppl', 'llama-perplexity', ?3)",
            rusqlite::params![outcome, value, reason],
        )
        .is_ok()
    }

    #[test]
    fn a_measured_row_with_no_value_is_refused_by_the_table() {
        assert!(!raw(&db(), "measured", None, None));
    }

    #[test]
    fn an_unevaluable_row_cannot_carry_a_zero() {
        assert!(!raw(&db(), "unevaluable", Some(0.0), Some("did not load")));
    }

    #[test]
    fn an_unevaluable_row_must_say_why() {
        assert!(!raw(&db(), "unevaluable", None, None));
    }

    #[test]
    fn a_measured_row_must_say_how_it_was_measured() {
        let conn = db();
        let stored = conn
            .execute(
                "INSERT INTO bench_local_results (session, runtime_id, metric, outcome, value, unit, method)
                 VALUES ('s', 'r', 'peak_host_memory', 'measured', 1024, 'bytes', NULL)",
                (),
            )
            .is_ok();
        assert!(!stored);
    }

    #[test]
    fn a_metric_outside_the_set_is_refused() {
        let conn = db();
        let stored = conn
            .execute(
                "INSERT INTO bench_local_results (session, runtime_id, metric, outcome, reason)
                 VALUES ('s', 'r', 'vibes', 'unevaluable', 'x')",
                (),
            )
            .is_ok();
        assert!(!stored);
    }

    #[test]
    fn a_measured_nan_is_not_stored() {
        let conn = db();
        let nan = LocalRecord {
            session: "s",
            runtime_id: "r",
            metric: Metric::Perplexity,
            measurement: Measurement::Measured {
                value: f64::NAN,
                unit: "ppl".into(),
                method: "llama-perplexity".into(),
            },
        };
        assert!(insert(&conn, &nan).is_err());
        assert!(read(&conn, "s").unwrap().is_empty());
    }

    #[test]
    fn measured_and_unevaluable_rows_read_back_as_written() {
        let conn = db();
        let measured = Measurement::Measured {
            value: 7.25,
            unit: "ppl".into(),
            method: "llama-perplexity".into(),
        };
        let unevaluable = Measurement::Unevaluable {
            reason: "not_our_process".into(),
        };
        for (runtime_id, metric, measurement) in [
            ("llama", Metric::Perplexity, measured.clone()),
            ("ollama", Metric::PeakHostMemory, unevaluable.clone()),
        ] {
            insert(
                &conn,
                &LocalRecord {
                    session: "s",
                    runtime_id,
                    metric,
                    measurement,
                },
            )
            .unwrap();
        }
        let rows = read(&conn, "s").unwrap();
        assert_eq!(
            rows,
            vec![
                LocalRow {
                    runtime_id: "llama".into(),
                    metric: Metric::Perplexity,
                    measurement: measured,
                },
                LocalRow {
                    runtime_id: "ollama".into(),
                    metric: Metric::PeakHostMemory,
                    measurement: unevaluable,
                },
            ]
        );
    }

    #[test]
    fn one_runtime_has_one_value_per_metric() {
        let conn = db();
        let record = LocalRecord {
            session: "s",
            runtime_id: "r",
            metric: Metric::TotalParameters,
            measurement: Measurement::Measured {
                value: 7.0e9,
                unit: "parameters".into(),
                method: "derived".into(),
            },
        };
        insert(&conn, &record).unwrap();
        assert!(insert(&conn, &record).is_err());
    }
}
