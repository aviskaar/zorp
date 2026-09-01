use crate::approval::WebApprover;
use crate::event::{Event, EventKind};
use crate::renderer::WebRenderer;
use crate::state::{SessionState, SettingsHandle};
use std::sync::{Arc, Mutex};
use zorp_agent::{
    cancel_token, plan_seed, Agent, ApprovalMode, ContextBudget, HttpModel, Outcome, SeedPlan,
    SqliteRecorder, Store, DEFAULT_SYSTEM_PROMPT,
};

/// The transcript a turn on this session starts from.
///
/// This function is the fix for the bug that the browser had no memory. A
/// turn used to be a brand new `Agent` handed one message, with a recorder
/// attached that wrote the conversation to the store and nothing that ever
/// read it back. The sidebar and the replay endpoint read the store for
/// display, so the transcript looked right on screen while the model was
/// being told, every single turn, that the conversation had just started. Ask
/// it to convert a file and then ask what it just converted and it answered
/// "this is the start of our session".
///
/// The store is the source, not an in-memory agent kept alive per session.
/// The store outlives the process; a live agent does not. Rebuilding from the
/// record means reopening a session from the sidebar after a restart
/// continues it, which is the same code path as continuing it a second after
/// the last turn.
fn seed_transcript(store: &Store, session_id: &str, budget: &ContextBudget) -> SeedPlan {
    let stored = store.load_message_records(session_id).unwrap_or_default();
    plan_seed(stored, system_prompt(), budget)
}

/// The policy every agent on this server runs under.
///
/// Same policy the agent already defaults to, plus the one thing only the
/// server knows: its own port. Commands that call back into this server are
/// denied, because one approved `run_command` is otherwise enough to stand
/// the approval gate down and leave every later call unreviewed.
///
/// A function rather than a line inside `run_agent` because
/// `GET /api/capabilities` has to answer questions about it, and an answer
/// about a policy the turn does not use would be worse than no answer.
pub fn policy(own_port: Option<u16>) -> zorp_agent::Policy {
    let policy = zorp_agent::Policy::default();
    match own_port {
        Some(port) => policy.with_own_server(port),
        None => policy,
    }
}

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
/// Every path ends with `Done`, the failures included, and a stop is a
/// failure for this purpose even though it is not one for the reader. The
/// browser re-enables its composer on `Done` and on nothing else, so a turn
/// that closed with only an `Error` left the send button disabled until the
/// page was reloaded. An error ends a turn exactly as much as an answer does,
/// and the one thing a user needs after a failure is the ability to try again.
///
/// `stopped` says a human pressed stop, which is not something the outcome
/// can tell us on its own: the agent reports `Outcome::Cancelled` for any
/// raised cancel flag, and a run that was stopped a moment after it finished
/// still comes back `Complete`. The flag decides whether the transcript reads
/// "you stopped this" or "this fell over".
fn closing_events(outcome: Result<Outcome, String>, stopped: bool) -> Vec<EventKind> {
    let mut kinds = Vec::new();
    match outcome {
        Ok(Outcome::Complete(text)) => {
            // An empty answer gets no bubble. The turn still ended.
            if !text.trim().is_empty() {
                kinds.push(EventKind::Assistant { text });
            }
        }
        // A stop is the explanation for a cancelled or half-finished run, so
        // there is nothing left for an error card to add. A real failure is
        // still reported below even when a stop landed on top of it: the stop
        // says why nothing was retried, not why the run broke.
        Ok(_) if stopped => {}
        Ok(other) => kinds.push(EventKind::Error {
            message: other.describe(),
        }),
        Err(message) => kinds.push(EventKind::Error { message }),
    }
    if stopped {
        kinds.push(EventKind::Stopped);
    }
    kinds.push(EventKind::Done);
    kinds
}

/// Whether this one turn was told to look at earlier conversations.
///
/// A parameter and not a session setting, so it is a decision made once per
/// message rather than a mode somebody leaves on. Retrieval spends context
/// and puts untrusted text in front of the model, and both of those are
/// things a person should be choosing each time rather than discovering.
pub type UseMemory = bool;

