use serde::Serialize;

/// One finding, flattened for the wire.
///
/// A separate type from `zorp_agent::PanelFinding` so the JSON the browser
/// parses is this crate's contract rather than whatever shape the agent
/// crate happens to have today.
#[derive(Debug, Clone, Serialize)]
pub struct PanelFindingFrame {
    pub severity: String,
    pub claim: String,
    pub locus: String,
}

impl From<&zorp_agent::PanelFinding> for PanelFindingFrame {
    fn from(f: &zorp_agent::PanelFinding) -> Self {
        PanelFindingFrame {
            severity: f.severity.as_str().to_string(),
            claim: f.claim.clone(),
            locus: f.locus.clone(),
        }
    }
}

/// One locus and the lenses that raised it.
#[derive(Debug, Clone, Serialize)]
pub struct AgreementFrame {
    pub locus: String,
    pub lenses: Vec<String>,
    pub highest: String,
}

impl From<&zorp_agent::Agreement> for AgreementFrame {
    fn from(a: &zorp_agent::Agreement) -> Self {
        AgreementFrame {
            locus: a.locus.clone(),
            lenses: a.lenses.clone(),
            highest: a.highest.as_str().to_string(),
        }
    }
}

/// One recalled message, flattened for the wire.
///
/// The provenance the model was shown, so the person can check it: which
/// conversation, where in it, who wrote it, when, and how close the match
/// was. `author` is "you" or "the assistant", spelled out rather than left
/// as a role name, because the difference between a thing the user said and
/// a thing a model said is the whole point of showing this at all.
///
/// A separate type from `zorp_recall::Passage` so the JSON the browser
/// parses is this crate's contract, the same reason `PanelFindingFrame`
/// exists.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryCitationFrame {
    pub conversation_id: String,
    pub title: String,
    pub seq: i64,
    pub author: String,
    /// `YYYY-MM-DD`, or empty when the store recorded no date.
    pub when: String,
    pub text: String,
    pub score: f32,
}

#[cfg(feature = "memory")]
impl From<&crate::memory::Citation> for MemoryCitationFrame {
    fn from(c: &crate::memory::Citation) -> Self {
        MemoryCitationFrame {
            conversation_id: c.conversation_id.clone(),
            title: c.title.clone(),
            seq: c.seq,
            author: c.author.to_string(),
            when: c.when.clone(),
            text: c.text.clone(),
            score: c.score,
        }
    }
}

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
    /// One reviewer on a review panel has started.
    ///
    /// A panel is several agents working at once, so the browser needs a
    /// per reviewer signal rather than the single `Working` a turn emits.
    /// Without it a five reviewer panel shows one spinner for two minutes
    /// and no sign that four of them already finished.
    ReviewerStarted {
        lens: String,
    },
    /// One reviewer came back with a readable verdict.
    ///
    /// `findings` is already parsed, so the browser never sees the raw
    /// answer unless it asks. The count is what the live view shows.
    ReviewerFinished {
        lens: String,
        findings: Vec<PanelFindingFrame>,
        answer: String,
    },
    /// One reviewer did not come back with anything countable.
    ///
    /// Sent, not swallowed. A panel of five where two failed is not a
    /// panel of three, and a browser that only hears about successes
    /// would draw it as one.
    ReviewerFailed {
        lens: String,
        why: String,
    },
    /// The panel finished. Carries what only the whole set can say.
    PanelDone {
        target: String,
        lenses_requested: usize,
        verdicts: usize,
        /// True only when every requested reviewer returned a verdict.
        /// The browser must show a partial panel as partial: two of two
        /// agreeing is a weaker claim than two of five.
        complete: bool,
        /// Loci more than one lens raised, computed in code. Never asked
        /// of a model, because reviewers that negotiate agreement are one
        /// reviewer.
        agreements: Vec<AgreementFrame>,
    },
    /// One Zorp mode attempt finished, meaning one `investigate` run.
    ///
    /// `approved` is whether the post-attempt checkpoint kept the track
    /// alive. `None` means the attempt did not get that far, and an
    /// `Error` frame follows saying why. It matters that this frame goes
    /// out either way: conditions are recorded before the work starts,
    /// so an attempt that fell over still left something in the ledger,
    /// and a browser that only heard about successes would show nothing
    /// for it.
    ///
    /// The ledger itself is not in here. It is read back through
    /// `GET /api/investigate/ledger`, which is a reader the page can ask
    /// again without running anything.
    ///
    /// Declared whatever this crate was built with. The server only ever
    /// emits it under the `research` feature, but the browser bundle is
    /// one artifact and must know the shape either way.
    InvestigateDone {
        track_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        approved: Option<bool>,
    },
    /// This turn was told to look at earlier conversations, and here is
    /// exactly what it found.
    ///
    /// Sent before the model is called, and sent even when the answer is
    /// nothing, because "memory was on and found nothing" and "memory was
    /// off" are different states and a user who cannot tell them apart
    /// cannot tell whether an answer used their history.
    ///
    /// `unavailable` carries the reason a recall could not run at all, most
    /// often no local embedder. The turn goes ahead without memory rather
    /// than failing, and this is the only thing that says so.
    ///
    /// The snippets are text a model wrote or a page the agent fetched, so
    /// the browser draws every one of them through `textContent`.
    Memory {
        used: Vec<MemoryCitationFrame>,
        #[serde(skip_serializing_if = "Option::is_none")]
        unavailable: Option<String>,
    },
    /// This session now has a short, model-written name, and here it is.
    ///
    /// Sent after `Done`, because the title is asked for once the turn has
    /// an answer to read and the turn must not wait on it. Sent only when
    /// one was actually written: a failed, empty or declined titling call
    /// sends nothing at all and the sidebar keeps showing the first
    /// message, which is the correct thing for it to be showing.
    ///
    /// The text came from a model, so the browser puts it on the page
    /// through `textContent` like every other line a model wrote. It is
    /// display only: `sessions.task` still holds the verbatim first message
    /// and it is `task`, not this, that the recall index reads.
    SessionTitle {
        title: String,
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
