//! aryabhatta step 1: the conditions an experiment was run under.

use crate::experiment::MetricValue;
use crate::track::Store;
use crate::TrackError;
use duckdb::OptionalExt;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// One input an experiment was run under. `metrics` records what came
/// out, `conditions` records what went in, and both carry a
/// `MetricValue` so the two tables read the same way.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub id: String,
    pub experiment_id: String,
    pub condition_key: String,
    pub value: MetricValue,
    pub recorded_at: i64,
    pub seq: i64,
}

/// The `value_type` string comes from `MetricValue::type_str`, the one
/// `metrics` already writes, so a condition and a metric holding the
/// same value look identical on disk and neither can drift from the
/// other.
fn value_columns(value: &MetricValue) -> (&'static str, Option<f64>, Option<&str>, Option<bool>) {
    let (number, text, boolean) = match value {
        MetricValue::Number(n) => (Some(*n), None, None),
        MetricValue::Text(s) => (None, Some(s.as_str()), None),
        MetricValue::Bool(b) => (None, None, Some(*b)),
    };
    (value.type_str(), number, text, boolean)
}

impl Store {
    /// Record one condition of `experiment_id`. Recording the same
    /// `condition_key` twice appends a second row rather than replacing
    /// the first: the table is a history of what a run was performed
    /// under, and a detector asking which variable never varied has to
    /// see every value that was ever written.
    pub fn record_condition(
        &self,
        experiment_id: &str,
        condition_key: &str,
        value: &MetricValue,
    ) -> Result<Condition, TrackError> {
        self.assert_experiment_recorded(experiment_id)?;
        let recorded_at = now_millis();
        // The counter is what keeps two conditions recorded in the same
        // millisecond off each other's primary key. It is zero-padded
        // because ids sort as text: without padding, seq 10 would sort
        // before seq 9 within that millisecond, and experiment and
        // metric ids would no longer read the same way.
        let id = format!(
            "{experiment_id}-cond-{condition_key}-{recorded_at}-{:06}",
            crate::id::next_seq()
        );
        let (value_type, num, text, boolean) = value_columns(value);
        // seq is computed inside the INSERT itself, the way metrics does
        // it: a separate SELECT per insert is O(n^2) over an
        // experiment's conditions and can race to a duplicate seq.
        // RETURNING hands back the seq the database chose, so the caller
        // does not have to read the row it just wrote.
        let seq: i64 = self.conn.query_row(
            "INSERT INTO conditions (id, experiment_id, condition_key, value_type, value_number, value_string, value_bool, recorded_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM conditions WHERE experiment_id = ? \
             RETURNING seq",
            duckdb::params![id, experiment_id, condition_key, value_type, num, text, boolean, recorded_at, experiment_id],
            |r| r.get(0),
        )?;
        Ok(Condition {
            id,
            experiment_id: experiment_id.to_string(),
            condition_key: condition_key.to_string(),
            value: value.clone(),
            recorded_at,
            seq,
        })
    }

