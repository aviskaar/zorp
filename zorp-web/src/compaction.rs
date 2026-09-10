//! The model call behind stage two of compaction.
//!
//! Everything that decides *what* a summary says lives in
//! `zorp_agent::compaction`: the prompt, the fences, the clamp, and the
//! block that carries the result to the model. One prompt and one clamp in
//! the workspace, so the browser and the CLI cannot drift apart on what a
//! summary is. What lives here is the plumbing that needs settings, which
//! is the same split `title.rs` uses and for the same reason.
//!
//! The call is shaped like `title::ask`: an `HttpModel` built from
//! `settings.effective_model()`, no reasoning mode, one attempt, through
//! `zorp::http_agent` like every other model call in this workspace. No new
//! HTTP client and no new timeout variable.
//!
//! A failure returns `Err` with the provider's own words and the caller
//! falls back to stage one's deterministic elision, which is what a turn
//! ran on before any of this existed. A failed summary never blocks a turn.

use crate::state::SettingsHandle;
use zorp_agent::{HttpModel, Message, Summarizer};

/// Anthropic's `max_tokens` for this call. Ignored by OpenAI-compatible
/// endpoints, which have no equivalent here.
///
/// Generous, because the whole value of the summary is the section that
/// quotes every user message, and a summary cut off partway through that
/// section is refused by `clamp` and the call is wasted. The real bound on
/// length is `compaction::MAX_SUMMARY_CHARS`, enforced in code on the one
/// path to the store.
const MAX_TOKENS: u32 = 8192;

/// Writes summaries with the session's own model.
pub struct SettingsSummarizer {
    settings: SettingsHandle,
    /// The `# Compact instructions` section of this workspace's instruction
    /// files, when there is one. Untrusted text from a file in a repository
    /// somebody may have cloned; it is fenced as a preference by
    /// `compaction::prompt` and can change no rule.
    standing: Option<String>,
}

impl SettingsSummarizer {
    pub fn new(settings: SettingsHandle, standing: Option<String>) -> SettingsSummarizer {
        SettingsSummarizer { settings, standing }
    }

    /// Read standing compaction instructions out of the workspace's
    /// instruction files, which are the same files the system prompt is
    /// built from.
    pub fn standing_for(workspace: &std::path::Path) -> Option<String> {
        let loaded = zorp_agent::load_instructions(workspace, workspace)?;
        zorp_agent::compaction::standing_instructions(&loaded)
    }
}

impl Summarizer for SettingsSummarizer {
    fn summarize(&mut self, older: &[Message], focus: Option<&str>) -> Result<String, String> {
        let resolved = self.settings.lock().unwrap().effective_model();
        if !resolved.configured {
            return Err("no model is configured, so no summary could be written".to_string());
        }
        let url = zorp_agent::join_url(&resolved.base_url, resolved.provider.path_suffix());
        // No reasoning mode, deliberately, and not the one the person set
        // for their own work: `ZORP_REASONING_MODE` was set for the turns
        // they are reading, and this is not one of them.
        let model = HttpModel {
            url,
            api_key: resolved.api_key,
            model: resolved.model,
            provider: resolved.provider,
            max_tokens: Some(MAX_TOKENS),
        }
        .with_default_reasoning_mode(None);
        zorp_agent::compaction::summarize(&model, older, focus, self.standing.as_deref())
    }
}

/* -------------------------------------------------------------------- */
/* manual /compact, against the store                                    */
/* -------------------------------------------------------------------- */

/// What a manual compaction did.
pub enum Compacted {
    /// There was not enough in front of the recent exchanges to be worth a
    /// model call. The route says so rather than doing nothing.
    TooShort,
    Done {
        boundary_seq: i64,
        tokens_before: u64,
        tokens_after: u64,
    },
}

pub enum CompactError {
    NoSuchSession,
    /// The provider's own words.
    Failed(String),
}

