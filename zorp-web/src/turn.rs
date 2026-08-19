use crate::approval::WebApprover;
use crate::event::{Event, EventKind};
use crate::renderer::WebRenderer;
use crate::state::{SessionState, SettingsHandle};
use std::sync::{Arc, Mutex};
use zorp_agent::{
    cancel_token, Agent, ApprovalMode, HttpModel, Outcome, SqliteRecorder, Store,
    DEFAULT_SYSTEM_PROMPT,
};

/// The system prompt this server hands the agent.
///
/// A function rather than a `use` at the call site so a test can assert on
/// the value the browser actually gets. The browser is the surface where the
/// old prompt did its visible damage: it said only "a careful assistant", and
/// a local model filled in the rest by introducing itself as a "coding
/// buddy". zorp is a research agent, and there is now exactly one string in
/// the workspace that says what zorp is.
pub fn system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

/// Append one event to a session's replay backlog.
///
/// **The backlog is append-only, and must stay that way.** `stream_events` in
/// `api.rs` holds an index into it across polls and slices from there, so
/// anything that shortens this vector panics that task on the next tick and
/// poisons the session mutex, which takes every later request with it.
///
/// This function exists to say so where the next person will be tempted.
/// Streaming a turn writes one entry per fragment, and dropping them once the
/// finished answer arrived looked like an obvious saving. It is not available
/// without also teaching the reader that its index can move backwards, and a
/// few hundred kilobytes per session does not buy that risk.
fn record(backlog: &mut Vec<Event>, event: Event) {
    backlog.push(event);
}

/// The events that close a turn.
///
/// Every path ends with `Done`, the failures included. The browser
/// re-enables its composer on `Done` and on nothing else, so a turn that
/// closed with only an `Error` left the send button disabled until the page
/// was reloaded. An error ends a turn exactly as much as an answer does, and
/// the one thing a user needs after a failure is the ability to try again.
fn closing_events(outcome: Result<Outcome, String>) -> Vec<EventKind> {
    let mut kinds = Vec::new();
    match outcome {
        Ok(Outcome::Complete(text)) => {
            // An empty answer gets no bubble. The turn still ended.
            if !text.trim().is_empty() {
                kinds.push(EventKind::Assistant { text });
            }
        }
        Ok(other) => kinds.push(EventKind::Error {
            message: other.describe(),
        }),
        Err(message) => kinds.push(EventKind::Error { message }),
    }
    kinds.push(EventKind::Done);
    kinds
}

/// Run one turn to completion on a blocking thread.
///
/// The agent loop is synchronous, so it must not run on the async runtime.
/// Events are drained from the renderer's channel into the session backlog as
/// they arrive, which is what lets the SSE endpoint stream a run that is still
/// in progress.
pub fn spawn_turn(
    session: Arc<Mutex<SessionState>>,
    session_id: String,
    message: String,
    settings: SettingsHandle,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    // The counter lives on the session so numbering continues across turns.
    let seq = {
        let mut guard = session.lock().unwrap();
        guard.running = true;
        Arc::clone(&guard.seq)
    };
    let approver = Arc::new(WebApprover::new(tx.clone(), Arc::clone(&seq)));
    session.lock().unwrap().approver = Some(Arc::clone(&approver));

    // Drain into the backlog on its own thread so a slow browser cannot
    // apply backpressure to the agent.
    let drain_session = Arc::clone(&session);
    std::thread::spawn(move || {
        for event in rx {
            record(&mut drain_session.lock().unwrap().backlog, event);
        }
    });

    std::thread::spawn(move || {
        let mut renderer = WebRenderer::new(tx.clone());
        // The renderer and the approver share one counter so approval
        // requests interleave correctly with activity.
        renderer.set_seq(Arc::clone(&seq));

        let outcome = run_agent(
            &session_id,
            &message,
            Box::new(renderer),
            approver,
            &settings,
        );

        // The final answer arrives in Outcome::Complete rather than through
        // the renderer. The CLI prints it in finish(); the browser has to be
        // sent it explicitly or the turn ends with activity and no reply.
        let kinds = closing_events(outcome);
        let mut next = seq.lock().unwrap();
        for kind in kinds {
            let _ = tx.send(Event { seq: *next, kind });
            *next += 1;
        }
        drop(next);
        session.lock().unwrap().running = false;
    });
}

