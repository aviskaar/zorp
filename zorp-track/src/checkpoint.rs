use crate::track::Store;
use crate::TrackError;
use duckdb::OptionalExt;
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Asks a human a yes/no question at a research checkpoint. Mirrors
/// zorp-agent's `Approver` trait, at track granularity instead of
/// per-tool-call.
pub trait Decider: Send + Sync {
    fn decide(&self, prompt: &str) -> bool;
}

pub struct TerminalDecider;
impl Decider for TerminalDecider {
    fn decide(&self, prompt: &str) -> bool {
        eprint!("{prompt} [y/N] ");
        if io::stderr().flush().is_err() {
            return false;
        }
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return false;
        }
        matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    }
}

#[derive(Clone)]
pub enum CheckpointMode {
    Interactive(Arc<dyn Decider>),
    AutoApprove,
}

impl CheckpointMode {
    /// Unlike zorp-agent's per-tool-call `ApprovalMode`, there is no
    /// `NonInteractive` variant here: a research checkpoint has no safe
    /// default to fall back to when nobody can answer it, so that case
    /// is a hard error instead of a silent skip.
    pub fn terminal(auto_approve: bool) -> Result<Self, TrackError> {
        if auto_approve {
            Ok(CheckpointMode::AutoApprove)
        } else if io::stdin().is_terminal() {
            Ok(CheckpointMode::Interactive(Arc::new(TerminalDecider)))
        } else {
            Err(TrackError::CheckpointBlocked {
                kind: "terminal".to_string(),
            })
        }
    }

    fn decide(&self, prompt: &str) -> bool {
        match self {
            CheckpointMode::Interactive(d) => d.decide(prompt),
            CheckpointMode::AutoApprove => true,
        }
    }
}

impl Store {
    /// Run a checkpoint's decision and persist the outcome. `kind`
    /// identifies which capability this checkpoint belongs to (e.g.
    /// "validate", "experiment"); left as a plain string rather than a
    /// fixed enum since capabilities beyond the four already named may
    /// add their own.
    pub fn record_checkpoint(
        &self,
        track_id: &str,
        kind: &str,
        mode: &CheckpointMode,
        prompt: &str,
    ) -> Result<bool, TrackError> {
        let approved = mode.decide(prompt);
        let id = format!(
            "{track_id}-{kind}-{}-{}",
            now_millis(),
            crate::id::next_seq()
        );
        let now = now_millis();
        self.conn.execute(
            "INSERT INTO checkpoints (id, track_id, kind, status, prompt_shown, decision_notes, created_at, resolved_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM checkpoints WHERE track_id = ?",
            duckdb::params![
                id,
                track_id,
                kind,
                if approved { "approved" } else { "rejected" },
                prompt,
                Option::<String>::None,
                now,
                now,
                track_id
            ],
        )?;
        Ok(approved)
    }

    /// Record a non-optional kill and set the track's status to Killed.
    /// Unlike `record_checkpoint`, no `CheckpointMode` is consulted:
    /// this is for enforcement (a pre-registered kill threshold breach),
    /// which AutoApprove must not be able to skip. The reason is
    /// persisted as a checkpoints row of the given `kind` with status
    /// "enforced-kill" so the run record shows what killed the track.
    pub fn record_enforced_kill(
        &self,
        track_id: &str,
        kind: &str,
        reason: &str,
    ) -> Result<(), TrackError> {
        let id = format!(
            "{track_id}-{kind}-{}-{}",
            now_millis(),
            crate::id::next_seq()
        );
        let now = now_millis();
        self.conn.execute(
            "INSERT INTO checkpoints (id, track_id, kind, status, prompt_shown, decision_notes, created_at, resolved_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM checkpoints WHERE track_id = ?",
            duckdb::params![
                id,
                track_id,
                kind,
                "enforced-kill",
                reason,
                Option::<String>::None,
                now,
                now,
                track_id
            ],
        )?;
        self.set_track_status(track_id, crate::track::TrackStatus::Killed)?;
        Ok(())
    }