/// Summarize the older part of a stored conversation, right now.
///
/// Against the store rather than a live agent, because a manual compaction
/// happens between turns and there is no agent to reach. The boundary comes
/// from `compaction::boundary`, the same function the agent loop uses, so a
/// summary written here covers exactly what one written mid-run would.
///
/// **Nothing in `messages` is written, rewritten, or deleted.** The one
/// write is the `compactions` row, and the next turn's seed reads it.
///
/// `session` is the live state, when this process has one, so the frames
/// reach an open page. A session nobody has open still compacts; the frames
/// simply have nowhere to go.
///
/// Blocking; call it off the async runtime.
pub fn compact_stored(
    session_id: &str,
    focus: Option<String>,
    settings: &SettingsHandle,
    workspace: Option<&std::path::Path>,
    session: Option<std::sync::Arc<std::sync::Mutex<crate::state::SessionState>>>,
) -> Result<Compacted, CompactError> {
    use zorp_agent::compaction as core;

    let mut store =
        zorp_agent::Store::open_default().map_err(|e| CompactError::Failed(e.to_string()))?;
    if store.session_status(session_id).ok().flatten().is_none() {
        return Err(CompactError::NoSuchSession);
    }

    let stored = store
        .load_message_records(session_id)
        .map_err(|e| CompactError::Failed(e.to_string()))?;
    let already = store
        .latest_compaction(session_id)
        .unwrap_or_default()
        .map(|c| c.boundary_seq)
        .unwrap_or(-1);

    // The stored seq of each message that is not already summarized and is
    // not a system message. The index in `stored` is the seq: the recorder
    // assigns them from zero, one per message, and the loader orders by
    // them.
    let live: Vec<(i64, zorp_agent::Message)> = stored
        .into_iter()
        .enumerate()
        .map(|(seq, r)| (seq as i64, r.message))
        .filter(|(seq, m)| *seq > already && m.role != "system")
        .collect();
    if live.len() < core::MIN_MESSAGES_TO_COMPACT {
        return Ok(Compacted::TooShort);
    }
    let messages: Vec<zorp_agent::Message> = live.iter().map(|(_, m)| m.clone()).collect();
    let Some(cut) = core::boundary(&messages, 0) else {
        return Ok(Compacted::TooShort);
    };
    let older = &messages[..cut];
    let boundary_seq = live[cut - 1].0;
    let tokens_before = zorp_agent::estimate_tokens(&messages);

    let emit = Emitter::new(session);
    emit.compacting(older.len(), tokens_before, true);

    let standing = workspace.and_then(SettingsSummarizer::standing_for);
    let mut summarizer = SettingsSummarizer::new(settings.clone(), standing);
    let summary = match Summarizer::summarize(&mut summarizer, older, focus.as_deref()) {
        Ok(summary) => summary,
        Err(reason) => {
            emit.compacted(&zorp_agent::CompactionOutcome {
                ok: false,
                boundary_seq: None,
                tokens_before,
                tokens_after: tokens_before,
                summary: None,
                reason: Some(reason.clone()),
                manual: true,
            });
            return Err(CompactError::Failed(reason));
        }
    };

    // What the next turn's seed will hold: the block, then the tail.
    let block = core::block(&summary, &core::block_nonce(&summary));
    let mut after_messages = vec![block];
    after_messages.extend_from_slice(&messages[cut..]);
    let tokens_after = zorp_agent::estimate_tokens(&after_messages);

    let resolved = settings.lock().unwrap().effective_model();
    store
        .record_compaction(
            session_id,
            &zorp_agent::Compaction {
                id: 0,
                boundary_seq,
                summary: summary.clone(),
                focus,
                model: resolved.model,
                tokens_before: tokens_before as i64,
                tokens_after: tokens_after as i64,
                manual: true,
                created: 0,
            },
        )
        .map_err(|e| CompactError::Failed(e.to_string()))?;

    emit.compacted(&zorp_agent::CompactionOutcome {
        ok: true,
        boundary_seq: Some(boundary_seq),
        tokens_before,
        tokens_after,
        summary: Some(summary),
        reason: None,
        manual: true,
    });
    Ok(Compacted::Done {
        boundary_seq,
        tokens_before,
        tokens_after,
    })
}

/// Puts frames on a session's stream, when this process has that session.
///
/// The events go straight into the backlog under the session's own sequence
/// counter, which is what the stream drains. A compaction is not a turn and
/// does not set `running`: it holds the session only for as long as one
/// blocking call takes, and the route's 409 is what keeps it from
/// overlapping with a turn.
struct Emitter(Option<std::sync::Arc<std::sync::Mutex<crate::state::SessionState>>>);

impl Emitter {
    fn new(
        session: Option<std::sync::Arc<std::sync::Mutex<crate::state::SessionState>>>,
    ) -> Emitter {
        Emitter(session)
    }

    fn push(&self, kind: crate::event::EventKind) {
        let Some(session) = &self.0 else {
            return;
        };
        let mut guard = session.lock().unwrap();
        let seq = {
            let mut next = guard.seq.lock().unwrap();
            let seq = *next;
            *next += 1;
            seq
        };
        guard.backlog.push(crate::event::Event { seq, kind });
    }

    fn compacting(&self, messages: usize, tokens_before: u64, manual: bool) {
        self.push(crate::event::EventKind::Compacting {
            messages,
            tokens_before,
            manual,
        });
    }

    fn compacted(&self, result: &zorp_agent::CompactionOutcome) {
        self.push(crate::event::EventKind::Compacted {
            ok: result.ok,
            boundary_seq: result.boundary_seq,
            tokens_before: result.tokens_before,
            tokens_after: result.tokens_after,
            summary: result.summary.clone(),
            reason: result.reason.clone(),
            manual: result.manual,
        });
        // The meter reads `context` frames and nothing else, so it has to
        // hear about a transcript that just got smaller.
        if result.ok {
            self.push(crate::event::EventKind::Context {
                used_tokens: result.tokens_after,
                limit_tokens: zorp_agent::ContextBudget::from_env().limit_tokens,
                source: zorp_agent::UsageSource::Estimated.as_str().to_string(),
            });
        }
    }
}