fn run_agent(
    session_id: &str,
    message: &str,
    renderer: Box<dyn zorp_agent::Renderer>,
    approver: Arc<WebApprover>,
    settings: &SettingsHandle,
) -> Result<Outcome, String> {
    let resolved = settings.lock().unwrap().effective_model();
    if !resolved.configured {
        return Err("no model configured, open settings and pick one".to_string());
    }
    let url = zorp_agent::join_url(&resolved.base_url, resolved.provider.path_suffix());
    let model = HttpModel {
        url,
        api_key: resolved.api_key,
        model: resolved.model,
        provider: resolved.provider,
        max_tokens: resolved.max_tokens,
    }
    // Preserve the existing ZORP_REASONING_MODE support that
    // `HttpModel::try_from_env` used to apply, now that the base
    // model itself is built from resolved settings instead of `from_env`.
    .try_with_env_reasoning_mode(None)
    .map_err(|e| e.to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let steps = std::env::var("ZORP_MAX_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let cwd_display = cwd.display().to_string();
    let mut agent = Agent::new(
        Box::new(model),
        system_prompt(),
        steps,
        cwd,
        cancel_token(),
        ApprovalMode::Interactive(approver),
    )
    .register_builtins_filtered(None)
    .with_renderer(renderer);

    // Persist the conversation the same way the CLI does, so the sidebar and
    // replay survive a restart. Without a recorder the agent runs fine and
    // remembers nothing, which is what the first version did.
    if let Ok(store) = Store::open_default() {
        let seq = store.message_count(session_id).unwrap_or(0);
        if seq == 0 {
            let _ = store.create_session(session_id, message, &cwd_display, "");
        }
        agent = agent.with_recorder(Box::new(SqliteRecorder::new(
            store,
            session_id.to_string(),
            seq,
            0,
        )));
    }

    Ok(agent.run(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser is where this went wrong in front of a user, so the
    /// browser's own prompt gets its own test rather than relying on the
    /// library's.
    #[test]
    fn the_browser_gets_the_research_agent_prompt() {
        assert!(
            system_prompt().contains("research agent"),
            "the web UI is back to a prompt that does not say what zorp is: {}",
            system_prompt()
        );
    }

    /// The specific failure: a prompt vague enough that the model invents the
    /// product's positioning on its own.
    #[test]
    fn the_browsers_prompt_is_not_a_generic_assistant_line() {
        let lowered = system_prompt().to_lowercase();
        for vague in ["careful assistant", "helpful assistant", "coding"] {
            assert!(
                !lowered.contains(vague),
                "the web UI prompt is generic again ({vague}), which is what let a \
                 model introduce zorp as a coding buddy"
            );
        }
    }

    fn delta(text: &str) -> Event {
        Event {
            seq: 0,
            kind: EventKind::AssistantDelta { text: text.into() },
        }
    }

    /// The invariant `stream_events` depends on. It keeps an index into this
    /// vector across polls, so a backlog that ever gets shorter panics that
    /// task and poisons the session mutex behind it.
    #[test]
    fn the_backlog_only_ever_grows() {
        let mut backlog = Vec::new();
        let mut seen = 0;
        for event in [
            delta("he"),
            delta("llo"),
            Event {
                seq: 2,
                kind: EventKind::Assistant {
                    text: "hello".into(),
                },
            },
            Event {
                seq: 3,
                kind: EventKind::Done,
            },
        ] {
            record(&mut backlog, event);
            assert!(
                backlog.len() >= seen,
                "the backlog shrank from {seen} to {}",
                backlog.len()
            );
            // What the reader does every tick. It panicked here before.
            let _ = &backlog[seen..];
            seen = backlog.len();
        }
        assert_eq!(backlog.len(), 4);
    }

    /// Fragments are kept rather than pruned once the answer lands. They cost
    /// memory; removing them cost a poisoned mutex.
    #[test]
    fn fragments_survive_the_finished_answer() {
        let mut backlog = Vec::new();
        record(&mut backlog, delta("he"));
        record(&mut backlog, delta("llo"));
        record(
            &mut backlog,
            Event {
                seq: 2,
                kind: EventKind::Assistant {
                    text: "hello".into(),
                },
            },
        );
        assert_eq!(backlog.len(), 3);
    }

    /// One string, not two. This is the property that stops the next surface
    /// from writing its own.
    #[test]
    fn the_browser_and_the_cli_share_one_prompt() {
        assert_eq!(
            system_prompt(),
            zorp_agent::DEFAULT_SYSTEM_PROMPT,
            "zorp-web has started keeping its own copy of the system prompt"
        );
    }

    /// A turn that failed is still a turn that ended.
    ///
    /// The browser re-enables the composer on `Done` and on nothing else, so
    /// a turn closing with only an `Error` left the send button disabled
    /// until the page was reloaded. Found while testing the artifact pane,
    /// which is to say: found by a user, not by this suite.
    #[test]
    fn a_failed_turn_still_ends_the_turn() {
        let kinds = closing_events(Err("the model went away".to_string()));
        assert!(
            matches!(kinds.last(), Some(EventKind::Done)),
            "a failed turn never told the browser it was over: {kinds:?}"
        );
        assert!(
            kinds.iter().any(
                |k| matches!(k, EventKind::Error { message } if message.contains("went away"))
            ),
            "the failure was swallowed: {kinds:?}"
        );
    }

    /// Same for an outcome that is not an error but is not an answer either,
    /// such as a cancel or a step limit.
    #[test]
    fn an_outcome_that_is_not_an_answer_still_ends_the_turn() {
        let kinds = closing_events(Ok(Outcome::Cancelled));
        assert!(
            matches!(kinds.last(), Some(EventKind::Done)),
            "a cancelled turn never ended: {kinds:?}"
        );
    }

    #[test]
    fn a_successful_turn_sends_its_answer_and_then_ends() {
        let kinds = closing_events(Ok(Outcome::Complete("the answer".into())));
        assert!(
            matches!(&kinds[0], EventKind::Assistant { text } if text == "the answer"),
            "{kinds:?}"
        );
        assert!(matches!(kinds.last(), Some(EventKind::Done)), "{kinds:?}");
    }

    /// An empty answer is not worth a bubble, but the turn still ended.
    #[test]
    fn an_empty_answer_ends_the_turn_without_an_empty_message() {
        let kinds = closing_events(Ok(Outcome::Complete("   ".into())));
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, EventKind::Assistant { .. })),
            "{kinds:?}"
        );
        assert!(matches!(kinds.last(), Some(EventKind::Done)), "{kinds:?}");
    }
}
