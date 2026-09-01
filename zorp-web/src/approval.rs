use crate::event::{Event, EventKind};
use crate::state::SettingsHandle;
use crate::tool_safety::{self, Verdict};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zorp_agent::{Approver, ToolCall};

/// How long an approval waits for a human before denying.
///
/// Denying on timeout matches what the CLI already does when it cannot ask,
/// and it means a browser that was closed mid-run leaves the agent stopped
/// rather than parked forever.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// The longest tool name an auto-approval notice will print.
///
/// A tool name arrives from the model, so it is untrusted text on its way to
/// the transcript. The browser puts it on the page through `textContent` and
/// nothing else, so this is not the thing standing between a name and an
/// injection; it is what stops a run from being buried under one line of
/// megabyte long garbage per tool call. `zorp-agent`'s terminal prompt caps
/// its own names for the same reason.
const TOOL_NAME_MAX_BYTES: usize = 40;

/// Parks the agent thread until the browser answers.
///
/// `confirm` is called on the agent's blocking thread, so waiting here is
/// correct: it is exactly the terminal prompt's behavior with the prompt
/// moved into a browser.
///
/// Unless `auto_approve` is set and `tool_safety::check` clears the call, in
/// which case the human has already answered. See its field comment; the
/// short version is that this is the CLI's `ApprovalMode::AutoApprove`, made
/// revocable in the middle of a run, and now checked rather than blind.
pub struct WebApprover {
    events: Sender<Event>,
    /// Shared with the renderer so approval requests interleave correctly
    /// with activity in one ordered stream.
    seq: Arc<Mutex<u64>>,
    pending: Arc<Mutex<Option<Sender<bool>>>>,
    inbox: Arc<Mutex<Option<Receiver<bool>>>>,
    timeout: Duration,
    /// The session's standing answer, owned by `SessionState` and shared with
    /// every turn's approver so it survives from one turn to the next and can
    /// be revoked while a turn is running.
    ///
    /// This is a standing "yes" from the human, not a wider policy. It is read
    /// only at the point the browser would have been asked, which is *after*
    /// `Policy::decide` has already had its say in `zorp-agent`, so it can
    /// turn an `Ask` into an allow and can do nothing whatsoever with a
    /// `Deny`. A denylisted command never reaches this code.
    auto_approve: Arc<AtomicBool>,
    /// What the standing yes is checked against before it fires: the
    /// session's own model, asked fresh and with no tools of its own
    /// whether the call it is about to wave through looks safe. See
    /// `tool_safety`.
    settings: SettingsHandle,
    /// Test-only seam so the safety verdict can be fixed without a socket.
    /// `None` means the real `tool_safety::check`.
    #[cfg(test)]
    checker: Option<Arc<dyn Fn(&str, &str) -> Verdict + Send + Sync>>,
}

