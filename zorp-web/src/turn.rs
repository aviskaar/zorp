use crate::approval::WebApprover;
use crate::event::{Event, EventKind};
use crate::renderer::WebRenderer;
use crate::state::SessionState;
use std::sync::{Arc, Mutex};
use zorp_agent::{cancel_token, Agent, ApprovalMode, HttpModel, Outcome};

/// Run one turn to completion on a blocking thread.
///
/// The agent loop is synchronous, so it must not run on the async runtime.
/// Events are drained from the renderer's channel into the session backlog as
/// they arrive, which is what lets the SSE endpoint stream a run that is still
/// in progress.
pub fn spawn_turn(session: Arc<Mutex<SessionState>>, message: String) {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let seq = Arc::new(Mutex::new(0u64));
    let approver = Arc::new(WebApprover::new(tx.clone(), Arc::clone(&seq)));

    {
        let mut guard = session.lock().unwrap();
        guard.running = true;
        guard.approver = Some(Arc::clone(&approver));
    }

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

        let outcome = run_agent(&message, Box::new(renderer), approver);

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

    Ok(agent.run(message))
}
