mod error;
pub mod forecast;
mod result;

pub use error::InvestigateError;
pub use forecast::{parse_forecast, Forecast, ForecastError};
pub use result::{parse_attempt_result, AttemptResult, ParseError};

use crate::agent::{Agent, Outcome};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::{get_preregistration, write_prereg, ThresholdDirection};
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
    pub threshold_direction: ThresholdDirection,
}

/// Run one investigate attempt for `track_id`. On the first call for a
/// track (no prereg on file), `prereg_params` must be `Some` and is
/// written, checkpointed, and (if approved) used for this same attempt.
/// On a later call, `prereg_params` is optional; if given, it must match
/// the recorded prereg exactly. Returns whether the post-attempt
/// checkpoint was approved (mirrors `validate::run`'s `Result<bool, _>`
/// shape); a rejected *prereg* checkpoint also returns `Ok(false)`, with
/// no attempt run. A recorded metric that breaches the pre-registered
/// kill threshold kills the track and returns `Ok(false)` without
/// consulting the checkpoint mode at all; auto-approve cannot skip it.
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
            if existing.hypothesis_snapshot != hypothesis {
                return Err(InvestigateError::PreregMismatch {
                    field: "hypothesis",
                    recorded: existing.hypothesis_snapshot.clone(),
                    provided: hypothesis.to_string(),
                });
            }
            if existing.kill_threshold != params.kill_threshold {
                return Err(InvestigateError::PreregMismatch {
                    field: "kill-threshold",
                    recorded: existing.kill_threshold.to_string(),
                    provided: params.kill_threshold.to_string(),
                });
            }
            if existing.threshold_direction != Some(params.threshold_direction) {
                return Err(InvestigateError::PreregMismatch {
                    field: "threshold-direction",
                    recorded: existing
                        .threshold_direction
                        .map(|d| d.as_str().to_string())
                        .unwrap_or_else(|| "none (legacy pre-registration)".to_string()),
                    provided: params.threshold_direction.as_str().to_string(),
                });
            }
            existing
        }
        (None, None) => {
            return Err(InvestigateError::PreregRequired {
                missing: "metric-name, --kill-threshold, and --threshold-direction",
            })
        }
        (None, Some(params)) => {
            let track_dir = project.track_dir(track_id);
            let written = write_prereg(
                &project.store,
                &track_dir,
                track_id,
                hypothesis,
                params.metric_name,
                params.kill_threshold,
                params.threshold_direction,
            )?;
            let prereg_prompt = format!(
                "investigate: pre-register metric '{}' with kill threshold {} ({}). Hypothesis: {}\nProceed to run the first attempt?",
                written.metric_name,
                written.kill_threshold,
                params.threshold_direction.as_str(),
                hypothesis
            );
            let approved = project.store.record_checkpoint(
                track_id,
                "investigate-prereg",
                checkpoint_mode,
                &prereg_prompt,
            )?;
            if !approved {
                project
                    .store
                    .set_track_status(track_id, TrackStatus::Killed)?;
                return Ok(false);
            }
            written
        }
    };

    let experiment = project.store.create_experiment(track_id, &prereg.id)?;

    // aryabhatta, before the work starts. Both of these have to happen
    // here or they are worthless: conditions describe what the run was
    // performed under, and an expectation recorded after the outcome is
    // a postdiction that `record_expectation` would refuse anyway.
    record_conditions(agent, project, &experiment.id, checkpoint_mode)?;
    // A skipped forecast is said out loud. Silence here would look
    // exactly like a forecast that was made, and the difference decides
    // whether this experiment is ever scored.
    if let ForecastOutcome::Skipped(why) = record_forecast(
        agent,
        project,
        &experiment.id,
        hypothesis,
        &prereg.metric_name,
    ) {
        eprintln!(
            "zorp-agent: WARNING: no forecast recorded for this attempt ({why}); \
             it will not be scored by the calibration report"
        );
    }

    project
        .store
        .set_experiment_status(&experiment.id, ExperimentStatus::Running)?;

    let task = format!(
        "{TASK_PROMPT_PREFIX}{}{TASK_PROMPT_SUFFIX}{hypothesis}",
        prereg.metric_name
    );
    let outcome = agent.run(&task);
    let text = match outcome {
        Outcome::Complete(text) => text,
        other => {
            project
                .store
                .set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(InvestigateError::AgentOutcome(other.describe()));
        }
    };

    let attempt = match parse_attempt_result(&text) {
        Ok(a) => a,
        Err(e) => {
            project
                .store
                .set_experiment_status(&experiment.id, ExperimentStatus::Failed)?;
            return Err(e.into());
        }
    };

    project.store.record_metric(
        &experiment.id,
        &prereg.metric_name,
        MetricValue::Number(attempt.metric_value),
    )?;
    project
        .store
        .set_experiment_status(&experiment.id, ExperimentStatus::Completed)?;

    // Enforce the pre-registered kill threshold. This is not a
    // checkpoint: a breach kills the track unconditionally, so
    // AutoApprove (--yes) cannot wave it through. Only a legacy
    // pre-registration with no recorded direction escapes enforcement,
    // and loudly, because guessing a direction could kill a healthy
    // track or spare a doomed one.
    match prereg.threshold_direction {
        Some(direction) if direction.breached(attempt.metric_value, prereg.kill_threshold) => {
            let over_or_under = match direction {
                ThresholdDirection::LowerIsBetter => "above",
                ThresholdDirection::HigherIsBetter => "below",
            };
            let reason = format!(
                "kill threshold breached: metric '{}' = {} went {} threshold {} ({})",
                prereg.metric_name,
                attempt.metric_value,
                over_or_under,
                prereg.kill_threshold,
                direction.as_str()
            );
            project
                .store
                .record_enforced_kill(track_id, "investigate-threshold", &reason)?;
            eprintln!("zorp-agent: {reason}; track killed");
            return Ok(false);
        }
        Some(_) => {}
        None => {
            eprintln!(
                "zorp-agent: WARNING: pre-registration for track '{track_id}' records no threshold direction; \
                 kill threshold {} is NOT being enforced for this attempt",
                prereg.kill_threshold
            );
        }
    }

    let prompt = format!(
        "investigate: {} = {} (kill threshold {}). {}\nHypothesis: {}\nKeep this track alive?",
        prereg.metric_name,
        attempt.metric_value,
        prereg.kill_threshold,
        attempt.summary,
        hypothesis
    );
    let approved =
        project
            .store
            .record_checkpoint(track_id, "investigate", checkpoint_mode, &prompt)?;
    if !approved {
        project
            .store
            .set_track_status(track_id, TrackStatus::Killed)?;
    }

    Ok(approved)
}

