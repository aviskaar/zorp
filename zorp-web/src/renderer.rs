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
    /// When set, sequence numbers come from here instead of the local
    /// counter, so the renderer and the approval gate share one ordering.
    shared: Option<std::sync::Arc<std::sync::Mutex<u64>>>,
}

impl WebRenderer {
    pub fn new(tx: Sender<Event>) -> Self {
        WebRenderer {
            tx,
            seq: 0,
            shared: None,
        }
    }

    /// Share a sequence counter with another emitter, such as the approval
    /// gate, so all events on one session are totally ordered.
    pub fn set_seq(&mut self, shared: std::sync::Arc<std::sync::Mutex<u64>>) {
        self.shared = Some(shared);
    }

    fn next_seq(&mut self) -> u64 {
        match &self.shared {
            Some(shared) => {
                let mut guard = shared.lock().unwrap();
                let seq = *guard;
                *guard += 1;
                seq
            }
            None => {
                let seq = self.seq;
                self.seq += 1;
                seq
            }
        }
    }

    pub fn emit(&mut self, kind: EventKind) {
        let seq = self.next_seq();
        let _ = self.tx.send(Event { seq, kind });
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

    fn assistant_delta(&mut self, chunk: &str) {
        self.emit(EventKind::AssistantDelta {
            text: chunk.to_string(),
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

    #[test]
    fn deltas_become_their_own_event_kind_rather_than_assistant_messages() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut r = WebRenderer::new(tx);
        r.assistant_delta("he");
        r.assistant_delta("llo");
        r.assistant("hello");
        drop(r);

        let events: Vec<Event> = rx.iter().collect();
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match &e.kind {
                EventKind::AssistantDelta { .. } => "delta",
                EventKind::Assistant { .. } => "final",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["delta", "delta", "final"],
            "the browser cannot tell a preview from the answer"
        );
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