    /// Every condition recorded for `experiment_id`, oldest first.
    /// `recorded_at` is milliseconds, so two conditions written in the
    /// same millisecond tie there and `seq` breaks the tie.
    pub fn conditions_for(&self, experiment_id: &str) -> Result<Vec<Condition>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, condition_key, value_type, value_number, value_string, value_bool, recorded_at, seq \
             FROM conditions WHERE experiment_id = ? ORDER BY recorded_at, seq",
        )?;
        let rows = stmt.query_map(duckdb::params![experiment_id], |r| {
            Ok(Condition {
                id: r.get(0)?,
                experiment_id: r.get(1)?,
                condition_key: r.get(2)?,
                value: value_from_row(r)?,
                recorded_at: r.get(7)?,
                seq: r.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Consistent with `create_experiment`'s NotFound pattern rather
    /// than a DuckDB FOREIGN KEY: fail loudly on a typo'd or stale
    /// experiment_id instead of quietly recording conditions for a run
    /// that does not exist. `experiment` has the same check, private to
    /// that module and under a different name: two inherent methods on
    /// `Store` cannot share one.
    fn assert_experiment_recorded(&self, experiment_id: &str) -> Result<(), TrackError> {
        let found: Option<()> = self
            .conn
            .query_row(
                "SELECT 1 FROM experiments WHERE id = ?",
                duckdb::params![experiment_id],
                |_| Ok(()),
            )
            .optional()?;
        if found.is_none() {
            return Err(TrackError::NotFound {
                kind: "experiment",
                id: experiment_id.to_string(),
            });
        }
        Ok(())
    }
}

fn value_from_row(r: &duckdb::Row) -> duckdb::Result<MetricValue> {
    let value_type: String = r.get(3)?;
    match value_type.as_str() {
        "number" => Ok(MetricValue::Number(r.get(4)?)),
        "string" => Ok(MetricValue::Text(r.get(5)?)),
        "bool" => Ok(MetricValue::Bool(r.get(6)?)),
        other => Err(unknown_value_type(3, other)),
    }
}

/// A `value_type` this code does not name is a read failure, not a
/// value to guess at. Deliberately not a catch-all onto text: falling
/// back that way is how a recorded number reads back as the string it
/// was never stored as, and the same defect has already been fixed once
/// in `TrackStatus::parse`.
fn unknown_value_type(column: usize, raw: &str) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(
        column,
        duckdb::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unknown condition value_type {raw:?}"),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn conditions_round_trip_every_value_type() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store
            .record_condition(&exp.id, "context_tokens", &MetricValue::Number(8192.0))
            .unwrap();
        store
            .record_condition(&exp.id, "harness", &MetricValue::Text("zorp-agent".into()))
            .unwrap();
        store
            .record_condition(&exp.id, "search_enabled", &MetricValue::Bool(false))
            .unwrap();

        let found = store.conditions_for(&exp.id).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].condition_key, "context_tokens");
        assert_eq!(found[0].value, MetricValue::Number(8192.0));
        assert_eq!(found[1].condition_key, "harness");
        assert_eq!(found[1].value, MetricValue::Text("zorp-agent".into()));
        assert_eq!(found[2].condition_key, "search_enabled");
        assert_eq!(found[2].value, MetricValue::Bool(false));
        assert_eq!(found[0].experiment_id, exp.id);
    }

    #[test]
    fn condition_order_is_by_insertion_not_millisecond_timestamp() {
        // recorded_at is a millisecond timestamp, and a tight loop puts
        // many conditions inside one millisecond, where it gives no
        // relative order at all. seq is what makes read-back
        // deterministic, and a detector asking whether a key ever
        // changed value needs that order to mean something.
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        for i in 0..20 {
            store
                .record_condition(&exp.id, &format!("c{i}"), &MetricValue::Number(i as f64))
                .unwrap();
        }

        let found = store.conditions_for(&exp.id).unwrap();
        let keys: Vec<String> = found.iter().map(|c| c.condition_key.clone()).collect();
        let expected: Vec<String> = (0..20).map(|i| format!("c{i}")).collect();
        assert_eq!(keys, expected);
        let seqs: Vec<i64> = found.iter().map(|c| c.seq).collect();
        assert_eq!(seqs, (0..20).collect::<Vec<i64>>());
    }

    #[test]
    fn conditions_for_returns_only_that_experiments_conditions() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let a = store.create_experiment("t1", "t1-prereg").unwrap();
        let b = store.create_experiment("t1", "t1-prereg").unwrap();

        store
            .record_condition(&a.id, "harness", &MetricValue::Text("zorp-agent".into()))
            .unwrap();
        store
            .record_condition(&a.id, "context_tokens", &MetricValue::Number(8192.0))
            .unwrap();
        store
            .record_condition(&b.id, "harness", &MetricValue::Text("bare".into()))
            .unwrap();

        let a_conditions = store.conditions_for(&a.id).unwrap();
        let a_keys: Vec<&str> = a_conditions
            .iter()
            .map(|c| c.condition_key.as_str())
            .collect();
        assert_eq!(a_keys, vec!["harness", "context_tokens"]);

        let b_conditions = store.conditions_for(&b.id).unwrap();
        assert_eq!(b_conditions.len(), 1);
        assert_eq!(b_conditions[0].value, MetricValue::Text("bare".into()));
        // seq counts within an experiment, so b's first condition is 0
        // even though a already wrote two rows to the table.
        assert_eq!(b_conditions[0].seq, 0);
    }

    /// Recording a key twice keeps both rows. The table is a history of
    /// what a run was performed under, and an upsert would erase the
    /// only evidence that the value changed, which is exactly what a
    /// detector asking "has this ever varied" needs to see.
    #[test]
    fn the_same_condition_key_recorded_twice_keeps_both_rows() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        let first = store
            .record_condition(&exp.id, "context_tokens", &MetricValue::Number(8192.0))
            .unwrap();
        let second = store
            .record_condition(&exp.id, "context_tokens", &MetricValue::Number(16384.0))
            .unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(first.seq, 0);
        assert_eq!(second.seq, 1);

        let found = store.conditions_for(&exp.id).unwrap();
        assert_eq!(found.len(), 2, "both values must survive");
        assert_eq!(found[0].value, MetricValue::Number(8192.0));
        assert_eq!(found[1].value, MetricValue::Number(16384.0));
        assert_eq!(found[0].condition_key, found[1].condition_key);
    }

    #[test]
    fn record_condition_on_missing_experiment_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        let err = store
            .record_condition("nope", "harness", &MetricValue::Text("zorp-agent".into()))
            .unwrap_err();

        assert!(matches!(err, TrackError::NotFound { kind: "experiment", id } if id == "nope"));
        assert!(
            store.conditions_for("nope").unwrap().is_empty(),
            "a refused condition must not leave a row behind"
        );
    }

    /// A stored value_type this code does not name must not read back as
    /// text. `metrics` decodes anything unrecognized as its string
    /// column, which turns a recorded number into a string that was
    /// never stored, and the same silent-default defect has already been
    /// fixed once in `TrackStatus::parse`.
    #[test]
    fn an_unrecognized_stored_value_type_is_a_read_failure_not_text() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        store
            .record_condition(&exp.id, "context_tokens", &MetricValue::Number(8192.0))
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE conditions SET value_type = 'float64' WHERE experiment_id = ?",
                duckdb::params![exp.id],
            )
            .unwrap();

        let err = store
            .conditions_for(&exp.id)
            .expect_err("should refuse to read");
        let msg = err.to_string();
        assert!(
            msg.contains("float64"),
            "error should name the offending type, got: {msg}"
        );
    }
}
