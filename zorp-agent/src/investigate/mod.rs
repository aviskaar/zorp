mod error;
mod result;

pub use error::InvestigateError;
pub use result::{parse_attempt_result, AttemptResult, ParseError};

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::{get_preregistration, write_prereg};
use zorp_track::track::TrackStatus;
use zorp_track::Project;

const TASK_PROMPT_PREFIX: &str = "\
Work the following hypothesis using whatever tools are available to you. \
When you're done, report the value of the metric named '";

const TASK_PROMPT_SUFFIX: &str = "\
' that your work produced.\n\n\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{\"metric_value\": <number>, \"summary\": \"<one sentence>\"}\n\
```\n\n\
Hypothesis: ";

/// Parameters a caller supplies only on the first `run` call for a
/// track, when no pre-registration exists yet.
pub struct PreregParams<'a> {
    pub metric_name: &'a str,
    pub kill_threshold: f64,
}

/// Run one investigate attempt for `track_id`. On the first call for a
/// track (no prereg on file), `prereg_params` must be `Some` and is
/// written, checkpointed, and (if approved) used for this same attempt.
/// On a later call, `prereg_params` is optional; if given, it must match
/// the recorded prereg exactly. Returns whether the post-attempt
/// checkpoint was approved (mirrors `validate::run`'s `Result<bool, _>`
/// shape); a rejected *prereg* checkpoint also returns `Ok(false)`, with
/// no attempt run.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    prereg_params: Option<PreregParams>,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, InvestigateError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(InvestigateError::TrackKilled);
    }

    let existing = get_preregistration(&project.store, track_id)?;
    let prereg = match (existing, prereg_params) {
        (Some(existing), None) => existing,
        (Some(existing), Some(params)) => {
            if existing.metric_name != params.metric_name {
                return Err(InvestigateError::PreregMismatch {
                    field: "metric-name",
                    recorded: existing.metric_name,
                    provided: params.metric_name.to_string(),
                });
            }
            if existing.kill_threshold != params.kill_threshold {
                return Err(InvestigateError::PreregMismatch {
                    field: "kill-threshold",
                    recorded: existing.kill_threshold.to_string(),
                    provided: params.kill_threshold.to_string(),
                });
            }
            existing
        }
        (None, None) => return Err(InvestigateError::PreregRequired { missing: "metric-name and --kill-threshold" }),
        (None, Some(params)) => {
            let track_dir = project.track_dir(track_id);
            let written = write_prereg(
                &project.store,
                &track_dir,
                track_id,
                hypothesis,
                params.metric_name,
                params.kill_threshold,
            )?;
            let prereg_prompt = format!(
                "investigate: pre-register metric '{}' with kill threshold {}. Hypothesis: {}\nProceed to run the first attempt?",
                written.metric_name, written.kill_threshold, hypothesis
            );
            let approved = project.store.record_checkpoint(track_id, "investigate-prereg", checkpoint_mode, &prereg_prompt)?;
            if !approved {
                project.store.set_track_status(track_id, TrackStatus::Killed)?;
                return Ok(false);
            }
            written
        }
    };

    let experiment = project.store.create_experiment(track_id, &prereg.id)?;
    project.store.set_experiment_status(&experiment.id, ExperimentStatus::Running)?;

    let task = format!("{TASK_PROMPT_PREFIX}{}{TASK_PROMPT_SUFFIX}{hypothesis}", prereg.metric_name);
    let outcome = agent.run(&task);
    let text = match outcome {
        Outcome::Complete(text) => text,
        Outcome::StepLimit => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("StepLimit".to_string()));
        }
        Outcome::VerificationFailed { attempts } => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome(format!("VerificationFailed after {attempts} attempts")));
        }
        Outcome::Cancelled => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("Cancelled".to_string()));
        }
        Outcome::RepeatedAction => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("RepeatedAction".to_string()));
        }
        Outcome::Blocked => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome("Blocked".to_string()));
        }
        Outcome::Error(e) => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome(format!("Error: {e}")));
        }
    };

    let attempt = match parse_attempt_result(&text) {
        Ok(a) => a,
        Err(e) => {
            project.store.set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(e.into());
        }
    };

    project.store.record_metric(&experiment.id, &prereg.metric_name, MetricValue::Number(attempt.metric_value))?;
    project.store.set_experiment_status(&experiment.id, ExperimentStatus::Completed)?;

    let prompt = format!(
        "investigate: {} = {} (kill threshold {}). {}\nKeep this track alive?",
        prereg.metric_name, attempt.metric_value, prereg.kill_threshold, attempt.summary
    );
    let approved = project.store.record_checkpoint(track_id, "investigate", checkpoint_mode, &prompt)?;
    if !approved {
        project.store.set_track_status(track_id, TrackStatus::Killed)?;
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
        "Done.\n```json\n{\"metric_value\": 42.0, \"summary\": \"worked\"}\n```\n".to_string()
    }

    fn build_agent(response: String) -> Agent {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = StubModel { response, calls };
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

    #[test]
    fn killed_track_is_refused_before_creating_an_experiment() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        project.store.set_track_status("t1", TrackStatus::Killed).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", None, &mode).unwrap_err();
        assert!(matches!(err, InvestigateError::TrackKilled));
    }

    #[test]
    fn missing_prereg_params_on_first_call_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", None, &mode).unwrap_err();
        assert!(matches!(err, InvestigateError::PreregRequired { .. }));
    }

    #[test]
    fn mismatched_prereg_params_on_a_later_call_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
            &mode,
        )
        .unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams { metric_name: "latency_ms", kill_threshold: 50.0 }),
            &mode,
        )
        .unwrap_err();
        assert!(matches!(err, InvestigateError::PreregMismatch { field: "kill-threshold", .. }));
    }

    #[test]
    fn first_call_writes_prereg_runs_attempt_and_records_metric() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "does caching help").unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let approved = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams { metric_name: "latency_ms", kill_threshold: 100.0 }),
            &mode,
        )
        .unwrap();
        assert!(approved);

        let prereg = get_preregistration(&project.store, "t1").unwrap().unwrap();
        assert_eq!(prereg.metric_name, "latency_ms");
    }
}
