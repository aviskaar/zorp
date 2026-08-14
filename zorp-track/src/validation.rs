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
    /// Record a validation for `track_id`. Retrying validate on the same
    /// question the same day (e.g. after a prior run failed before
    /// completing, or a deliberate re-check) is expected to succeed, not
    /// collide on a primary key: the id includes the current timestamp so
    /// each call inserts a new row, and `get_validation` returns the most
    /// recent one for a track.
    pub fn record_validation(
        &self,
        track_id: &str,
        redundancy_score: f64,
        redundancy_citations: &[Citation],
        feasibility_score: f64,
        feasibility_citations: &[Citation],
        verdict: &str,
    ) -> Result<Validation, TrackError> {
        let created_at = now_millis();
        // next_seq only keeps the primary key unique when two inserts
        // land in the same millisecond. Ordering uses the seq column
        // below, which is derived from the table itself.
        let id = format!(
            "{track_id}-validation-{created_at}-{}",
            crate::id::next_seq()
        );
        let redundancy_json = citations_to_json(redundancy_citations);
        let feasibility_json = citations_to_json(feasibility_citations);
        self.conn.execute(
            "INSERT INTO validations \
             (id, track_id, redundancy_score, redundancy_citations, feasibility_score, feasibility_citations, verdict, created_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM validations WHERE track_id = ?",
            duckdb::params![
                id,
                track_id,
                redundancy_score,
                redundancy_json,
                feasibility_score,
                feasibility_json,
                verdict,
                created_at,
                track_id
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

    /// The most recent validation recorded for `track_id`. A track may have
    /// more than one row (validate can be retried), so this returns the
    /// latest by insert order, not an arbitrary one. `created_at` alone is
    /// not enough: it is milliseconds, and two retries in the same
    /// millisecond tie, so `seq` decides. Rows written before `seq`
    /// existed have NULL there and sort last within their millisecond.
    pub fn get_validation(&self, track_id: &str) -> Result<Validation, TrackError> {
        self.conn
            .query_row(
                "SELECT id, track_id, redundancy_score, redundancy_citations, feasibility_score, feasibility_citations, verdict, created_at \
                 FROM validations WHERE track_id = ? ORDER BY created_at DESC, seq DESC NULLS LAST LIMIT 1",
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
        Citation {
            text: text.to_string(),
            source: source.to_string(),
        }
    }

    #[test]
    fn record_and_get_round_trip() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();

        let red = vec![citation("no prior benchmark found", "search result 1")];
        let feas = vec![citation(
            "a benchmark harness already exists",
            "repo README",
        )];
        let recorded = store
            .record_validation("t1", 20.0, &red, 85.0, &feas, "worth investigating")
            .unwrap();

        let fetched = store.get_validation("t1").unwrap();
        assert_eq!(recorded, fetched);
        assert_eq!(fetched.redundancy_citations, red);
        assert_eq!(fetched.feasibility_citations, feas);
    }

    #[test]
    fn retrying_validate_on_the_same_track_succeeds_and_returns_the_latest() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();

        let first_red = vec![citation("first pass, nothing found", "search result 1")];
        store
            .record_validation("t1", 10.0, &first_red, 0.0, &[], "inconclusive, retrying")
            .unwrap();

        let second_red = vec![citation(
            "second pass, found a prior benchmark",
            "search result 2",
        )];
        let second = store
            .record_validation(
                "t1",
                40.0,
                &second_red,
                90.0,
                &[citation("tooling exists", "readme")],
                "worth investigating",
            )
            .unwrap();

        let fetched = store.get_validation("t1").unwrap();
        assert_eq!(fetched, second);
        assert_eq!(fetched.redundancy_score, 40.0);
        assert_eq!(fetched.redundancy_citations, second_red);
    }

    #[test]
    fn get_missing_validation_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let err = store.get_validation("t1").unwrap_err();
        assert!(matches!(
            err,
            TrackError::NotFound {
                kind: "validation",
                ..
            }
        ));
    }

    #[test]
    fn empty_citations_round_trip_as_empty() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        store
            .record_validation("t1", 0.0, &[], 0.0, &[], "no evidence found")
            .unwrap();
        let fetched = store.get_validation("t1").unwrap();
        assert!(fetched.redundancy_citations.is_empty());
        assert!(fetched.feasibility_citations.is_empty());
    }

    #[test]
    fn the_last_validation_written_wins_even_in_one_millisecond() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        // No sleeps: these land in the same millisecond, where created_at
        // gives no ordering at all.
        for i in 0..12 {
            store
                .record_validation("t1", i as f64, &[], 0.0, &[], &format!("pass {i}"))
                .unwrap();
        }
        let latest = store.get_validation("t1").unwrap();
        assert_eq!(latest.verdict, "pass 11");
        assert_eq!(latest.redundancy_score, 11.0);
    }
}