/// Whether the forecast step runs, and what it did.
///
/// Reported rather than returned, because nothing downstream branches
/// on it. It exists so a caller can see that a forecast was skipped
/// instead of silently getting a record with no expectations in it.
#[derive(Debug, Clone, PartialEq)]
pub enum ForecastOutcome {
    /// `ZORP_FORECAST` was not set, so no forecast was asked for.
    Disabled,
    /// A forecast was recorded, and the experiment can now be scored.
    Recorded,
    /// A forecast was asked for and could not be used. The attempt still
    /// runs: an experiment with no expectation is one the calibration
    /// report does not score, which is a smaller loss than an
    /// investigation that refuses to proceed.
    Skipped(String),
}

/// The environment variable that turns forecasting on.
///
/// Off by default, and deliberately so. A forecast costs an extra model
/// call on every attempt, and the adjacent evidence in the design says
/// the calibration it feeds is more likely to fail than pass. Making
/// every run pay for that without being asked would be the wrong
/// default. The engine is inert until someone opts in, and the opt-in
/// is what makes the record worth reading.
pub const FORECAST_ENV: &str = "ZORP_FORECAST";

fn forecasting_enabled() -> bool {
    matches!(
        std::env::var(FORECAST_ENV).ok().as_deref(),
        Some("1") | Some("true")
    )
}

