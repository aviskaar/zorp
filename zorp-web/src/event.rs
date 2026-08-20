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
    /// A fragment of the answer, as the provider produces it.
    ///
    /// A preview, not the answer. The browser shows these as they land and
    /// then replaces them with the `Assistant` text below, which is the one
    /// authoritative statement of what the model said. Treating deltas as
    /// final is how a dropped frame becomes a silently truncated answer.
    AssistantDelta {
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
    /// How full the context window is.
    ///
    /// `source` is load bearing and must reach the page: `reported` is what
    /// the provider said the last request cost, `estimated` is zorp counting
    /// bytes over four. A meter that draws them identically is claiming a
    /// precision it does not have.
    ///
    /// `limit_tokens` is absent when nobody has said how large the window is,
    /// which is the default. zorp talks to arbitrary endpoints and there is no
    /// reliable way to ask one, so it never guesses; the browser then shows
    /// what was used and says the window is unset instead of inventing a
    /// denominator.
    Context {
        used_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        limit_tokens: Option<u64>,
        source: String,
    },
    Error {
        message: String,
    },
    /// A human pressed stop and the run ended because of it.
    ///
    /// Separate from `Error` because it is not one. The agent reports a
    /// cancelled run as an outcome like any other, and sending that down as an
    /// error card put "cancelled" under a "Something went wrong" heading for
    /// something the user did on purpose. `Done` still follows: a stopped turn
    /// is still a turn that ended.
    Stopped,
    Done,
}
