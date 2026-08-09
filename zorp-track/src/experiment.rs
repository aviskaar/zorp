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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Killed,
}

impl ExperimentStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ExperimentStatus::Planned => "planned",
            ExperimentStatus::Running => "running",
            ExperimentStatus::Completed => "completed",
            ExperimentStatus::Failed => "failed",
            ExperimentStatus::Killed => "killed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "running" => ExperimentStatus::Running,
            "completed" => ExperimentStatus::Completed,
            "failed" => ExperimentStatus::Failed,
            "killed" => ExperimentStatus::Killed,
            _ => ExperimentStatus::Planned,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Experiment {
    pub id: String,
    pub track_id: String,
    pub prereg_id: String,
    pub status: ExperimentStatus,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    Number(f64),
    Text(String),
    Bool(bool),
}

impl MetricValue {
    fn type_str(&self) -> &'static str {
        match self {
            MetricValue::Number(_) => "number",
            MetricValue::Text(_) => "string",
            MetricValue::Bool(_) => "bool",
        }
    }
}

impl Store {
    pub fn create_experiment(&self, track_id: &str, prereg_id: &str) -> Result<Experiment, TrackError> {
        // Consistent with get_track's NotFound pattern rather than a
        // DuckDB FOREIGN KEY constraint: fail loudly on a typo'd or
        // stale track_id instead of silently creating an orphan row.
        self.get_track(track_id)?;
        let id = format!("{track_id}-exp-{}", now_millis());
        self.conn.execute(
            "INSERT INTO experiments (id, track_id, prereg_id, status, started_at, completed_at) VALUES (?, ?, ?, ?, NULL, NULL)",
            duckdb::params![id, track_id, prereg_id, ExperimentStatus::Planned.as_str()],
        )?;
        Ok(Experiment {
            id,
            track_id: track_id.to_string(),
            prereg_id: prereg_id.to_string(),
            status: ExperimentStatus::Planned,
            started_at: None,
            completed_at: None,
        })
    }

    pub fn set_experiment_status(&self, id: &str, status: ExperimentStatus) -> Result<(), TrackError> {
        let now = now_millis();
        let sql = match status {
            ExperimentStatus::Running => "UPDATE experiments SET status = ?, started_at = ? WHERE id = ?",
            ExperimentStatus::Completed | ExperimentStatus::Failed | ExperimentStatus::Killed => {
                "UPDATE experiments SET status = ?, completed_at = ? WHERE id = ?"
            }
            ExperimentStatus::Planned => "UPDATE experiments SET status = ? WHERE id = ?",
        };
        let updated = if matches!(status, ExperimentStatus::Planned) {
            self.conn.execute(sql, duckdb::params![status.as_str(), id])?
        } else {
            self.conn.execute(sql, duckdb::params![status.as_str(), now, id])?
        };
        if updated == 0 {
            return Err(TrackError::NotFound { kind: "experiment", id: id.to_string() });
        }
        Ok(())
    }

