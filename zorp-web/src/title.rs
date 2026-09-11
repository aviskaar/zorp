//! Resolving the browser's model for a titling call, and putting the answer
//! on the session's event stream.
//!
//! Everything that decides *what* a title is lives in `zorp_agent::title`:
//! the prompt, the fences, the clamp, and the one write path to
//! `display_title`. It moved there so a conversation started in the terminal
//! gets a name too, and so there is one set of character rules rather than
//! two that drift. What is left here is the plumbing that needs
//! `SettingsHandle`, which is the same split `compaction.rs` uses.

use crate::event::{Event, EventKind};
use crate::state::SettingsHandle;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use zorp_agent::title::{enabled, title_session_in};
use zorp_agent::{HttpModel, Model, Store};

pub use zorp_agent::title::{clamp, prompt, scrub, ENABLED_ENV, MAX_CHARS, MAX_WORDS};

/// Anthropic's `max_tokens` for this one call. Ignored by
/// OpenAI-compatible endpoints, which have no equivalent here. A title is
/// a dozen tokens and the clamp catches anything longer.
const MAX_TOKENS: u32 = 64;

/// Ask the model, once, and hand back exactly what it said.
///
/// The clamp is deliberately not applied here. It belongs next to the
/// write, in `title_session_in`, so that nothing can ever reach the column
/// without going through it.
fn ask(settings: &SettingsHandle, question: &str, answer: &str) -> Option<String> {
    let resolved = settings.lock().unwrap().effective_model();
    if !resolved.configured {
        return None;
    }
    let url = zorp_agent::join_url(&resolved.base_url, resolved.provider.path_suffix());
    // No reasoning mode, deliberately, and not the one the person set for
    // their own work. A sidebar label is not worth a thinking budget, and
    // `ZORP_REASONING_MODE` was set for the turns they are reading.
    let model = HttpModel {
        url,
        api_key: resolved.api_key,
        model: resolved.model,
        provider: resolved.provider,
        max_tokens: Some(MAX_TOKENS),
    }
    .with_default_reasoning_mode(None);
    let reply = model.complete(&prompt(question, answer), &[]).ok()?;
    Some(reply.content)
}

/// Name this session if it still needs a name, then say so on its stream.
///
/// Everything here is best effort and every failure is the same failure:
/// nothing is written, and the sidebar keeps showing the first message.
/// That is why the return type is `()` and why nothing is reported to the
/// browser when it does not work. A conversation with no title is not a
/// broken conversation.
fn title_session(session_id: &str, settings: &SettingsHandle) -> Option<String> {
    let store = Store::open_default().ok()?;
    title_session_in(&store, session_id, |question, answer| {
        ask(settings, question, answer)
    })
}

/// Run the titling for one session on its own thread.
///
/// Called after the turn's closing events have gone out, so the reply and
/// the `Done` that re-enables the composer are already on their way before
/// this starts. It never touches the turn's outcome and it cannot fail it.
///
/// The event goes down the turn's own channel rather than onto the backlog
/// directly, which is what keeps the sequence numbers in order: the drain
/// thread appends in channel order and a browser drops anything at or below
/// the last id it saw. Holding a sender also keeps that drain thread alive
/// until this finishes.
pub fn spawn_titling(
    session_id: String,
    settings: SettingsHandle,
    tx: Sender<Event>,
    seq: Arc<Mutex<u64>>,
) {
    if !enabled() {
        return;
    }
    std::thread::spawn(move || {
        let Some(title) = title_session(&session_id, &settings) else {
            return;
        };
        let mut next = seq.lock().unwrap();
        let _ = tx.send(Event {
            seq: *next,
            kind: EventKind::SessionTitle { title },
        });
        *next += 1;
    });
}