    /// Read back the `resolved_at` of the most recent checkpoint of
    /// `kind` for `track_id`, or `None` if no such checkpoint exists yet.
    /// Used by `co_write::run`'s mtime-warning heuristic, not for any
    /// integrity enforcement.
    pub fn latest_checkpoint_time(
        &self,
        track_id: &str,
        kind: &str,
    ) -> Result<Option<i64>, TrackError> {
        let row: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT resolved_at FROM checkpoints WHERE track_id = ? AND kind = ? ORDER BY created_at DESC, seq DESC NULLS LAST LIMIT 1",
                duckdb::params![track_id, kind],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    struct Stub {
        answer: bool,
        calls: AtomicUsize,
    }
    impl Decider for Stub {
        fn decide(&self, _prompt: &str) -> bool {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answer
        }
    }

    #[test]
    fn auto_approve_always_decides_true() {
        assert!(CheckpointMode::AutoApprove.decide("proceed?"));
    }

    #[test]
    fn interactive_mode_defers_to_the_decider() {
        let approve = CheckpointMode::Interactive(Arc::new(Stub {
            answer: true,
            calls: AtomicUsize::new(0),
        }));
        assert!(approve.decide("proceed?"));
        let reject = CheckpointMode::Interactive(Arc::new(Stub {
            answer: false,
            calls: AtomicUsize::new(0),
        }));
        assert!(!reject.decide("proceed?"));
    }

    #[test]
    fn terminal_without_auto_approve_and_no_tty_errors() {
        // Test processes normally have no interactive stdin.
        let result = CheckpointMode::terminal(false);
        assert!(matches!(result, Err(TrackError::CheckpointBlocked { .. })));
    }

    #[test]
    fn terminal_with_auto_approve_never_checks_stdin() {
        let result = CheckpointMode::terminal(true);
        assert!(matches!(result, Ok(CheckpointMode::AutoApprove)));
    }

    #[test]
    fn record_checkpoint_persists_the_decision() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::Interactive(Arc::new(Stub {
            answer: true,
            calls: AtomicUsize::new(0),
        }));

        let approved = store
            .record_checkpoint("t1", "validate", &mode, "is this novel?")
            .unwrap();
        assert!(approved);

        let (status, prompt): (String, String) = store
            .conn
            .query_row(
                "SELECT status, prompt_shown FROM checkpoints WHERE track_id = ? AND kind = ?",
                duckdb::params!["t1", "validate"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "approved");
        assert_eq!(prompt, "is this novel?");
    }

    #[test]
    fn record_enforced_kill_persists_the_reason_and_kills_the_track() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        store
            .record_enforced_kill(
                "t1",
                "investigate-threshold",
                "latency_ms = 150 went above threshold 100",
            )
            .unwrap();

        assert_eq!(
            store.get_track("t1").unwrap().status,
            crate::track::TrackStatus::Killed
        );
        let (status, reason): (String, String) = store
            .conn
            .query_row(
                "SELECT status, prompt_shown FROM checkpoints WHERE track_id = ? AND kind = ?",
                duckdb::params!["t1", "investigate-threshold"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "enforced-kill");
        assert!(reason.contains("latency_ms"));
        assert!(reason.contains("100"));
    }

    #[test]
    fn checkpoints_recorded_in_the_same_millisecond_do_not_collide() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::AutoApprove;
        for _ in 0..50 {
            store
                .record_checkpoint("t1", "validate", &mode, "proceed?")
                .unwrap();
        }
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM checkpoints WHERE track_id = ?",
                duckdb::params!["t1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 50);
    }

    #[test]
    fn latest_checkpoint_time_returns_none_when_absent() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        assert_eq!(
            store.latest_checkpoint_time("t1", "co-write").unwrap(),
            None
        );
    }

    #[test]
    fn latest_checkpoint_time_only_matches_the_given_kind() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::AutoApprove;
        store
            .record_checkpoint("t1", "validate", &mode, "novel?")
            .unwrap();

        assert_eq!(
            store.latest_checkpoint_time("t1", "co-write").unwrap(),
            None
        );

        // Positive case: a co-write row alongside the validate row must be
        // found and must not be confused with the validate row's timestamp.
        std::thread::sleep(std::time::Duration::from_millis(5));
        store
            .record_checkpoint("t1", "co-write", &mode, "draft ready?")
            .unwrap();

