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

/// One thing a critique pass found wrong with a draft. `kind` is a plain
/// string for the same reason `checkpoint`'s `kind` is: the set of
/// checks belongs to whoever runs the pass, not to storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CritiqueFinding {
    pub kind: String,
    pub claim: String,
    pub detail: String,
}

/// One audited draft. Round 0 is the draft as co-write left it; every
/// later round is a revision the pass produced. `accepted` says whether
/// that revision became the current draft, so the row sequence shows
/// both what was criticised and what actually changed.
#[derive(Debug, Clone, PartialEq)]
pub struct CritiqueRound {
    pub id: String,
    pub track_id: String,
    pub round: i64,
    pub draft_hash: String,
    pub findings: Vec<CritiqueFinding>,
    pub accepted: bool,
    pub created_at: i64,
}

fn findings_to_json(findings: &[CritiqueFinding]) -> String {
    serde_json::to_string(findings).unwrap_or_else(|_| "[]".to_string())
}

fn findings_from_json(raw: &str) -> Vec<CritiqueFinding> {
    serde_json::from_str(raw).unwrap_or_default()
}

impl Store {
    /// Record one audited draft. The draft itself is not stored, only its
    /// hash: the draft lives on disk as `draft.md`, and copying it into
    /// the run record would give two sources of truth for the same text.
    pub fn record_critique_round(
        &self,
        track_id: &str,
        round: i64,
        draft: &str,
        findings: &[CritiqueFinding],
        accepted: bool,
    ) -> Result<CritiqueRound, TrackError> {
        let created_at = now_millis();
        // next_seq only keeps the primary key unique when two inserts
        // land in the same millisecond. Ordering uses the seq column
        // below, which is derived from the table itself.
        let id = format!("{track_id}-critique-{created_at}-{}", crate::id::next_seq());
        let draft_hash = crate::prereg::sha256_hex(draft.as_bytes());
        let findings_json = findings_to_json(findings);
        self.conn.execute(
            "INSERT INTO critiques \
             (id, track_id, round, draft_hash, findings, accepted, created_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM critiques WHERE track_id = ?",
            duckdb::params![
                id,
                track_id,
                round,
                draft_hash,
                findings_json,
                accepted,
                created_at,
                track_id
            ],
        )?;
        Ok(CritiqueRound {
            id,
            track_id: track_id.to_string(),
            round,
            draft_hash,
            findings: findings.to_vec(),
            accepted,
            created_at,
        })
    }

    /// Every critique round recorded for `track_id`, in the order they
    /// were written. Ordered by `seq`, not `created_at`: a whole pass
    /// can run inside one millisecond, and rounds read back out of order
    /// would misreport which revision replaced which.
    pub fn critiques_for(&self, track_id: &str) -> Result<Vec<CritiqueRound>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, round, draft_hash, findings, accepted, created_at \
             FROM critiques WHERE track_id = ? ORDER BY created_at, seq NULLS FIRST",
        )?;
        let rows = stmt.query_map(duckdb::params![track_id], |r| {
            let findings_raw: String = r.get(4)?;
            Ok(CritiqueRound {
                id: r.get(0)?,
                track_id: r.get(1)?,
                round: r.get(2)?,
                draft_hash: r.get(3)?,
                findings: findings_from_json(&findings_raw),
                accepted: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn finding(kind: &str, claim: &str) -> CritiqueFinding {
        CritiqueFinding {
            kind: kind.to_string(),
            claim: claim.to_string(),
            detail: "detail".to_string(),
        }
    }

    #[test]
    fn a_recorded_round_reads_back_with_its_findings() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "does caching help").unwrap();

        let findings = vec![
            finding("number-not-in-record", "latency fell 58%"),
            finding("uncited-claim", "users noticed"),
        ];
        store
            .record_critique_round("t1", 0, "# Draft\n", &findings, true)
            .unwrap();

        let rounds = store.critiques_for("t1").unwrap();
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].round, 0);
        assert_eq!(rounds[0].findings, findings);
        assert!(rounds[0].accepted);
    }

    #[test]
    fn the_draft_hash_changes_with_the_draft_and_not_otherwise() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        let a = store
            .record_critique_round("t1", 0, "same text", &[], true)
            .unwrap();
        let b = store
            .record_critique_round("t1", 1, "same text", &[], false)
            .unwrap();
        let c = store
            .record_critique_round("t1", 2, "different text", &[], true)
            .unwrap();

        assert_eq!(a.draft_hash, b.draft_hash);
        assert_ne!(a.draft_hash, c.draft_hash);
    }

    #[test]
    fn rounds_read_back_in_order_even_within_one_millisecond() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        // No sleeps: these land in the same millisecond, where created_at
        // gives no ordering at all. Same discipline as validations.
        for round in 0..12 {
            store
                .record_critique_round("t1", round, &format!("draft {round}"), &[], true)
                .unwrap();
        }

        let rounds: Vec<i64> = store
            .critiques_for("t1")
            .unwrap()
            .into_iter()
            .map(|r| r.round)
            .collect();
        assert_eq!(rounds, (0..12).collect::<Vec<i64>>());
    }

    #[test]
    fn one_tracks_rounds_do_not_leak_into_another() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        store.create_track("t2", "other").unwrap();

        store
            .record_critique_round("t1", 0, "draft one", &[finding("uncited-claim", "a")], true)
            .unwrap();
        store
            .record_critique_round("t2", 0, "draft two", &[], true)
            .unwrap();

        assert_eq!(store.critiques_for("t1").unwrap().len(), 1);
        assert_eq!(store.critiques_for("t2").unwrap().len(), 1);
        assert_eq!(store.critiques_for("t1").unwrap()[0].findings.len(), 1);
        assert!(store.critiques_for("t2").unwrap()[0].findings.is_empty());
    }

    #[test]
    fn a_clean_round_records_no_findings_rather_than_no_row() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        store
            .record_critique_round("t1", 0, "# Draft\n", &[], true)
            .unwrap();

        // "The pass ran and found nothing" and "the pass never ran" are
        // different facts, and the record has to be able to tell them
        // apart.
        let rounds = store.critiques_for("t1").unwrap();
        assert_eq!(rounds.len(), 1);
        assert!(rounds[0].findings.is_empty());
    }
}
