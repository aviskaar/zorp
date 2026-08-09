use crate::track::Store;
use crate::TrackError;
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
            Err(TrackError::CheckpointBlocked { kind: "terminal".to_string() })
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
        let id = format!("{track_id}-{kind}-{}", now_millis());
        let now = now_millis();
        self.conn.execute(
            "INSERT INTO checkpoints (id, track_id, kind, status, prompt_shown, decision_notes, created_at, resolved_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                id,
                track_id,
                kind,
                if approved { "approved" } else { "rejected" },
                prompt,
                Option::<String>::None,
                now,
                now
            ],
        )?;
        Ok(approved)
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
        let approve = CheckpointMode::Interactive(Arc::new(Stub { answer: true, calls: AtomicUsize::new(0) }));
        assert!(approve.decide("proceed?"));
        let reject = CheckpointMode::Interactive(Arc::new(Stub { answer: false, calls: AtomicUsize::new(0) }));
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
        let mode = CheckpointMode::Interactive(Arc::new(Stub { answer: true, calls: AtomicUsize::new(0) }));

        let approved = store.record_checkpoint("t1", "validate", &mode, "is this novel?").unwrap();
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
}