#[cfg(feature = "recall")]
pub type RecallFeed = Option<crate::recall::IndexerHandle>;
#[cfg(not(feature = "recall"))]
pub type RecallFeed = ();

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
    use_memory: UseMemory,
    settings: SettingsHandle,
    own_port: Option<u16>,
    recall_indexer: RecallFeed,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    // One token per turn, held by the session so the stop endpoint can reach
    // it and passed to the agent so raising it reaches the run. It used to be
    // built inside `run_agent`, which meant nothing outside that function
    // ever had a handle on it: the agent was cancellable and nothing could
    // cancel it.
    let cancel = cancel_token();
    // The counter lives on the session so numbering continues across turns.
    // So does the standing approval answer: a user who stood approvals down
    // did it for this conversation, not for one message of it.
    let (seq, auto_approve) = {
        let mut guard = session.lock().unwrap();
        guard.running = true;
        guard.cancel = Some(Arc::clone(&cancel));
        (Arc::clone(&guard.seq), Arc::clone(&guard.auto_approve))
    };
    let approver = Arc::new(WebApprover::new(
        tx.clone(),
        Arc::clone(&seq),
        auto_approve,
        settings.clone(),
    ));
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

        // Before the model is called, never after. A recall run afterwards
        // would be a search for what the answer turned out to need, which
        // is a different thing from what the question asked for, and the
        // model would already have answered without it.
        let recalled = recall_into_turn(use_memory, &message, &tx, &seq);

        let outcome = run_agent(
            Ask {
                session_id: &session_id,
                message: &message,
                recalled,
            },
            Box::new(renderer),
            approver,
            Arc::clone(&cancel),
            &settings,
            own_port,
        );

        // Read after the run, not before, so a stop that lands during the
        // final moments of a turn is still reported as a stop.
        let stopped = cancel.load(std::sync::atomic::Ordering::SeqCst);
        // The final answer arrives in Outcome::Complete rather than through
        // the renderer. The CLI prints it in finish(); the browser has to be
        // sent it explicitly or the turn ends with activity and no reply.
        let kinds = closing_events(outcome, stopped);
        let mut next = seq.lock().unwrap();
        for kind in kinds {
            let _ = tx.send(Event { seq: *next, kind });
            *next += 1;
        }
        drop(next);
        session.lock().unwrap().running = false;

        // Name the conversation, if it still needs a name. Strictly after
        // the closing events: `Done` has already gone out, so the composer
        // is back and the reader has their answer before this costs
        // anything. It runs on its own thread, it writes to a column
        // nothing but the sidebar reads, and every way it can fail leaves
        // the sidebar showing the first message.
        crate::title::spawn_titling(
            session_id.clone(),
            settings.clone(),
            tx.clone(),
            Arc::clone(&seq),
        );

        // Queue only after the answer. The send is immediate and the one
        // indexer thread does the blocking work, so a missing local model
        // cannot slow or fail this turn. The periodic sweep is the catch-up
        // for a failed attempt and for changes made outside a turn.
        feed_recall(recall_indexer, session_id);
    });
}

/// Index the conversation that just finished.
///
/// Compiled away entirely without the feature, which is what keeps a build
/// that never opted into this from doing anything at all with the store.
#[cfg(feature = "recall")]
fn feed_recall(indexer: RecallFeed, session_id: String) {
    match indexer {
        Some(indexer) => indexer.index_session(session_id),
        // A router embedded without the process worker cannot promise
        // automatic indexing. The forced endpoint remains available, but
        // starting unmanaged per-turn workers here would break serialization.
        None => {}
    }
}

#[cfg(not(feature = "recall"))]
fn feed_recall(_indexer: RecallFeed, _session_id: String) {}

/// Look up what earlier conversations said about this message, tell the
/// browser what came back, and hand the text to the run.
///
/// Everything about the result reaches the browser first, including the
/// case where nothing was found and the case where no local embedder
/// answered. A recall the user cannot see is a model that knows things for
/// reasons nobody can check.
#[cfg(feature = "memory")]
fn recall_into_turn(
    use_memory: UseMemory,
    message: &str,
    tx: &std::sync::mpsc::Sender<Event>,
    seq: &Arc<Mutex<u64>>,
) -> Option<String> {
    if !use_memory {
        return None;
    }
    let (block, kind) = match crate::memory::recall_for(message, crate::memory::DEFAULT_PASSAGES) {
        Ok(found) => (
            found.block,
            EventKind::Memory {
                used: found.citations.iter().map(Into::into).collect(),
                unavailable: None,
            },
        ),
        // Not an error card, and not a silent fall through either. The
        // turn goes ahead without memory, because refusing to answer a
        // question over a search index being down is the wrong trade, and
        // the user is told in the server's own words that memory was asked
        // for and could not be used. Those words already name the missing
        // local embedder and say that nothing was sent anywhere.
        Err(e) => (
            None,
            EventKind::Memory {
                used: Vec::new(),
                unavailable: Some(e.to_string()),
            },
        ),
    };
    let mut next = seq.lock().unwrap();
    let _ = tx.send(Event { seq: *next, kind });
    *next += 1;
    drop(next);
    block
}