/// Record what this run was performed under.
///
/// Only facts the harness observes. Nothing the model wrote goes in
/// here: `conditions` is read by the boredom detectors and by the
/// confounding search, and integrity rule 5 exists because a condition
/// carrying model prose turns the agent's own speculation into
/// tomorrow's observation. The hypothesis in particular is deliberately
/// absent, and it is the tempting one.
///
/// Values pinned by the pre-registration are also absent, for a
/// different reason. `metric_name`, `kill_threshold` and
/// `threshold_direction` cannot vary within a track by construction, so
/// recording them would make the invariant-condition detector fire on
/// every track forever while telling nobody anything.
fn record_conditions(
    agent: &Agent,
    project: &Project,
    experiment_id: &str,
    checkpoint_mode: &CheckpointMode,
) -> Result<(), InvestigateError> {
    if let Some(model) = agent.model().identity() {
        project.store.record_condition(
            experiment_id,
            "model",
            &MetricValue::Text(model.to_string()),
        )?;
    }
    project.store.record_condition(
        experiment_id,
        "checkpoint_mode",
        &MetricValue::Text(
            match checkpoint_mode {
                CheckpointMode::AutoApprove => "auto-approve",
                CheckpointMode::Interactive(_) => "interactive",
            }
            .to_string(),
        ),
    )?;
    Ok(())
}

