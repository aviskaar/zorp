use crate::track::Store;
use crate::TrackError;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub id: String,
    pub track_id: String,
    pub redundancy_score: f64,
    pub redundancy_citations: Vec<Citation>,
    pub feasibility_score: f64,
    pub feasibility_citations: Vec<Citation>,
    pub verdict: String,
    pub created_at: i64,
}

fn citations_to_json(citations: &[Citation]) -> String {
    serde_json::to_string(citations).unwrap_or_else(|_| "[]".to_string())
}

fn citations_from_json(raw: &str) -> Vec<Citation> {
    serde_json::from_str(raw).unwrap_or_default()
}

impl Store {
    pub fn record_validation(
        &self,
        track_id: &str,
        redundancy_score: f64,
        redundancy_citations: &[Citation],
        feasibility_score: f64,
        feasibility_citations: &[Citation],
        verdict: &str,
    ) -> Result<Validation, TrackError> {
        let id = format!("{track_id}-validation");
        let created_at = now_millis();
        let redundancy_json = citations_to_json(redundancy_citations);
        let feasibility_json = citations_to_json(feasibility_citations);
        self.conn.execute(
            "INSERT INTO validations \
             (id, track_id, redundancy_score, redundancy_citations, feasibility_score, feasibility_citations, verdict, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                id,
                track_id,
                redundancy_score,
                redundancy_json,
                feasibility_score,
                feasibility_json,
                verdict,
                created_at
            ],
        )?;
        Ok(Validation {
            id,
            track_id: track_id.to_string(),
            redundancy_score,
            redundancy_citations: redundancy_citations.to_vec(),
            feasibility_score,
            feasibility_citations: feasibility_citations.to_vec(),
            verdict: verdict.to_string(),
            created_at,
        })
    }

    pub fn get_validation(&self, track_id: &str) -> Result<Validation, TrackError> {
        self.conn
            .query_row(
                "SELECT id, track_id, redundancy_score, redundancy_citations, feasibility_score, feasibility_citations, verdict, created_at \
                 FROM validations WHERE track_id = ?",
                duckdb::params![track_id],
                |r| {
                    let redundancy_raw: String = r.get(3)?;
                    let feasibility_raw: String = r.get(5)?;
                    Ok(Validation {
                        id: r.get(0)?,
                        track_id: r.get(1)?,
                        redundancy_score: r.get(2)?,
                        redundancy_citations: citations_from_json(&redundancy_raw),
                        feasibility_score: r.get(4)?,
                        feasibility_citations: citations_from_json(&feasibility_raw),
                        verdict: r.get(6)?,
                        created_at: r.get(7)?,
                    })
                },
            )
            .map_err(|e| match e {
                duckdb::Error::QueryReturnedNoRows => TrackError::NotFound {
                    kind: "validation",
                    id: track_id.to_string(),
                },
                other => TrackError::from(other),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn citation(text: &str, source: &str) -> Citation {
        Citation { text: text.to_string(), source: source.to_string() }
    }

    #[test]
    fn record_and_get_round_trip() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();

        let red = vec![citation("no prior benchmark found", "search result 1")];
        let feas = vec![citation("a benchmark harness already exists", "repo README")];
        let recorded = store
            .record_validation("t1", 20.0, &red, 85.0, &feas, "worth investigating")
            .unwrap();

        let fetched = store.get_validation("t1").unwrap();
        assert_eq!(recorded, fetched);
        assert_eq!(fetched.redundancy_citations, red);
        assert_eq!(fetched.feasibility_citations, feas);
    }

    #[test]
    fn get_missing_validation_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let err = store.get_validation("t1").unwrap_err();
        assert!(matches!(err, TrackError::NotFound { kind: "validation", .. }));
    }

    #[test]
    fn empty_citations_round_trip_as_empty() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        store.record_validation("t1", 0.0, &[], 0.0, &[], "no evidence found").unwrap();
        let fetched = store.get_validation("t1").unwrap();
        assert!(fetched.redundancy_citations.is_empty());
        assert!(fetched.feasibility_citations.is_empty());
    }
}
