use crate::event::{Event, EventKind};
use std::sync::mpsc::Sender;
use zorp_agent::Renderer;

/// Bridges the agent's activity callbacks onto a channel the SSE endpoint
/// drains.
///
/// Sends are best effort. A browser that has gone away must not stall the
/// agent, so a closed receiver is ignored rather than propagated.
pub struct WebRenderer {
    tx: Sender<Event>,
    seq: u64,
}

impl WebRenderer {
    pub fn new(tx: Sender<Event>) -> Self {
        WebRenderer { tx, seq: 0 }
    }

    pub fn emit(&mut self, kind: EventKind) {
        let event = Event { seq: self.seq, kind };
        self.seq += 1;
        let _ = self.tx.send(event);
    }
}

impl Renderer for WebRenderer {
    fn working(&mut self) {
        self.emit(EventKind::Working);
    }

    fn working_done(&mut self) {
        self.emit(EventKind::WorkingDone);
    }

    fn tool(&mut self, name: &str, summary: &str) {
        self.emit(EventKind::Tool {
            name: name.to_string(),
            summary: summary.to_string(),
        });
    }

    fn verify(&mut self, command: &str, passed: bool) {
        self.emit(EventKind::Verify {
            command: command.to_string(),
            passed,
        });
    }

    fn notice(&mut self, text: &str) {
        self.emit(EventKind::Notice {
            text: text.to_string(),
        });
    }

    fn assistant(&mut self, text: &str) {
        self.emit(EventKind::Assistant {
            text: text.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_callbacks_become_events_in_order() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut r = WebRenderer::new(tx);
        r.tool("read_file", "a.txt (1 lines)");
        r.assistant("hello");
        drop(r);

        let events: Vec<Event> = rx.iter().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 1);
        assert!(matches!(&events[0].kind, EventKind::Tool { name, .. } if name == "read_file"));
        assert!(matches!(&events[1].kind, EventKind::Assistant { text } if text == "hello"));
    }

    /// The browser reconnects with Last-Event-ID, so seq must never repeat.
    #[test]
    fn seq_is_monotonic_across_kinds() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut r = WebRenderer::new(tx);
        r.working();
        r.notice("n");
        r.verify("cargo test", true);
        drop(r);
        let seqs: Vec<u64> = rx.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    /// A browser that closed its stream must not take the agent down with it.
    #[test]
    fn a_dropped_receiver_does_not_panic_the_renderer() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut r = WebRenderer::new(tx);
        drop(rx);
        r.assistant("nobody is listening");
    }

    #[test]
    fn events_serialize_tagged_by_type() {
        let e = Event {
            seq: 7,
            kind: EventKind::Tool {
                name: "run_command".into(),
                summary: "exited 0".into(),
            },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"tool\""), "{json}");
        assert!(json.contains("\"seq\":7"), "{json}");
        assert!(json.contains("\"name\":\"run_command\""), "{json}");
    }
}
