mod error;
mod result;

pub use error::ValidateError;
pub use result::{parse_validation_result, ParseError, ValidationResult};

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::Project;

const TASK_PROMPT_PREFIX: &str = "\
Research the following question using whatever tools are available to you. \
Determine two things: (1) redundancy, has this question already been \
answered with enough confidence by something you found (a settled best \
practice, an existing analysis, a prior benchmark)? (2) feasibility, can \
this question actually be investigated further given what you found? \
Score each 0 to 100. Every score above 0 must be backed by at least one \
citation to something you actually found; a score with no citation is \
invalid. Cite the search result or source you're relying on for each \
claim.\n\n\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{\"redundancy_score\": <number>, \"redundancy_citations\": [{\"text\": \"<what it says>\", \"source\": \"<where it came from>\"}], \
\"feasibility_score\": <number>, \"feasibility_citations\": [...], \"verdict\": \"<one sentence>\"}\n\
```\n\n\
Question: ";

fn has_search_tool(agent: &Agent) -> bool {
    agent.tool_names().iter().any(|n| n.starts_with("mcp__"))
}

/// Run validate for an already-created track: search, score, embed
/// cited sources, record the validation, and checkpoint. Returns
/// whether the checkpoint was approved.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    question: &str,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, ValidateError> {
    if !has_search_tool(agent) {
        return Err(ValidateError::NoSearchTool);
    }

    let task = format!("{TASK_PROMPT_PREFIX}{question}");
    let outcome = agent.run(&task);
    let text = match outcome {
        Outcome::Complete(text) => text,
        Outcome::StepLimit => return Err(ValidateError::AgentOutcome("StepLimit".to_string())),
        Outcome::VerificationFailed { attempts } => {
            return Err(ValidateError::AgentOutcome(format!("VerificationFailed after {attempts} attempts")))
        }
        Outcome::Cancelled => return Err(ValidateError::AgentOutcome("Cancelled".to_string())),
        Outcome::RepeatedAction => return Err(ValidateError::AgentOutcome("RepeatedAction".to_string())),
        Outcome::Blocked => return Err(ValidateError::AgentOutcome("Blocked".to_string())),
        Outcome::Error(e) => return Err(ValidateError::AgentOutcome(format!("Error: {e}"))),
    };

    let result = parse_validation_result(&text)?;

    for citation in result.redundancy_citations.iter().chain(result.feasibility_citations.iter()) {
        let embedding = crate::embed_texts(&[citation.text.clone()])
            .map_err(ValidateError::Embedding)?
            .into_iter()
            .next()
            .ok_or_else(|| ValidateError::Embedding("no embedding returned".to_string()))?;
        project
            .library
            .insert_source(track_id, "validate-source", &citation.text, &citation.source, &embedding)?;
    }

    project.store.record_validation(
        track_id,
        result.redundancy_score,
        &result.redundancy_citations,
        result.feasibility_score,
        &result.feasibility_citations,
        &result.verdict,
    )?;

    let prompt = format!(
        "validate: redundancy {:.0}/100, feasibility {:.0}/100. {}\nProceed to investigate?",
        result.redundancy_score, result.feasibility_score, result.verdict
    );
    let approved = project.store.record_checkpoint(track_id, "validate", checkpoint_mode, &prompt)?;
    if !approved {
        project.store.set_track_status(track_id, zorp_track::track::TrackStatus::Killed)?;
    }

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

    fn well_formed_response() -> String {
        "Findings below.\n```json\n{\"redundancy_score\": 10.0, \"redundancy_citations\": [{\"text\": \"nothing directly on point\", \"source\": \"search\"}], \"feasibility_score\": 90.0, \"feasibility_citations\": [{\"text\": \"tools are available\", \"source\": \"search\"}], \"verdict\": \"worth investigating\"}\n```\n".to_string()
    }

    #[test]
    fn no_search_tool_errors_before_calling_the_model() {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel { response: well_formed_response(), calls: calls.clone() };
        let mut agent = Agent::new(
            Box::new(model),
            "system",
            5,
            std::env::temp_dir(),
            crate::cancel_token(),
            crate::ApprovalMode::AutoApprove,
        )
        .register_builtins();
        // No MCP tools attached: only built-in local tools are present.

        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
        assert!(matches!(err, ValidateError::NoSearchTool));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