        let (validate_resolved_at,): (i64,) = store
            .conn
            .query_row(
                "SELECT resolved_at FROM checkpoints WHERE track_id = ? AND kind = ?",
                duckdb::params!["t1", "validate"],
                |r| Ok((r.get(0)?,)),
            )
            .unwrap();
        let (co_write_resolved_at,): (i64,) = store
            .conn
            .query_row(
                "SELECT resolved_at FROM checkpoints WHERE track_id = ? AND kind = ?",
                duckdb::params!["t1", "co-write"],
                |r| Ok((r.get(0)?,)),
            )
            .unwrap();

        let time = store.latest_checkpoint_time("t1", "co-write").unwrap();
        assert_eq!(time, Some(co_write_resolved_at));
        assert_ne!(time, Some(validate_resolved_at));
    }

    #[test]
    fn latest_checkpoint_time_returns_the_most_recent_matching_row() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let mode = CheckpointMode::AutoApprove;
        store
            .record_checkpoint("t1", "co-write", &mode, "draft 1 ready?")
            .unwrap();
        store
            .record_checkpoint("t1", "co-write", &mode, "draft 2 ready?")
            .unwrap();

        // Deliberately no sleep between the two: AutoApprove does no I/O
        // while deciding, so back-to-back checkpoints land in the same
        // millisecond and created_at alone cannot order them. seq does.
        let (latest_prompt, latest_resolved_at): (String, i64) = store
            .conn
            .query_row(
                "SELECT prompt_shown, resolved_at FROM checkpoints WHERE track_id = ? AND kind = ? \
                 ORDER BY created_at DESC, seq DESC NULLS LAST LIMIT 1",
                duckdb::params!["t1", "co-write"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(latest_prompt, "draft 2 ready?");

        let time = store.latest_checkpoint_time("t1", "co-write").unwrap();
        assert_eq!(time, Some(latest_resolved_at));
    }

    #[test]
    fn each_checkpoint_on_a_track_gets_the_next_sequence_number() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        store.create_track("t2", "other").unwrap();
        let mode = CheckpointMode::AutoApprove;
        for i in 0..12 {
            store
                .record_checkpoint("t1", "co-write", &mode, &format!("draft {i}?"))
                .unwrap();
        }
        store
            .record_enforced_kill("t1", "investigate", "threshold breached")
            .unwrap();
        store
            .record_checkpoint("t2", "validate", &mode, "unrelated")
            .unwrap();

        // The sequence must actually advance. Ordering by created_at
        // cannot be trusted here, so this asserts the column itself.
        let mut stmt = store
            .conn
            .prepare("SELECT seq FROM checkpoints WHERE track_id = ? ORDER BY seq")
            .unwrap();
        let seqs: Vec<i64> = stmt
            .query_map(duckdb::params!["t1"], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(seqs, (0..13).collect::<Vec<i64>>());

        // Sequences are per track, so a second track starts over.
        let t2_seq: i64 = store
            .conn
            .query_row(
                "SELECT seq FROM checkpoints WHERE track_id = ?",
                duckdb::params!["t2"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t2_seq, 0);
    }

    #[test]
    fn the_latest_checkpoint_is_decided_by_seq_when_the_timestamp_ties() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        // Pin both rows to the same created_at so the timestamp cannot
        // break the tie and only the seq ordering can. Written oldest
        // first, so a query that ignores seq returns the wrong one.
        for (id, seq, resolved) in [("cp-old", 0_i64, 111_i64), ("cp-new", 1, 222)] {
            store
                .conn
                .execute(
                    "INSERT INTO checkpoints (id, track_id, kind, status, prompt_shown, decision_notes, created_at, resolved_at, seq) \
                     VALUES (?, 't1', 'co-write', 'approved', ?, NULL, 5000, ?, ?)",
                    duckdb::params![id, id, resolved, seq],
                )
                .unwrap();
        }

        assert_eq!(
            store.latest_checkpoint_time("t1", "co-write").unwrap(),
            Some(222)
        );
    }
}