/// Ask for a forecast and record it, before the experiment runs.
///
/// Never fails the attempt. A forecast is the thing that makes an
/// anomaly possible later, not a precondition for doing the work, and
/// an investigation that dies because a model wrote a malformed JSON
/// block would be a worse harness than one that records no expectation.
fn record_forecast(
    agent: &Agent,
    project: &Project,
    experiment_id: &str,
    hypothesis: &str,
    metric_name: &str,
) -> ForecastOutcome {
    if !forecasting_enabled() {
        return ForecastOutcome::Disabled;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let forecast = match forecast::ask(agent.model(), hypothesis, metric_name, cwd) {
        Ok(Some(f)) => f,
        Ok(None) => return ForecastOutcome::Skipped("the forecaster did not answer".to_string()),
        Err(e) => return ForecastOutcome::Skipped(e.to_string()),
    };
    match project.store.record_expectation(
        experiment_id,
        metric_name,
        forecast.expected_value,
        forecast.interval_low,
        forecast.interval_high,
        forecast.confidence,
        &[],
    ) {
        Ok(_) => ForecastOutcome::Recorded,
        // Includes the refusal that matters: if a metric under this key
        // somehow already exists, the store refuses, and that refusal is
        // reported rather than worked around.
        Err(e) => ForecastOutcome::Skipped(e.to_string()),
    }
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
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        project
            .store
            .set_track_status("t1", TrackStatus::Killed)
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", None, &mode).unwrap_err();
        assert!(matches!(err, InvestigateError::TrackKilled));
    }

    #[test]
    fn missing_prereg_params_on_first_call_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", "does caching help", None, &mode).unwrap_err();
        assert!(matches!(err, InvestigateError::PreregRequired { .. }));
    }

    #[test]
    fn mismatched_prereg_params_on_a_later_call_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 50.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            InvestigateError::PreregMismatch {
                field: "kill-threshold",
                ..
            }
        ));
    }

    #[test]
    fn first_call_writes_prereg_runs_attempt_and_records_metric() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        let approved = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap();
        assert!(approved);

        let prereg = get_preregistration(&project.store, "t1").unwrap().unwrap();
        assert_eq!(prereg.metric_name, "latency_ms");

        let experiments = project.store.experiments_for("t1").unwrap();
        assert_eq!(experiments.len(), 1);
        assert_eq!(experiments[0].status, ExperimentStatus::Completed);
        let metrics = project.store.metrics_for(&experiments[0].id).unwrap();
        assert_eq!(
            metrics,
            vec![("latency_ms".to_string(), MetricValue::Number(42.0))]
        );
    }

    /// Before this wiring the conditions table was never written by
    /// anything outside zorp-track's own tests, so every boredom
    /// detector and the whole confounding search read an empty table
    /// forever. Deleting the `record_conditions` call makes this fail.
    #[test]
    fn an_attempt_records_the_conditions_it_ran_under() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap();

        let experiments = project.store.experiments_for("t1").unwrap();
        let conditions = project
            .store
            .conditions_for(&experiments[0].id)
            .unwrap()
            .into_iter()
            .map(|c| (c.condition_key, c.value))
            .collect::<Vec<_>>();

        assert!(
            conditions.contains(&(
                "checkpoint_mode".to_string(),
                MetricValue::Text("auto-approve".to_string())
            )),
            "{conditions:?}"
        );
        // The stub model names no model, so no `model` condition is
        // written. A blank one would be worse than none: it would group
        // unrelated runs together as though they shared a model.
        assert!(
            !conditions.iter().any(|(k, _)| k == "model"),
            "a model with no identity must record no model condition: {conditions:?}"
        );
    }

    /// The hypothesis is model and human authored prose, and
    /// `conditions` is read by the detectors and the search layer.
    /// Recording it there would put speculation on the observation side
    /// of integrity rule 5, which is the one thing that rule forbids.
    #[test]
    fn no_condition_carries_the_hypothesis() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let hypothesis = "SENTINEL_HYPOTHESIS_PROSE";
        project.store.create_track("t1", hypothesis).unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            hypothesis,
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap();

        let experiments = project.store.experiments_for("t1").unwrap();
        for condition in project.store.conditions_for(&experiments[0].id).unwrap() {
            let rendered = format!("{:?}", condition.value);
            assert!(
                !rendered.contains(hypothesis) && !condition.condition_key.contains(hypothesis),
                "a condition carries the hypothesis: {condition:?}"
            );
        }
    }

    /// Forecasting is off unless asked for, so the default path costs no
    /// extra model call.
    ///
    /// The model here answers with a *forecast*, not an attempt, which
    /// is what gives the test teeth: if the default flipped to on, the
    /// forecaster would parse that answer and write an expectation, and
    /// the assertion below would fail. A model returning a well formed
    /// attempt would fail forecast parsing and leave the table empty
    /// either way, so the test would pass whatever the default was.
    #[test]
    fn no_forecast_is_recorded_unless_it_is_asked_for() {
        let forecast_shaped = "```json\n{\"expected_value\": 42.0, \"interval_low\": 40.0, \
             \"interval_high\": 44.0, \"confidence\": 0.8}\n```"
            .to_string();
        let mut agent = build_agent(forecast_shaped);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        // The work phase cannot parse this answer as an attempt, so the
        // run errors. That is after the forecast step, which is the
        // part under test.
        let _ = run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        );

        let experiments = project.store.experiments_for("t1").unwrap();
        assert_eq!(experiments.len(), 1);
        assert!(
            project
                .store
                .expectations_for(&experiments[0].id)
                .unwrap()
                .is_empty(),
            "forecasting must stay off until ZORP_FORECAST asks for it"
        );
    }

    #[test]
    fn a_hypothesis_that_differs_from_the_recorded_prereg_errors() {
        let mut agent = build_agent(well_formed_response());
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let mode = CheckpointMode::terminal(true).unwrap();

        run(
            &mut agent,
            &project,
            "t1",
            "does caching help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap();

        let err = run(
            &mut agent,
            &project,
            "t1",
            "does sharding help",
            Some(PreregParams {
                metric_name: "latency_ms",
                kill_threshold: 100.0,
                threshold_direction: ThresholdDirection::LowerIsBetter,
            }),
            &mode,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            InvestigateError::PreregMismatch {
                field: "hypothesis",
                ..
            }
        ));
    }
}