    pub fn record_metric(&self, experiment_id: &str, key: &str, value: MetricValue) -> Result<(), TrackError> {
        self.assert_experiment_exists(experiment_id)?;
        let seq: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metrics WHERE experiment_id = ?",
            duckdb::params![experiment_id],
            |r| r.get(0),
        )?;
        let metric_id = format!("{experiment_id}-{key}-{}", now_millis());
        let (num, text, boolean) = match &value {
            MetricValue::Number(n) => (Some(*n), None, None),
            MetricValue::Text(s) => (None, Some(s.clone()), None),
            MetricValue::Bool(b) => (None, None, Some(*b)),
        };
        self.conn.execute(
            "INSERT INTO metrics (id, experiment_id, metric_key, value_type, value_number, value_string, value_bool, recorded_at, seq) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![metric_id, experiment_id, key, value.type_str(), num, text, boolean, now_millis(), seq],
        )?;
        Ok(())
    }

    /// Ordering within an experiment's metrics is by insertion order
    /// (`seq`), not `recorded_at`: two metrics recorded in the same
    /// millisecond would otherwise have no defined relative order.
    pub fn metrics_for(&self, experiment_id: &str) -> Result<Vec<(String, MetricValue)>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT metric_key, value_type, value_number, value_string, value_bool FROM metrics WHERE experiment_id = ? ORDER BY seq",
        )?;
        let rows = stmt.query_map(duckdb::params![experiment_id], |r| {
            let key: String = r.get(0)?;
            let value_type: String = r.get(1)?;
            let value = match value_type.as_str() {
                "number" => MetricValue::Number(r.get(2)?),
                "bool" => MetricValue::Bool(r.get(4)?),
                _ => MetricValue::Text(r.get(3)?),
            };
            Ok((key, value))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Every experiment recorded for `track_id`, oldest first. Ids embed
    /// a millisecond timestamp, so ordering by id is insertion order for
    /// any two experiments that were not created in the same millisecond;
    /// `started_at` is NULL until an experiment goes Running, so it can't
    /// be the sort key.
    pub fn experiments_for(&self, track_id: &str) -> Result<Vec<Experiment>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, prereg_id, status, started_at, completed_at FROM experiments WHERE track_id = ? ORDER BY id",
        )?;
        let rows = stmt.query_map(duckdb::params![track_id], |r| {
            let status: String = r.get(3)?;
            Ok(Experiment {
                id: r.get(0)?,
                track_id: r.get(1)?,
                prereg_id: r.get(2)?,
                status: ExperimentStatus::from_str(&status),
                started_at: r.get(4)?,
                completed_at: r.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn assert_experiment_exists(&self, experiment_id: &str) -> Result<(), TrackError> {
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM experiments WHERE id = ?",
                duckdb::params![experiment_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(TrackError::NotFound {
                kind: "experiment",
                id: experiment_id.to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_experiment_starts_planned() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        assert_eq!(exp.status, ExperimentStatus::Planned);
        assert_eq!(exp.started_at, None);
    }

    #[test]
    fn status_transition_to_running_sets_started_at_not_completed_at() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store.set_experiment_status(&exp.id, ExperimentStatus::Running).unwrap();

        let (started_at, completed_at): (Option<i64>, Option<i64>) = store
            .conn
            .query_row(
                "SELECT started_at, completed_at FROM experiments WHERE id = ?",
                duckdb::params![exp.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(started_at.is_some());
        assert_eq!(completed_at, None);
    }

    #[test]
    fn status_transition_to_completed_sets_completed_at() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store.set_experiment_status(&exp.id, ExperimentStatus::Completed).unwrap();

        let completed_at: Option<i64> = store
            .conn
            .query_row(
                "SELECT completed_at FROM experiments WHERE id = ?",
                duckdb::params![exp.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(completed_at.is_some());
    }

    #[test]
    fn record_and_read_back_typed_metrics() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store.record_metric(&exp.id, "accuracy", MetricValue::Number(0.87)).unwrap();
        store.record_metric(&exp.id, "notes", MetricValue::Text("looked promising".into())).unwrap();
        store.record_metric(&exp.id, "converged", MetricValue::Bool(true)).unwrap();

        let metrics = store.metrics_for(&exp.id).unwrap();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0], ("accuracy".to_string(), MetricValue::Number(0.87)));
        assert_eq!(metrics[1], ("notes".to_string(), MetricValue::Text("looked promising".into())));
        assert_eq!(metrics[2], ("converged".to_string(), MetricValue::Bool(true)));
    }

    #[test]
    fn metric_order_is_by_insertion_not_millisecond_timestamp() {
        // record_metric's recorded_at is a millisecond timestamp, which
        // gives no defined relative order for metrics recorded within
        // the same millisecond. `seq` is what actually guarantees
        // insertion order is preserved on read-back, regardless of how
        // close together in time the inserts happen.
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        for i in 0..20 {
            store
                .record_metric(&exp.id, &format!("m{i}"), MetricValue::Number(i as f64))
                .unwrap();
        }

        let metrics = store.metrics_for(&exp.id).unwrap();
        let keys: Vec<String> = metrics.into_iter().map(|(k, _)| k).collect();
        let expected: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn experiments_for_returns_only_that_tracks_experiments_with_current_status() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        store.create_track("t2", "other").unwrap();
        let a = store.create_experiment("t1", "t1-prereg").unwrap();
        let b = store.create_experiment("t2", "t2-prereg").unwrap();
        store.set_experiment_status(&a.id, ExperimentStatus::Completed).unwrap();

        let found = store.experiments_for("t1").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, a.id);
        assert_eq!(found[0].track_id, "t1");
        assert_eq!(found[0].prereg_id, "t1-prereg");
        assert_eq!(found[0].status, ExperimentStatus::Completed);
        assert!(found[0].completed_at.is_some());
        assert_ne!(found[0].id, b.id);

        assert!(store.experiments_for("nope").unwrap().is_empty());
    }

    #[test]
    fn set_status_on_missing_experiment_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let err = store.set_experiment_status("nope", ExperimentStatus::Running).unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "experiment", .. }));
    }

    #[test]
    fn create_experiment_on_missing_track_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        let err = store.create_experiment("nope", "nope-prereg").unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "track", id } if id == "nope"));
    }

    #[test]
    fn record_metric_on_missing_experiment_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let err = store
            .record_metric("nope", "accuracy", MetricValue::Number(1.0))
            .unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "experiment", id } if id == "nope"));
    }
}
