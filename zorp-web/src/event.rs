use serde::Serialize;

/// One frame on the SSE stream.
///
/// `seq` is monotonic per session so a browser that reconnects can send
/// `Last-Event-ID` and receive only what it missed.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub seq: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// The first six variants map one to one onto the agent's `Renderer` trait,
/// which is the point: the browser sees exactly what the terminal sees.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    Working,
    WorkingDone,
    Tool {
        name: String,
        summary: String,
    },
    Verify {
        command: String,
        passed: bool,
    },
    Notice {
        text: String,
    },
    Assistant {
        text: String,
    },
    /// The agent has parked on an approval-gated tool and is waiting for a
    /// decision from the browser.
    ApprovalRequest {
        id: String,
        tool: String,
        arguments: String,
    },
    Error {
        message: String,
    },
    Done,
}
