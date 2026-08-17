use crate::event::{Event, EventKind};
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

/// Parks the agent thread until the browser answers.
///
/// `confirm` is called on the agent's blocking thread, so waiting here is
/// correct: it is exactly the terminal prompt's behavior with the prompt
/// moved into a browser.
pub struct WebApprover {
    events: Sender<Event>,
    /// Shared with the renderer so approval requests interleave correctly
    /// with activity in one ordered stream.
    seq: Arc<Mutex<u64>>,
    pending: Arc<Mutex<Option<Sender<bool>>>>,
    inbox: Arc<Mutex<Option<Receiver<bool>>>>,
    timeout: Duration,
}

impl WebApprover {
    pub fn new(events: Sender<Event>, seq: Arc<Mutex<u64>>) -> Self {
        WebApprover {
            events,
            seq,
            pending: Arc::new(Mutex::new(None)),
            inbox: Arc::new(Mutex::new(None)),
            timeout: APPROVAL_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
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

    fn next_seq(&self) -> u64 {
        let mut guard = self.seq.lock().unwrap();
        let seq = *guard;
        *guard += 1;
        seq
    }
}

impl Approver for WebApprover {
    fn confirm(&self, call: &ToolCall) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        *self.pending.lock().unwrap() = Some(tx);
        *self.inbox.lock().unwrap() = Some(rx);

        let id = format!("approval-{}", self.next_seq());
        let event = Event {
            seq: self.next_seq(),
            kind: EventKind::ApprovalRequest {
                id: id.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call() -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: "write_file".into(),
            arguments: json!({"path": "a.txt", "content": "hi"}),
        }
    }

    #[test]
    fn an_allow_decision_lets_the_tool_run() {
        let (tx, rx) = std::sync::mpsc::channel();
        let approver = Arc::new(WebApprover::new(tx, Arc::new(Mutex::new(0))));
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
        let approver = Arc::new(WebApprover::new(tx, Arc::new(Mutex::new(0))));
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
        let approver =
            WebApprover::new(tx, Arc::new(Mutex::new(0))).with_timeout(Duration::from_millis(50));
        assert!(!approver.confirm(&call()));
    }

    /// A closed browser stream means no decision can arrive.
    #[test]
    fn a_dead_event_stream_denies_immediately() {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(rx);
        let approver = WebApprover::new(tx, Arc::new(Mutex::new(0)));
        assert!(!approver.confirm(&call()));
    }
}
