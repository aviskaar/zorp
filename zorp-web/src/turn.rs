use crate::approval::WebApprover;
use crate::event::{Event, EventKind};
use crate::renderer::WebRenderer;
use crate::state::SessionState;
use std::sync::{Arc, Mutex};
use zorp_agent::{cancel_token, Agent, ApprovalMode, HttpModel, Outcome, SqliteRecorder, Store};

/// Run one turn to completion on a blocking thread.
///
/// The agent loop is synchronous, so it must not run on the async runtime.
/// Events are drained from the renderer's channel into the session backlog as
/// they arrive, which is what lets the SSE endpoint stream a run that is still
/// in progress.
pub fn spawn_turn(session: Arc<Mutex<SessionState>>, session_id: String, message: String) {
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
            drain_session.lock().unwrap().backlog.push(event);
        }
    });

    std::thread::spawn(move || {
        let mut renderer = WebRenderer::new(tx.clone());
        // The renderer and the approver share one counter so approval
        // requests interleave correctly with activity.
        renderer.set_seq(Arc::clone(&seq));

        let outcome = run_agent(&session_id, &message, Box::new(renderer), approver);

        // The final answer arrives in Outcome::Complete rather than through
        // the renderer. The CLI prints it in finish(); the browser has to be
        // sent it explicitly or the turn ends with activity and no reply.
        let mut kinds = Vec::new();
        match outcome {
            Ok(Outcome::Complete(text)) => {
                if !text.trim().is_empty() {
                    kinds.push(EventKind::Assistant { text });
                }
                kinds.push(EventKind::Done);
            }
            Ok(other) => kinds.push(EventKind::Error {
                message: other.describe(),
            }),
            Err(message) => kinds.push(EventKind::Error { message }),
        }
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
) -> Result<Outcome, String> {
    let model = HttpModel::try_from_env().map_err(|e| e.to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let steps = std::env::var("ZORP_MAX_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let cwd_display = cwd.display().to_string();
    let mut agent = Agent::new(
        Box::new(model),
        "You are zorp, a careful assistant. Use tools when they help.",
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
