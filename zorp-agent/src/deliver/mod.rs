mod error;

pub use error::DeliverError;

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

fn has_huiban_tool(agent: &Agent) -> bool {
    agent.tool_names().iter().any(|n| n.starts_with("mcp__huiban__"))
}

fn build_task_prompt(hypothesis: &str, draft: &str) -> String {
    format!(
        "Determine this draft's scope and contribution type, then use the \
         available huiban tools to search for real conferences and journals \
         that fit. Rank the candidates you find, including each one's \
         deadline and ranking (CCF/CORE) where available.\n\n\
         Hypothesis: {hypothesis}\n\n\
         Draft:\n{draft}"
    )
}

/// Run deliver for a track that already has a co-written draft: find
/// real venues via huiban, write a ranked shortlist to `venues.md`, and
/// checkpoint it. Returns whether the checkpoint was approved. Like
/// `co_write::run`, neither outcome changes the track's status.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, DeliverError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(DeliverError::TrackKilled);
    }

    let draft_path = project.track_dir(track_id).join("draft.md");
    let draft = std::fs::read_to_string(&draft_path).map_err(|_| DeliverError::NoDraft)?;

    if !has_huiban_tool(agent) {
        return Err(DeliverError::NoVenueTool);
    }

    let task = build_task_prompt(hypothesis, &draft);
    let outcome = agent.run(&task);
    let shortlist = match outcome {
        Outcome::Complete(text) => text,
        Outcome::StepLimit => return Err(DeliverError::AgentOutcome("StepLimit".to_string())),
        Outcome::VerificationFailed { attempts } => {
            return Err(DeliverError::AgentOutcome(format!("VerificationFailed after {attempts} attempts")))
        }
        Outcome::Cancelled => return Err(DeliverError::AgentOutcome("Cancelled".to_string())),
        Outcome::RepeatedAction => return Err(DeliverError::AgentOutcome("RepeatedAction".to_string())),
        Outcome::Blocked => return Err(DeliverError::AgentOutcome("Blocked".to_string())),
        Outcome::Error(e) => return Err(DeliverError::AgentOutcome(format!("Error: {e}"))),
    };

    let track_dir = project.track_dir(track_id);
    std::fs::create_dir_all(&track_dir)?;
    let venues_path = track_dir.join("venues.md");
    std::fs::write(&venues_path, &shortlist)?;

    let prompt = format!(
        "deliver: shortlist written to {} ({} lines). Ready for review?",
        venues_path.display(),
        shortlist.lines().count()
    );
    let approved = project.store.record_checkpoint(track_id, "deliver", checkpoint_mode, &prompt)?;

    Ok(approved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssistantMessage, Message, Model};
    use crate::BoxErr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

    struct StubModel {
        response: String,
        calls: Arc<AtomicUsize>,
    }

    impl Model for StubModel {
        fn complete(&self, _messages: &[Message], _tools: &[serde_json::Value]) -> Result<AssistantMessage, BoxErr> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(AssistantMessage {
                content: self.response.clone(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }

        fn clone_box(&self) -> Box<dyn Model> {
            Box::new(StubModel { response: self.response.clone(), calls: self.calls.clone() })
        }
    }

    fn build_agent(response: &str) -> Agent {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel { response: response.to_string(), calls };
        Agent::new(
            Box::new(model),
            "system",
            5,
            std::env::temp_dir(),
            crate::cancel_token(),
            crate::ApprovalMode::AutoApprove,
        )
        .register_builtins()
    }

    fn track_with_draft(project: &Project, track_id: &str) {
        project.store.create_track(track_id, "does caching help").unwrap();
        let track_dir = project.track_dir(track_id);
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(track_dir.join("draft.md"), "# Draft\n\nLatency improved.").unwrap();
    }

    #[test]
    fn killed_track_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        project.store.set_track_status("t1", TrackStatus::Killed).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, DeliverError::TrackKilled));
    }

    #[test]
    fn no_draft_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, DeliverError::NoDraft));
    }

    #[test]
    fn no_huiban_tool_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        let mode = CheckpointMode::terminal(true).unwrap();
        // No MCP tools attached: only built-in local tools are present.

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, DeliverError::NoVenueTool));
    }
}