#[cfg(not(feature = "memory"))]
fn recall_into_turn(
    _use_memory: UseMemory,
    _message: &str,
    _tx: &std::sync::mpsc::Sender<Event>,
    _seq: &Arc<Mutex<u64>>,
) -> Option<String> {
    None
}

/// What this turn is about, as opposed to the machinery it runs on.
///
/// Grouped because the three travel together and because two of them are
/// strings: `run_agent(&id, &message, block, ...)` is a call site where
/// swapping two arguments compiles and then puts the session id in front of
/// the model.
struct Ask<'a> {
    session_id: &'a str,
    message: &'a str,
    /// Earlier conversations, framed and fenced, or `None` when this turn
    /// did not ask for any.
    recalled: Option<String>,
}

fn run_agent(
    ask: Ask<'_>,
    mut renderer: Box<dyn zorp_agent::Renderer>,
    approver: Arc<WebApprover>,
    cancel: zorp_agent::CancelToken,
    settings: &SettingsHandle,
    own_port: Option<u16>,
) -> Result<Outcome, String> {
    let Ask {
        session_id,
        message,
        recalled,
    } = ask;
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
    let budget = ContextBudget::from_env();

    // Persist the conversation the same way the CLI does, so the sidebar and
    // replay survive a restart, and seed this turn from what is already
    // there. Without a recorder the agent runs fine and remembers nothing,
    // which is what the first version did; with a recorder and no seed it
    // remembered nothing either, which is what the second version did.
    let mut seed: Option<SeedPlan> = None;
    let mut recorder: Option<Box<dyn zorp_agent::RunRecorder>> = None;
    if let Ok(store) = Store::open_default() {
        let seq = store.message_count(session_id).unwrap_or(0);
        if seq == 0 {
            let _ = store.create_session(session_id, message, &cwd_display, "");
        }
        seed = Some(seed_transcript(&store, session_id, &budget));
        recorder = Some(Box::new(SqliteRecorder::new(
            store,
            session_id.to_string(),
            seq,
            0,
        )));
    }

    // Recalled conversations go on the end of the seed, which is what keeps
    // them out of the store.
    //
    // `with_message_records` tells the agent how much of the transcript is
    // already persisted, and it counts what it is handed. So a record
    // appended here is one the agent believes it has already written, and
    // `sync` never offers it to the recorder. The model sees it; the
    // conversation does not keep it.
    //
    // That is not a tidiness argument. A block written into the store would
    // be embedded by the next feed and recalled by the turn after that, and
    // the harness's own framing of somebody else's text would become a
    // thing the corpus says. Growing your own evidence is the failure this
    // whole feature is arranged around.
    //
    // A `user` message, because that is the least trusted role a provider
    // will accept in a transcript and this is the least trusted text in the
    // request. Never `system`: the one channel the harness speaks in is the
    // one channel recalled text must never occupy.
    if let (Some(plan), Some(block)) = (seed.as_mut(), recalled) {
        plan.records.push(zorp_agent::Message::user(block).into());
    }

    // Say what compaction took before the turn starts, not after, so it reads
    // as a fact about this request rather than as commentary on the answer.
    if let Some(plan) = &seed {
        if let Some(text) = plan.report.notice() {
            renderer.notice(&text);
        }
        let messages: Vec<zorp_agent::Message> =
            plan.records.iter().map(|r| r.message.clone()).collect();
        renderer.context(&zorp_agent::ContextUsage {
            used_tokens: zorp_agent::estimate_tokens(&messages),
            source: zorp_agent::UsageSource::Estimated,
            limit_tokens: budget.limit_tokens,
        });
    }

    // The same token the session holds. The agent checks it between steps and
    // around every tool call, and hands it to the tool context, whose sandbox
    // kills the process group of a command that is already running. That is
    // what makes stop mean stopped rather than looked away.
    let mut agent = Agent::new(
        Box::new(model),
        system_prompt(),
        steps,
        cwd,
        cancel,
        ApprovalMode::Interactive(approver),
    )
    .with_context_budget(budget)
    .register_builtins_filtered(None)
    .with_renderer(renderer);

    agent = agent.with_policy(policy(own_port));

    // The seed replaces the transcript wholesale, so it has to land before
    // the recorder: `with_message_records` sets how much the agent believes
    // is already persisted, and the recorder's own counter starts from what
    // the store actually holds. Attaching the recorder first would leave the
    // agent recording the whole replayed history a second time.
    if let Some(plan) = seed {
        agent = agent.with_message_records(plan.records);
    }
    if let Some(recorder) = recorder {
        agent = agent.with_recorder(recorder);
    }

    Ok(agent.run(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorp_agent::{Message, ToolCall};

    /// A store on disk in a fresh directory, so tests never touch the
    /// developer's real session database.
    fn temp_store(name: &str) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "zorp-web-turn-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");
        let store = Store::open_at(&path).unwrap();
        (store, dir)
    }

    fn record_all(store: &mut Store, id: &str, messages: &[Message]) {
        store.create_session(id, "a task", "/tmp", "m").unwrap();
        for (seq, m) in messages.iter().enumerate() {
            store.record_message(id, seq as i64, m).unwrap();
        }
    }

    fn texts(plan: &SeedPlan) -> Vec<String> {
        plan.records
            .iter()
            .map(|r| r.message.text().into_owned())
            .collect()
    }

    /// The bug, as a test.
    ///
    /// A user asked zorp to convert a file, it did, and the very next turn it
    /// said "I haven't converted any file. This is the start of our session."
    /// A turn must see the conversation that came before it.
    #[test]
    fn a_turn_sees_the_previous_turns_of_its_session() {
        let (mut store, dir) = temp_store("history");
        record_all(
            &mut store,
            "s1",
            &[
                Message::system("an older prompt"),
                Message::user("convert notes.md to notes.pdf with pandoc"),
                Message::assistant("Converted notes.md to notes.pdf."),
            ],
        );

        let plan = seed_transcript(&store, "s1", &ContextBudget::default());

        let seen = texts(&plan);
        assert!(
            seen.iter()
                .any(|t| t.contains("convert notes.md to notes.pdf")),
            "the turn cannot see what the user asked for last time: {seen:?}"
        );
        assert!(
            seen.iter().any(|t| t.contains("Converted notes.md")),
            "the turn cannot see what it answered last time: {seen:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The prompt is the harness's to set. A session recorded before a prompt
    /// change must not keep re-sending the old one, and the several stored
    /// copies the server used to write must collapse back to one.
    #[test]
    fn a_seeded_turn_leads_with_the_current_system_prompt_once() {
        let (mut store, dir) = temp_store("prompt");
        record_all(
            &mut store,
            "s1",
            &[
                Message::system("a careful assistant"),
                Message::user("hello"),
                Message::assistant("hi"),
                Message::system("a careful assistant"),
                Message::user("still there?"),
            ],
        );

        let plan = seed_transcript(&store, "s1", &ContextBudget::default());

        let roles: Vec<&str> = plan
            .records
            .iter()
            .map(|r| r.message.role.as_str())
            .collect();
        assert_eq!(roles, vec!["system", "user", "assistant", "user"]);
        assert_eq!(plan.records[0].message.text(), system_prompt());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A stored transcript can be cut off between an assistant turn that
    /// announced a tool call and the result of that call: kill the server at
    /// the wrong moment and that is exactly what is on disk. Replayed
    /// unrepaired, providers reject the whole request, so reopening the
    /// session would fail with nothing on screen to explain it.
    #[test]
    fn a_seeded_turn_never_replays_a_dangling_tool_call() {
        let (mut store, dir) = temp_store("dangling");
        record_all(
            &mut store,
            "s1",
            &[
                Message::system("prompt"),
                Message::user("convert notes.md"),
                Message::assistant_with_calls(
                    "running pandoc",
                    vec![ToolCall {
                        id: "call_1".into(),
                        name: "run_command".into(),
                        arguments: serde_json::json!({"command": "pandoc notes.md"}),
                    }],
                ),
            ],
        );

        let plan = seed_transcript(&store, "s1", &ContextBudget::default());

        let messages: Vec<Message> = plan.records.iter().map(|r| r.message.clone()).collect();
        let announced: Vec<&str> = messages
            .iter()
            .flat_map(|m| m.tool_calls.iter().map(|c| c.id.as_str()))
            .collect();
        for id in announced {
            assert!(
                messages
                    .iter()
                    .any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some(id)),
                "tool call {id} has no result and the provider will refuse the request"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A session that has never run yet still gets a system message, and
    /// nothing else.
    #[test]
    fn a_brand_new_session_seeds_only_the_system_prompt() {
        let (store, dir) = temp_store("fresh");

        let plan = seed_transcript(&store, "unknown", &ContextBudget::default());

        assert_eq!(plan.records.len(), 1);
        assert_eq!(plan.records[0].message.role, "system");
        assert!(plan.report.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Compaction changes what is sent. It must never change what was said:
    /// the store is the durable record and `zorp-track` treats a record as
    /// evidence.
    #[test]
    fn seeding_never_rewrites_the_stored_record() {
        let (mut store, dir) = temp_store("record");
        let stored = vec![
            Message::system("prompt"),
            Message::user("old question ".repeat(80)),
            Message::assistant_with_calls(
                "working",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path": "a"}),
                }],
            ),
            Message::tool_result("c1", "a very long tool result ".repeat(200)),
            Message::assistant("old answer"),
            Message::user("the newest question"),
        ];
        record_all(&mut store, "s1", &stored);
        let before = store.load_messages("s1").unwrap();

        let plan = seed_transcript(
            &store,
            "s1",
            &ContextBudget::default().with_limit(Some(500)),
        );

        assert!(
            !plan.report.is_empty(),
            "this case is meant to force compaction"
        );
        assert_eq!(
            store.load_messages("s1").unwrap(),
            before,
            "compaction rewrote the durable transcript"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

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
        let kinds = closing_events(Err("the model went away".to_string()), false);
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
    /// such as a step limit.
    #[test]
    fn an_outcome_that_is_not_an_answer_still_ends_the_turn() {
        let kinds = closing_events(Ok(Outcome::StepLimit), false);
        assert!(
            matches!(kinds.last(), Some(EventKind::Done)),
            "a turn that hit the step limit never ended: {kinds:?}"
        );
    }

    /// A stopped turn is still a turn that ended.
    ///
    /// The same property as `a_failed_turn_still_ends_the_turn`, for the
    /// button a user presses on purpose. The browser leaves "running" on
    /// `Done` and on nothing else.
    #[test]
    fn a_stopped_turn_still_ends_the_turn() {
        let kinds = closing_events(Ok(Outcome::Cancelled), true);
        assert!(
            matches!(kinds.last(), Some(EventKind::Done)),
            "a stopped turn never ended: {kinds:?}"
        );
    }

    /// Pressing stop is not a failure. The transcript has to say which of the
    /// two happened, because "cancelled" in an error card reads like the run
    /// fell over on its own.
    #[test]
    fn a_stopped_turn_says_it_was_stopped_rather_than_that_it_failed() {
        let kinds = closing_events(Ok(Outcome::Cancelled), true);
        assert!(
            kinds.iter().any(|k| matches!(k, EventKind::Stopped)),
            "nothing in the transcript says the turn was stopped: {kinds:?}"
        );
        assert!(
            !kinds.iter().any(|k| matches!(k, EventKind::Error { .. })),
            "a deliberate stop was reported as a failure: {kinds:?}"
        );
    }

    /// A run that broke and was then stopped still has to report the breakage.
    /// The stop explains why nothing was retried; it does not explain the
    /// error.
    #[test]
    fn a_stop_does_not_swallow_a_real_failure() {
        let kinds = closing_events(Err("the model went away".to_string()), true);
        assert!(
            kinds.iter().any(|k| matches!(k, EventKind::Error { .. })),
            "the failure was swallowed by the stop: {kinds:?}"
        );
        assert!(
            kinds.iter().any(|k| matches!(k, EventKind::Stopped)),
            "{kinds:?}"
        );
        assert!(matches!(kinds.last(), Some(EventKind::Done)), "{kinds:?}");
    }

    /// A turn nobody stopped must not claim it was stopped. This is the
    /// property that keeps the flag honest when a run ends on its own a
    /// moment before the button is pressed.
    #[test]
    fn a_turn_that_finished_on_its_own_is_not_reported_as_stopped() {
        let kinds = closing_events(Ok(Outcome::Complete("the answer".into())), false);
        assert!(
            !kinds.iter().any(|k| matches!(k, EventKind::Stopped)),
            "{kinds:?}"
        );
    }

    #[test]
    fn a_successful_turn_sends_its_answer_and_then_ends() {
        let kinds = closing_events(Ok(Outcome::Complete("the answer".into())), false);
        assert!(
            matches!(&kinds[0], EventKind::Assistant { text } if text == "the answer"),
            "{kinds:?}"
        );
        assert!(matches!(kinds.last(), Some(EventKind::Done)), "{kinds:?}");
    }

    /// An empty answer is not worth a bubble, but the turn still ended.
    #[test]
    fn an_empty_answer_ends_the_turn_without_an_empty_message() {
        let kinds = closing_events(Ok(Outcome::Complete("   ".into())), false);
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, EventKind::Assistant { .. })),
            "{kinds:?}"
        );
        assert!(matches!(kinds.last(), Some(EventKind::Done)), "{kinds:?}");
    }
}