impl WebApprover {
    pub fn new(
        events: Sender<Event>,
        seq: Arc<Mutex<u64>>,
        auto_approve: Arc<AtomicBool>,
        settings: SettingsHandle,
    ) -> Self {
        WebApprover {
            events,
            seq,
            pending: Arc::new(Mutex::new(None)),
            inbox: Arc::new(Mutex::new(None)),
            timeout: APPROVAL_TIMEOUT,
            auto_approve,
            settings,
            #[cfg(test)]
            checker: None,
        }
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Fix the safety verdict every call gets, instead of asking a model.
    #[cfg(test)]
    fn with_checker(mut self, verdict: Verdict) -> Self {
        self.checker = Some(Arc::new(move |_, _| verdict));
        self
    }

    fn safety_check(&self, tool: &str, arguments: &str) -> Verdict {
        #[cfg(test)]
        if let Some(checker) = &self.checker {
            return checker(tool, arguments);
        }
        tool_safety::check(&self.settings, tool, arguments)
    }

    /// Resolve whatever the agent is currently waiting on. Returns false when
    /// nothing is pending, which is what a stale click from the browser looks
    /// like.
    pub fn resolve(&self, allow: bool) -> bool {
        let taken = self.pending.lock().unwrap().take();
        match taken {
            Some(tx) => tx.send(allow).is_ok(),
            None => false,
        }
    }

    /// Put one line in the transcript saying what the standing yes just let
    /// through, and allow the call only if that line was recorded.
    ///
    /// Refusing when the line cannot be sent looks pedantic and is the point.
    /// A mode this consequential earns its keep by leaving a trail, and the
    /// only way to lose the trail is for the session's own event drain to be
    /// gone, which is not a state in which a machine changing tool should
    /// quietly run. It matches what the interactive path already does with a
    /// dead stream: no way to tell the human, no.
    fn record_auto_approval(&self, tool: &str) -> bool {
        let event = Event {
            seq: self.next_seq(),
            kind: EventKind::Notice {
                text: format!("auto-approved {}", tool_label(tool)),
            },
        };
        self.events.send(event).is_ok()
    }

    /// Say why a call that could have run unattended is asking instead. Best
    /// effort: unlike `record_auto_approval`, a failure here does not stop
    /// the call, because the normal ask flow that follows has its own
    /// dead-stream handling and this is only ever the reason, not the
    /// decision.
    fn record_flagged(&self, tool: &str) {
        let event = Event {
            seq: self.next_seq(),
            kind: EventKind::Notice {
                text: format!(
                    "auto-approve paused for {}: the safety check did not clear it, asking you",
                    tool_label(tool)
                ),
            },
        };
        let _ = self.events.send(event);
    }

    fn next_seq(&self) -> u64 {
        let mut guard = self.seq.lock().unwrap();
        let seq = *guard;
        *guard += 1;
        seq
    }
}

impl Approver for WebApprover {
    fn confirm(&self, call: &ToolCall) -> bool {
        if self.auto_approve.load(Ordering::SeqCst) {
            let arguments = call.arguments.to_string();
            if self.safety_check(&call.name, &arguments) == Verdict::Safe {
                return self.record_auto_approval(&call.name);
            }
            // Not a clear SAFE: fall through and ask a human, exactly as if
            // there were no standing yes for this one call. The denylist
            // already ran in `Policy::decide`; this is the second pair of
            // eyes on what it let through, not a second denylist, so it
            // never refuses on its own.
            self.record_flagged(&call.name);
        }

        let (tx, rx) = std::sync::mpsc::channel();
        *self.pending.lock().unwrap() = Some(tx);
        *self.inbox.lock().unwrap() = Some(rx);

        let id = format!("approval-{}", self.next_seq());
        let event = Event {
            seq: self.next_seq(),
            kind: EventKind::ApprovalRequest {
                id,
                tool: call.name.clone(),
                arguments: call.arguments.to_string(),
            },
        };
        if self.events.send(event).is_err() {
            // Nobody is listening, so nobody can approve. Deny rather than
            // wait out the timeout.
            return false;
        }

        let rx = self.inbox.lock().unwrap().take();
        match rx {
            Some(rx) => rx.recv_timeout(self.timeout).unwrap_or(false),
            None => false,
        }
    }
}

/// A tool name, made safe to put in one line of a transcript: control
/// characters flattened to spaces and the whole thing cut to a bound, on a
/// character boundary so the result stays valid UTF-8.
fn tool_label(name: &str) -> String {
    let flattened: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flattened.len() <= TOOL_NAME_MAX_BYTES {
        return flattened;
    }
    let marker = "…";
    let mut end = TOOL_NAME_MAX_BYTES.saturating_sub(marker.len());
    while end > 0 && !flattened.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{marker}", &flattened[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsState;
    use serde_json::json;

    fn call() -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: "write_file".into(),
            arguments: json!({"path": "a.txt", "content": "hi"}),
        }
    }

    fn settings() -> SettingsHandle {
        Arc::new(Mutex::new(SettingsState::default()))
    }

    fn asking(events: Sender<Event>) -> WebApprover {
        WebApprover::new(
            events,
            Arc::new(Mutex::new(0)),
            Arc::new(AtomicBool::new(false)),
            settings(),
        )
    }

    /// An auto-approver whose safety check always comes back SAFE, so tests
    /// of the standing-yes path itself are not also tests of the reviewer.
    fn cleared(events: Sender<Event>) -> WebApprover {
        WebApprover::new(
            events,
            Arc::new(Mutex::new(0)),
            Arc::new(AtomicBool::new(true)),
            settings(),
        )
        .with_checker(Verdict::Safe)
    }

    #[test]
    fn an_allow_decision_lets_the_tool_run() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = Arc::new(asking(tx));
        let a = Arc::clone(&approver);
        let handle = std::thread::spawn(move || a.confirm(&call()));
        // Wait for the request to appear before answering it.
        let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(event.kind, EventKind::ApprovalRequest { .. }));
        while !approver.resolve(true) {}
        assert!(handle.join().unwrap());
    }

    #[test]
    fn a_deny_decision_stops_the_tool() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = Arc::new(asking(tx));
        let a = Arc::clone(&approver);
        let handle = std::thread::spawn(move || a.confirm(&call()));
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        while !approver.resolve(false) {}
        assert!(!handle.join().unwrap());
    }

    /// An approval nobody answers must deny, not hang the agent forever.
    #[test]
    fn an_unanswered_approval_denies_on_timeout() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let approver = asking(tx).with_timeout(Duration::from_millis(50));
        assert!(!approver.confirm(&call()));
    }

    /// A closed browser stream means no decision can arrive.
    #[test]
    fn a_dead_event_stream_denies_immediately() {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(rx);
        let approver = asking(tx);
        assert!(!approver.confirm(&call()));
    }

    /// The mode itself: no question goes out, and the answer is yes, once
    /// the safety check has cleared the call.
    #[test]
    fn a_standing_yes_answers_without_asking_the_browser() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = cleared(tx);

        // No thread and no timeout: an auto-approval that parked at all would
        // hang this test, which is the failure worth catching.
        assert!(approver.confirm(&call()));

        let event = rx.try_recv().expect("nothing was recorded");
        match event.kind {
            EventKind::Notice { text } => assert_eq!(text, "auto-approved write_file"),
            other => panic!("the browser was asked instead of told: {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "more than one event went out");
    }

    /// Revoking mid-run. The same approver the agent is already holding has to
    /// go back to asking, or "turn it off" would mean "turn it off next time".
    #[test]
    fn revoking_it_makes_the_very_next_call_ask_again() {
        let (tx, rx) = std::sync::mpsc::channel();
        let standing = Arc::new(AtomicBool::new(true));
        let approver = Arc::new(
            WebApprover::new(
                tx,
                Arc::new(Mutex::new(0)),
                Arc::clone(&standing),
                settings(),
            )
            .with_checker(Verdict::Safe),
        );
        assert!(approver.confirm(&call()));

        standing.store(false, Ordering::SeqCst);
        let a = Arc::clone(&approver);
        let handle = std::thread::spawn(move || a.confirm(&call()));
        // The notice from the first call, then a real question for the second.
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            matches!(event.kind, EventKind::ApprovalRequest { .. }),
            "revoking left the gate down: {:?}",
            event.kind
        );
        while !approver.resolve(false) {}
        assert!(!handle.join().unwrap());
    }

    /// An auto-approval that cannot be written down does not happen. The
    /// transcript is the only record of what the standing yes let through.
    #[test]
    fn an_auto_approval_that_cannot_be_recorded_is_refused() {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(rx);
        let approver = cleared(tx);
        assert!(!approver.confirm(&call()));
    }

    /// The reviewer, not the browser, is asked first. An UNSAFE verdict does
    /// not refuse the call on its own; it falls through to the same
    /// approval request a session with no standing yes would send, with a
    /// notice first explaining why auto-approve did not fire.
    #[test]
    fn an_unsafe_verdict_falls_through_to_asking_a_human() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = Arc::new(
            WebApprover::new(
                tx,
                Arc::new(Mutex::new(0)),
                Arc::new(AtomicBool::new(true)),
                settings(),
            )
            .with_checker(Verdict::Unsafe),
        );
        let a = Arc::clone(&approver);
        let handle = std::thread::spawn(move || a.confirm(&call()));

        let notice = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        match notice.kind {
            EventKind::Notice { text } => assert!(
                text.contains("write_file") && text.contains("safety check"),
                "{text}"
            ),
            other => panic!("expected the flagged notice first: {other:?}"),
        }
        let request = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            matches!(request.kind, EventKind::ApprovalRequest { .. }),
            "an unsafe verdict did not fall through to asking: {:?}",
            request.kind
        );
        while !approver.resolve(true) {}
        assert!(handle.join().unwrap());
    }

    /// A verdict that is neither SAFE nor UNSAFE, the shape a down model
    /// endpoint or an unparseable answer takes, gets exactly the same
    /// treatment as UNSAFE: it asks, it does not deny on its own.
    #[test]
    fn an_unclear_verdict_also_falls_through_to_asking() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = Arc::new(
            WebApprover::new(
                tx,
                Arc::new(Mutex::new(0)),
                Arc::new(AtomicBool::new(true)),
                settings(),
            )
            .with_checker(Verdict::Unclear),
        );
        let a = Arc::clone(&approver);
        let handle = std::thread::spawn(move || a.confirm(&call()));

        rx.recv_timeout(Duration::from_secs(5)).unwrap(); // the flagged notice
        let request = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(request.kind, EventKind::ApprovalRequest { .. }));
        while !approver.resolve(false) {}
        assert!(!handle.join().unwrap());
    }

    /// With no model configured, the real checker (not the test seam) has to
    /// resolve to something, and it must be the fail-safe answer: ask
    /// rather than wave the call through with nothing having reviewed it.
    #[test]
    fn with_no_checker_and_no_configured_model_it_still_asks() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = Arc::new(WebApprover::new(
            tx,
            Arc::new(Mutex::new(0)),
            Arc::new(AtomicBool::new(true)),
            settings(),
        ));
        let a = Arc::clone(&approver);
        let handle = std::thread::spawn(move || a.confirm(&call()));

        rx.recv_timeout(Duration::from_secs(5)).unwrap(); // the flagged notice
        let request = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(request.kind, EventKind::ApprovalRequest { .. }));
        while !approver.resolve(false) {}
        assert!(!handle.join().unwrap());
    }

    /// The tool name in that notice comes from the model. It reaches the page
    /// as text either way, so this is about a transcript that stays readable.
    #[test]
    fn a_hostile_tool_name_cannot_run_away_with_the_transcript() {
        let label = tool_label(&format!("a\nb\u{0}{}", "x".repeat(500)));
        assert!(label.len() <= TOOL_NAME_MAX_BYTES, "{label}");
        assert!(!label.contains('\n') && !label.contains('\u{0}'), "{label}");
        // Cutting a multi-byte character in half would panic on the slice.
        assert!(tool_label(&"é".repeat(200)).len() <= TOOL_NAME_MAX_BYTES);
    }
}
