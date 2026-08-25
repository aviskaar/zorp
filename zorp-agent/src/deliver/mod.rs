mod error;

pub use error::DeliverError;

use crate::agent::{Agent, Outcome};
use crate::sanitize::{sanitize, SanitizeMode};
use std::fmt::Write as _;
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

fn has_huiban_tool(agent: &Agent) -> bool {
    agent
        .tool_names()
        .iter()
        .any(|n| n.starts_with("mcp__huiban__"))
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

/// Count how many candidates a shortlist names. Only heading and list
/// item lines count: blank lines and prose would inflate a raw line
/// count and tell the human nothing useful.
fn candidate_count(shortlist: &str) -> usize {
    shortlist
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("## ") || t.starts_with("- ")
        })
        .count()
}

/// Run deliver for a track that already has a co-written draft: find
/// real venues via huiban, write a ranked shortlist to `venues.md`, and
/// checkpoint it. Returns whether the checkpoint was approved. Like
/// `co_write::run`, neither outcome changes the track's status.
///
/// `sanitize_mode` decides how much of the text sanitization pass runs
/// over the shortlist before it is written. Whatever it changes is
/// reported on stderr and in the checkpoint prompt.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    checkpoint_mode: &CheckpointMode,
    sanitize_mode: SanitizeMode,
) -> Result<bool, DeliverError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(DeliverError::TrackKilled);
    }

    let draft_path = project.track_dir(track_id).join("draft.md");
    let draft = match std::fs::read_to_string(&draft_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(DeliverError::NoDraft),
        Err(e) => return Err(e.into()),
    };

    if !has_huiban_tool(agent) {
        return Err(DeliverError::NoVenueTool);
    }

    let task = build_task_prompt(hypothesis, &draft);
    let outcome = agent.run(&task);
    let raw = match outcome {
        Outcome::Complete(text) => text,
        other => return Err(DeliverError::AgentOutcome(other.describe())),
    };

    let cleaned = sanitize(&raw, sanitize_mode);
    let shortlist = cleaned.text;
    let sanitize_note = cleaned.report.summary();
    if let Some(note) = &sanitize_note {
        eprintln!("zorp-agent: deliver sanitized the shortlist: {note}");
    }

    let track_dir = project.track_dir(track_id);
    std::fs::create_dir_all(&track_dir)?;
    let venues_path = track_dir.join("venues.md");
    std::fs::write(&venues_path, &shortlist)?;

    let mut prompt = format!(
        "deliver: shortlist written to {} ({} candidates",
        venues_path.display(),
        candidate_count(&shortlist)
    );
    // The checkpoint is where a human decides on this shortlist, so it is
    // where they have to be told the text is not verbatim what the model
    // produced.
    if let Some(note) = &sanitize_note {
        let _ = write!(prompt, ", sanitized: {note}");
    }
    prompt.push_str("). Ready for review?");
    let approved =
        project
            .store
            .record_checkpoint(track_id, "deliver", checkpoint_mode, &prompt)?;

    Ok(approved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssistantMessage, Message, Model};
    use crate::BoxErr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    struct StubModel {
        response: String,
        calls: Arc<AtomicUsize>,
    }

    impl Model for StubModel {
        fn complete(
            &self,
            _messages: &[Message],
            _tools: &[serde_json::Value],
        ) -> Result<AssistantMessage, BoxErr> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(AssistantMessage {
                content: self.response.clone(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }

        fn clone_box(&self) -> Box<dyn Model> {
            Box::new(StubModel {
                response: self.response.clone(),
                calls: self.calls.clone(),
            })
        }
    }

    fn build_agent(response: &str) -> Agent {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel {
            response: response.to_string(),
            calls,
        };
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
        project
            .store
            .create_track(track_id, "does caching help")
            .unwrap();
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
        project
            .store
            .set_track_status("t1", TrackStatus::Killed)
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Full,
        )
        .unwrap_err();
        assert!(matches!(err, DeliverError::TrackKilled));
    }

    #[test]
    fn no_draft_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Full,
        )
        .unwrap_err();
        assert!(matches!(err, DeliverError::NoDraft));
    }

    struct FakeHuibanTool;

    impl crate::tools::Tool for FakeHuibanTool {
        fn name(&self) -> &str {
            "mcp__huiban__search"
        }
        fn description(&self) -> &str {
            "fake venue search"
        }
        fn schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        fn run(
            &self,
            _args: &serde_json::Value,
            _cx: &mut crate::tools::Context,
        ) -> crate::tools::ToolResult {
            Ok(crate::tools::ToolOutput::new("no venues", "no venues"))
        }
    }

    /// Captures the prompt string it was handed so a test can assert on
    /// what the human would actually have been asked.
    struct CapturingDecider {
        prompt: Arc<Mutex<Option<String>>>,
    }

    impl zorp_track::checkpoint::Decider for CapturingDecider {
        fn decide(&self, prompt: &str) -> bool {
            *self.prompt.lock().unwrap() = Some(prompt.to_string());
            true
        }
    }

    #[test]
    fn checkpoint_prompt_counts_candidates_not_lines() {
        let shortlist = "Here is a short preamble.\n\nIt runs over several lines\nof plain prose that names no venue.\n\n## Example Systems Conference\n\ndeadline 2026-12-01\n\n- Journal of Caching\n- Journal of Latency\n";
        // 3 candidates (one `## ` heading, two `- ` items) out of 11 lines.
        let mut agent = build_agent(shortlist).register(Box::new(FakeHuibanTool));
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        let captured = Arc::new(Mutex::new(None));
        let mode = CheckpointMode::Interactive(Arc::new(CapturingDecider {
            prompt: captured.clone(),
        }));

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Full,
        )
        .unwrap();

        let prompt = captured
            .lock()
            .unwrap()
            .clone()
            .expect("decider should have been asked");
        assert!(
            prompt.contains("(3 candidates)"),
            "prompt should report 3 candidates, got: {prompt}"
        );
        assert!(
            !prompt.contains("11"),
            "prompt should not report the raw line count, got: {prompt}"
        );
    }

    #[test]
    fn unreadable_draft_is_an_io_error_not_no_draft() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        // A directory where draft.md should be: reading it fails with
        // something other than NotFound.
        std::fs::create_dir_all(project.track_dir("t1").join("draft.md")).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Full,
        )
        .unwrap_err();
        assert!(
            matches!(err, DeliverError::Io(_)),
            "expected Io, got {err:?}"
        );
    }

    #[test]
    fn the_shortlist_is_sanitized_before_it_is_written() {
        let shortlist =
            "## Venue\u{200B} One \u{2014} \u{201C}systems\u{201D}\n- Journal of\u{202E} Caching\n";
        let mut agent = build_agent(shortlist).register(Box::new(FakeHuibanTool));
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Full,
        )
        .unwrap();

        let content = std::fs::read_to_string(project.track_dir("t1").join("venues.md")).unwrap();
        assert_eq!(content, "## Venue One, \"systems\"\n- Journal of Caching\n");
    }

    #[test]
    fn sanitize_off_writes_the_shortlist_verbatim() {
        let shortlist = "## Venue\u{200B} One \u{2014} \u{201C}systems\u{201D}\n";
        let mut agent = build_agent(shortlist).register(Box::new(FakeHuibanTool));
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Off,
        )
        .unwrap();

        let content = std::fs::read_to_string(project.track_dir("t1").join("venues.md")).unwrap();
        assert_eq!(content, shortlist);
    }

    #[test]
    fn no_huiban_tool_is_refused() {
        let mut agent = build_agent("a shortlist");
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1");
        let mode = CheckpointMode::terminal(true).unwrap();
        // No MCP tools attached: only built-in local tools are present.

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            &mode,
            SanitizeMode::Full,
        )
        .unwrap_err();
        assert!(matches!(err, DeliverError::NoVenueTool));
    }
}
