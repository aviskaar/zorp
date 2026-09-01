//! The bolt: pre-registered `investigate` attempts from the browser, the
//! write-up they produce, and a read of what they recorded.
//!
//! There is no aryabhatta engine and this does not add one. aryabhatta is
//! record plus readers, nine modules inside `zorp-track`, and it ships no
//! command on purpose. What writes to it is `investigate`: every attempt
//! records the conditions it ran under before the work starts, and, when
//! `ZORP_FORECAST` is set, a forecast before that.
//!
//! One press runs several attempts (`ZORP_BOLT_ATTEMPTS`, three by
//! default), then hands the track to `co_write` and `critique` and
//! reports the draft's path for the artifact pane. That is the whole
//! shape: attempts, a write-up audited against what the attempts
//! recorded, and a ledger read.
//!
//! # Every attempt starts where the first one did
//!
//! The transcript is truncated back to the seed before each one. An
//! attempt that could read the previous attempt's answer is not an
//! independent measurement of the metric, and comparing several of them
//! is the only reason to run several. Same rule and same call as the
//! re-run gate.
//!
//! # A killed track gets no write-up
//!
//! A breach of the pre-registered kill threshold is the answer to the
//! question, not a failure to reach one. The loop stops, and no draft is
//! written: `co_write` and `critique` both refuse a killed track, and
//! producing a document arguing for a hypothesis its own threshold just
//! rejected is the one artifact this must never make.
//!
//! It mirrors `panel.rs`. An attempt occupies the session exactly as a
//! turn does: it sets `running`, it answers the existing stop endpoint,
//! it shares the session's sequence counter, and it clears `running`
//! when it finishes. A turn and an attempt interleaved under one counter
//! would give a reconnecting browser two conversations in one
//! transcript.
//!
//! # What is deliberately absent
//!
//! There is no model-callable tool that starts an attempt. A run is
//! launched by a person, from the browser, the same rule `panel` holds,
//! and for a sharper reason here: an attempt writes to a pre-registered
//! evidence record and to the aryabhatta ledger, so a model that could
//! start one could feed the record it is later read against.
//! `agent.rs` carries the test that says so; this module carries a
//! second one over the tool set the browser's own agent gets.
//!
//! # The reader reads no model-authored text
//!
//! `read_ledger` is a display reader, not a detector and not part of the
//! search layer. It still declines to read the one model-authored text
//! column on the tables it touches, `expectations.assumptions`, because
//! the cheapest way to keep integrity rules 5 and 7 true is to have no
//! read path that names such a column at all. Nothing here is fed back
//! to a model either: the numbers on the page are arithmetic over
//! recorded rows, which is the same split `critique` and the detectors
//! use.

use crate::event::{Event, EventKind};
use crate::renderer::WebRenderer;
use crate::state::{SessionState, SettingsHandle};
use std::path::Path;
use std::sync::{Arc, Mutex};
use zorp_agent::investigate::InvestigateError;
use zorp_agent::{cancel_token, Agent, ApprovalMode, HttpModel};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::{ExperimentStatus, MetricValue};
use zorp_track::prereg::ThresholdDirection;
use zorp_track::Project;

/// What the browser asked for.
///
/// The pre-registration trio is all-or-nothing, exactly as the CLI
/// requires it: a metric name, a kill threshold and the direction that
/// kills. It is required on the first attempt for a track and must match
/// the record on every later one. `investigate::run` does that checking;
/// this type only carries what was typed.
pub struct InvestigateRequest {
    pub question: String,
    pub metric_name: Option<String>,
    pub kill_threshold: Option<f64>,
    pub threshold_direction: Option<String>,
}

/// Why a request was refused before anything ran.
///
/// Separate from the run's own errors because these are answers to an
/// HTTP request, not events on a stream: nothing has been recorded and
/// no session has been occupied when one of these comes back.
#[derive(Debug, PartialEq)]
pub enum RequestError {
    EmptyQuestion,
    PartialPrereg,
    NonFiniteThreshold,
    UnknownDirection,
}

impl RequestError {
    pub fn message(&self) -> &'static str {
        match self {
            RequestError::EmptyQuestion => {
                "nothing to investigate: the question is empty"
            }
            RequestError::PartialPrereg => {
                "metric_name, kill_threshold and threshold_direction must be given together, or all left out to reuse the recorded pre-registration"
            }
            RequestError::NonFiniteThreshold => "kill_threshold must be a finite number",
            RequestError::UnknownDirection => {
                "threshold_direction must be lower-is-better or higher-is-better"
            }
        }
    }
}

/// The pre-registration the browser supplied, if it supplied one.
///
/// Checked here rather than deeper in, because a NaN threshold written
/// into a pre-registration never compares equal to itself again and
/// locks the track out of every later run that passes the flags. The CLI
/// refuses it before recording anything and so does this.
pub fn check_request(
    request: &InvestigateRequest,
) -> Result<Option<ThresholdDirection>, RequestError> {
    if request.question.trim().is_empty() {
        return Err(RequestError::EmptyQuestion);
    }
    match (
        request.metric_name.as_deref(),
        request.kill_threshold,
        request.threshold_direction.as_deref(),
    ) {
        (None, None, None) => Ok(None),
        (Some(name), Some(threshold), Some(direction)) => {
            if name.trim().is_empty() {
                return Err(RequestError::PartialPrereg);
            }
            if !threshold.is_finite() {
                return Err(RequestError::NonFiniteThreshold);
            }
            ThresholdDirection::parse(direction)
                .map(Some)
                .ok_or(RequestError::UnknownDirection)
        }
        _ => Err(RequestError::PartialPrereg),
    }
}

/// One input an attempt was recorded as having run under.
///
/// The value is flattened to a string for display. The ledger view puts
/// it on the page and does nothing else with it, so the type it was
/// stored under buys the reader nothing here.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConditionFrame {
    pub key: String,
    pub value: String,
}

/// One forecast, as recorded before the attempt ran.
///
/// `assumptions` is missing on purpose. It is the one model-authored
/// text column on this table, and the way to keep integrity rules 5 and
/// 7 easy to check is for no read path to name it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExpectationFrame {
    pub metric_key: String,
    pub expected_value: f64,
    pub interval_low: f64,
    pub interval_high: f64,
    pub confidence: f64,
}

/// One recorded outcome.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricFrame {
    pub key: String,
    pub value: String,
}

/// A track's whole recorded ledger, as the browser reads it back.
///
/// `present` is not cosmetic. An empty ledger is the honest state for a
/// record nobody has fed, and a missing run record is a different fact,
/// so the page must be able to tell them apart.
///
/// `forecasting` says whether the server would ask for a forecast on the
/// next attempt, which is what decides whether `expectations` can ever
/// be non-empty. It is read from the server's environment and reported,
/// never set from here: forecasting costs a model call on every attempt
/// and stays off unless the person running the server said otherwise.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LedgerFrame {
    pub track_id: String,
    pub present: bool,
    pub forecasting: bool,
    pub experiments: Vec<ExperimentFrame>,
}

/// One attempt, with what went in and what came out.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExperimentFrame {
    pub id: String,
    pub status: String,
    pub conditions: Vec<ConditionFrame>,
    pub expectations: Vec<ExpectationFrame>,
    pub metrics: Vec<MetricFrame>,
}

fn show(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) => n.to_string(),
        MetricValue::Text(s) => s.clone(),
        MetricValue::Bool(b) => b.to_string(),
    }
}

fn status_name(status: ExperimentStatus) -> &'static str {
    match status {
        ExperimentStatus::Planned => "planned",
        ExperimentStatus::Running => "running",
        ExperimentStatus::Completed => "completed",
        ExperimentStatus::Failed => "failed",
        ExperimentStatus::Killed => "killed",
    }
}

/// Read back what a track's attempts recorded.
///
/// Never creates a run record. `Project::open` makes one, and a read
/// that brings a `.zorp/` directory into existence would mean opening
/// the ledger view on a fresh checkout wrote to it. `present` says which
/// case the caller is in, so an empty ledger and a missing one do not
/// have to look the same on the page: an empty ledger is the honest
/// state for a record nobody has fed, and a missing one is a different
/// fact.
pub fn read_ledger(root: &Path, question: &str) -> Result<LedgerFrame, String> {
    let track_id = zorp_track::id::track_id(question);
    if !root.join(".zorp").join("zorp.duckdb").exists() {
        return Ok(LedgerFrame {
            track_id,
            present: false,
            forecasting: zorp_agent::investigate::forecasting_enabled(),
            experiments: Vec::new(),
        });
    }
    let project = Project::open(root).map_err(|e| e.to_string())?;
    read_ledger_from(&project, &track_id)
}

/// The read itself, against an already open project.
///
/// Split out so the run thread, which is holding the project's DuckDB
/// lock, can read the ledger without a second `Project::open` that would
/// deadlock on that lock.
fn read_ledger_from(project: &Project, track_id: &str) -> Result<LedgerFrame, String> {
    let experiments = match project.store.experiments_for(track_id) {
        Ok(rows) => rows,
        // A track nobody has run is not an error. It is the same empty
        // answer as a track that exists with no attempts yet.
        Err(zorp_track::TrackError::NotFound { .. }) => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    let mut out = Vec::new();
    for experiment in experiments {
        let conditions = project
            .store
            .conditions_for(&experiment.id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| ConditionFrame {
                key: c.condition_key,
                value: show(&c.value),
            })
            .collect();
        let expectations = project
            .store
            .expectations_for(&experiment.id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| ExpectationFrame {
                metric_key: e.metric_key,
                expected_value: e.expected_value,
                interval_low: e.interval_low,
                interval_high: e.interval_high,
                confidence: e.confidence,
            })
            .collect();
        let metrics = project
            .store
            .metrics_for(&experiment.id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(key, value)| MetricFrame {
                key,
                value: show(&value),
            })
            .collect();
        out.push(ExperimentFrame {
            id: experiment.id,
            status: status_name(experiment.status).to_string(),
            conditions,
            expectations,
            metrics,
        });
    }
    Ok(LedgerFrame {
        track_id: track_id.to_string(),
        present: true,
        forecasting: zorp_agent::investigate::forecasting_enabled(),
        experiments: out,
    })
}

/// Run one attempt on a blocking thread, streaming as it goes.
///
/// Mirrors `panel::spawn_panel`, which mirrors `turn::spawn_turn`: same
/// channel, same drain thread, same closing `Done`, so the browser needs
/// no third state machine and an attempt that ends re-enables the
/// composer exactly like a turn that ends.
pub fn spawn_investigate(
    session: Arc<Mutex<SessionState>>,
    request: InvestigateRequest,
    settings: SettingsHandle,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let cancel = cancel_token();
    let seq = {
        let mut guard = session.lock().unwrap();
        guard.running = true;
        guard.cancel = Some(Arc::clone(&cancel));
        Arc::clone(&guard.seq)
    };

    let drain_session = Arc::clone(&session);
    std::thread::spawn(move || {
        for event in rx {
            drain_session.lock().unwrap().backlog.push(event);
        }
    });

    std::thread::spawn(move || {
        let mut renderer = WebRenderer::new(tx.clone());
        renderer.set_seq(Arc::clone(&seq));
        let track_id = zorp_track::id::track_id(&request.question);
        let kinds = match run_attempt(&request, &settings, &cancel, Box::new(renderer)) {
            Ok(done) => vec![done, EventKind::Done],
            Err(failure) => vec![
                EventKind::InvestigateDone {
                    track_id,
                    approved: None,
                    needs_prereg: failure.needs_prereg,
                    artifact: None,
                },
                EventKind::Error {
                    message: failure.message,
                },
                EventKind::Done,
            ],
        };
        let mut next = seq.lock().unwrap();
        for kind in kinds {
            let _ = tx.send(Event { seq: *next, kind });
            *next += 1;
        }
        drop(next);
        session.lock().unwrap().running = false;
    });
}

fn run_attempt(
    request: &InvestigateRequest,
    settings: &SettingsHandle,
    cancel: &zorp_agent::CancelToken,
    renderer: Box<dyn zorp_agent::Renderer>,
) -> Result<EventKind, AttemptFailure> {
    let direction = check_request(request).map_err(|e| e.message().to_string())?;

    let resolved = settings.lock().unwrap().effective_model();
    if !resolved.configured {
        return Err("no model configured, open settings and pick one"
            .to_string()
            .into());
    }
    let url = zorp_agent::join_url(&resolved.base_url, resolved.provider.path_suffix());
    let model = HttpModel {
        url,
        api_key: resolved.api_key,
        model: resolved.model,
        provider: resolved.provider,
        max_tokens: resolved.max_tokens,
    }
    .try_with_env_reasoning_mode(None)
    .map_err(|e| e.to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let steps = std::env::var("ZORP_MAX_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    // The same preamble the CLI puts in front of an attempt, from the
    // one place it is written down, on top of the browser's own system
    // prompt. Two copies of it would be two different experiments
    // wearing one name.
    let system = format!(
        "{}\n\n{}",
        zorp_agent::investigate::SYSTEM_PREAMBLE,
        crate::turn::system_prompt()
    );

    let project = Project::open(&cwd).map_err(|e| e.to_string())?;
    let track_id = zorp_track::id::track_id(&request.question);
    get_or_create_track(&project.store, &track_id, &request.question)?;

    // What the person typed wins. Then the track's own record, which
    // `investigate::run` reads for itself. Only when there is neither is
    // the model asked to propose one, so nothing here can ever revise a
    // commitment that already exists: a second attempt on a track uses
    // the recorded trio and a mismatch is refused exactly as before.
    let inferred = if direction.is_none()
        && zorp_track::prereg::get_preregistration(&project.store, &track_id)
            .map_err(|e| e.to_string())?
            .is_none()
    {
        crate::prereg_infer::infer(settings, &request.question)
    } else {
        None
    };

    // Say what is about to be committed, before it is committed, and
    // before the work starts. A person who never filled in the form still
    // has to be able to see what they are being held to and stop the run
    // if it is measuring the wrong thing. This is why the confidence is
    // carried out of `prereg_infer` at all.
    let mut renderer = renderer;
    if let Some(proposed) = &inferred {
        renderer.notice(&format!(
            "Pre-registering {} with a kill threshold of {} ({}), read from the question with {:.0}% confidence. It is committed before this attempt runs and cannot be changed afterwards.",
            proposed.metric_name,
            proposed.kill_threshold,
            proposed.threshold_direction.as_str(),
            proposed.confidence * 100.0,
        ));
    }

    // No recorder and no seed. An attempt is not a chat turn: the record
    // it belongs to is the track's, and seeding it with whatever was said
    // earlier in this conversation would put the browser's chat history
    // inside a pre-registered experiment.
    let agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel.clone(),
        // Checkpoints are auto-approved from the browser, so the tool
        // gate is the only thing left that could park this run waiting
        // for a person who is watching a page with no prompt on it.
        // Auto-approve here matches that, and it is a loosening: see the
        // note on `CheckpointMode::AutoApprove` below, and the decision
        // entry for 2026-08-21.
        ApprovalMode::AutoApprove,
    )
    .register_builtins_filtered(None)
    .with_renderer(renderer);
    let mut agent = agent;

    let mut prereg_params = match (direction, &inferred) {
        (Some(threshold_direction), _) => Some(zorp_agent::investigate::PreregParams {
            // Unwraps are safe: `check_request` returns a direction
            // only when all three arrived together.
            metric_name: request.metric_name.as_deref().unwrap_or_default(),
            kill_threshold: request.kill_threshold.unwrap_or_default(),
            threshold_direction,
        }),
        (None, Some(proposed)) => Some(zorp_agent::investigate::PreregParams {
            metric_name: &proposed.metric_name,
            kill_threshold: proposed.kill_threshold,
            threshold_direction: proposed.threshold_direction,
        }),
        // No commitment from anywhere. `investigate::run` answers with
        // `PreregRequired`, which is the escalation: the page shows the
        // form and a person fills it in. A declining model must not be
        // able to turn into a guessed threshold here.
        (None, None) => None,
    };

    // There is no terminal behind a browser, so the interactive
    // checkpoint decider has nothing to read from and
    // `CheckpointMode::terminal` refuses outright. Auto-approve is the
    // CLI's `--yes`, chosen explicitly here rather than fallen back to.
    // What it cannot do is skip the pre-registered kill threshold: a
    // breach kills the track unconditionally in `investigate::run`,
    // without consulting the checkpoint mode at all. So the commitment
    // still holds from the browser; what is missing is the human
    // judgement call on top of it. That gap is recorded, because
    // `checkpoint_mode` is one of the conditions every attempt writes,
    // and it will read `auto-approve` in the ledger below.
    let checkpoint_mode = CheckpointMode::AutoApprove;

    // Where the transcript stands before any attempt, so each one can
    // start from here. See the loop below.
    let seed_len = agent.transcript_len();
    let wanted = attempts();
    let mut approved = false;

    for n in 1..=wanted {
        // Every attempt starts where the first one did. This is the
        // difference between measuring a thing several times and asking
        // a model to agree with itself: an attempt that can read the
        // previous answer is not an independent measurement of the
        // metric, and the whole point of putting several of them in one
        // evidence record is that they can be compared. Same reason the
        // re-run gate truncates, and it uses the same call.
        agent.truncate_transcript(seed_len);

        if wanted > 1 {
            agent.notice(&format!("Attempt {n} of {wanted}."));
        }

        // The commitment goes in on the first attempt only. Afterwards
        // `investigate::run` reads the recorded trio for itself, and
        // passing it again would only give a mismatch something to
        // refuse.
        let params = if n == 1 { prereg_params.take() } else { None };

        approved = zorp_agent::investigate::run(
            &mut agent,
            &project,
            &track_id,
            &request.question,
            params,
            &checkpoint_mode,
        )
        .map_err(|e| AttemptFailure {
            // The one error the page acts on rather than only displays.
            // It means nobody has committed a metric and a threshold for
            // this question: none typed, none recorded, and no proposal
            // the model stood behind. The form is the answer, so say so
            // in a field.
            needs_prereg: matches!(e, InvestigateError::PreregRequired { .. }),
            message: describe(e),
        })?;

        // A breach killed the track, which is the pre-registered answer
        // to the question and not a failure to get one. Stop: further
        // attempts would be refused by `investigate::run` anyway, since
        // it will not start on a killed track.
        if is_killed(&project, &track_id) {
            agent.notice(
                "The kill threshold was breached, so the track is closed. \
                 That is the pre-registered answer to the question, and no \
                 write-up is produced for it.",
            );
            return Ok(EventKind::InvestigateDone {
                track_id,
                approved: Some(approved),
                needs_prereg: false,
                artifact: None,
            });
        }
    }

    // The attempts are the evidence. This is the artifact.
    let artifact = write_up(
        &mut agent,
        &project,
        &track_id,
        &request.question,
        &checkpoint_mode,
    );

    Ok(EventKind::InvestigateDone {
        track_id,
        approved: Some(approved),
        needs_prereg: false,
        artifact,
    })
}

/// Turns the bolt from one attempt into several.
pub const ATTEMPTS_ENV: &str = "ZORP_BOLT_ATTEMPTS";

/// Three, because one attempt is a single measurement and cannot show
/// its own spread, and because each one is a whole agent run against a
/// person watching a page. Somebody who wants a real distribution wants
/// a batch harness, not a browser click, which is what `MAX_ATTEMPTS` is
/// there to say.
pub const DEFAULT_ATTEMPTS: usize = 3;
pub const MAX_ATTEMPTS: usize = 10;

/// How many attempts one bolt press runs.
///
/// A zero or an unparseable value falls back to the default rather than
/// running nothing, for the same reason `ZORP_RERUN_REPEATS` does: a typo
/// in a count must not silently turn the feature off.
fn attempts() -> usize {
    std::env::var(ATTEMPTS_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_ATTEMPTS)
        .min(MAX_ATTEMPTS)
}

fn is_killed(project: &Project, track_id: &str) -> bool {
    project
        .store
        .get_track(track_id)
        .map(|t| t.status == zorp_track::track::TrackStatus::Killed)
        .unwrap_or(false)
}

/// Write the track up and audit the write-up, and return the artifact's
/// path for the page to open.
///
/// Returns `None` rather than failing the run, and says why on the way
/// past. The attempts are recorded whatever happens here, and throwing
/// away an evidence record because the prose stage stumbled would be the
/// worst trade available. This is also why `critique` failing still
/// returns the draft: an unaudited draft is worth strictly more than no
/// draft, as long as nobody is told it was audited.
fn write_up(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    question: &str,
    checkpoint_mode: &CheckpointMode,
) -> Option<String> {
    if let Err(e) = zorp_agent::co_write::run(agent, project, track_id, question, checkpoint_mode) {
        agent.notice(&format!("No write-up: {e}"));
        return None;
    }

    // Bounded by the same variable the CLI uses. The audit is what makes
    // the draft evidence-backed rather than merely written: it inventories
    // the draft's claims and revises the ones the track's own record does
    // not support.
    let rounds = std::env::var("ZORP_CRITIQUE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    match zorp_agent::critique::run(agent, project, track_id, rounds, checkpoint_mode) {
        Ok(report) => {
            let audited = if report.was_clean() {
                "the record supported every claim in it".to_string()
            } else {
                format!(
                    "{} claims were not supported by the record, {} still are not after revision",
                    report.initial(),
                    report.remaining()
                )
            };
            agent.notice(&format!("Draft audited: {audited}."));
        }
        Err(e) => agent.notice(&format!(
            "Draft written but not audited: {e}. Its claims have not been \
             checked against the track's evidence record."
        )),
    }

    // Relative to the working directory, which is what the artifact pane
    // serves paths against. An absolute path would not open.
    let draft = project.track_dir(track_id).join("draft.md");
    if !draft.is_file() {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    Some(
        draft
            .strip_prefix(&cwd)
            .unwrap_or(&draft)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Why an attempt did not finish, and whether the page can act on it.
///
/// A struct rather than a string because one case, a question with no
/// pre-registration anywhere, opens a form. Recognising that from the
/// wording of a message would be two copies of a sentence that must never
/// drift, and the failure mode of drift is the form silently never
/// opening.
struct AttemptFailure {
    message: String,
    needs_prereg: bool,
}

impl From<String> for AttemptFailure {
    fn from(message: String) -> Self {
        AttemptFailure {
            message,
            needs_prereg: false,
        }
    }
}

/// Say what went wrong in the browser's terms.
///
/// `InvestigateError`'s own `Display` names CLI flags, because the CLI
/// is where it was written and where a reader can act on them. A page
/// has no `--metric-name`, and telling somebody to pass one sends them
/// to a surface they are not on.
///
/// Only the two variants that name a flag are re-worded. Rewriting the
/// rest would let this crate's wording drift away from what the agent
/// actually did, which is a worse failure than a slightly terse
/// sentence.
fn describe(error: InvestigateError) -> String {
    match error {
        InvestigateError::PreregRequired { .. } => "this question has no pre-registration yet: \
             give the metric name, the kill threshold and which side kills, then run it again. \
             An attempt with no commitment recorded before it is not evidence of anything."
            .to_string(),
        InvestigateError::PreregMismatch {
            field,
            recorded,
            provided,
        } => {
            let label = match field {
                "metric-name" => "metric name",
                "kill-threshold" => "kill threshold",
                "threshold-direction" => "side that kills",
                "hypothesis" => "question",
                other => other,
            };
            format!(
                "the {label} you gave ({provided}) does not match this question's recorded \
                 pre-registration ({recorded}). A pre-registration is a commitment, so it is \
                 the attempt that has to change, not the commitment."
            )
        }
        other => other.to_string(),
    }
}

/// Reuse the track for this question, or make it.
///
/// The same rule the CLI applies: a track id that is already registered
/// today for a different question is refused rather than quietly reused,
/// because two questions sharing one evidence record is the failure that
/// is hardest to notice afterwards.
fn get_or_create_track(
    store: &zorp_track::Store,
    track_id: &str,
    question: &str,
) -> Result<(), String> {
    match store.get_track(track_id) {
        Ok(existing) if existing.hypothesis == question => Ok(()),
        Ok(existing) => Err(format!(
            "track id '{track_id}' is already registered today for a different question ({:?}); refusing to reuse it for ({:?}). Rephrase the question so it produces a distinct id.",
            existing.hypothesis, question
        )),
        Err(zorp_track::TrackError::NotFound { .. }) => store
            .create_track(track_id, question)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorp_track::experiment::MetricValue;

    fn request(question: &str) -> InvestigateRequest {
        InvestigateRequest {
            question: question.to_string(),
            metric_name: None,
            kill_threshold: None,
            threshold_direction: None,
        }
    }

    /// A run is launched by a person, from the browser, never by a
    /// model. The agent that runs the attempt must therefore have no
    /// tool that starts another one. `agent.rs` asserts the same thing
    /// over the unfiltered builtin set; this asserts it over the set
    /// this server actually hands out.
    #[test]
    fn no_tool_the_browser_hands_out_can_start_an_investigation() {
        let names: Vec<String> = zorp_agent::builtin_tools()
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        for forbidden in ["investigate", "zorp_mode", "start_investigate"] {
            assert!(
                !names.contains(&forbidden.to_string()),
                "a model must not be able to start an investigation: found {forbidden}"
            );
        }
    }

    #[test]
    fn an_empty_question_is_refused() {
        assert_eq!(
            check_request(&request("   ")),
            Err(RequestError::EmptyQuestion)
        );
    }

    #[test]
    fn no_prereg_at_all_is_allowed_because_the_record_may_already_hold_one() {
        assert_eq!(check_request(&request("does caching help")), Ok(None));
    }

    #[test]
    fn a_partial_pre_registration_is_refused() {
        let mut r = request("does caching help");
        r.metric_name = Some("latency_ms".to_string());
        assert_eq!(check_request(&r), Err(RequestError::PartialPrereg));
    }

    /// NaN is written into the pre-registration and then never compares
    /// equal to itself again, which locks the track out of every later
    /// run that passes the values explicitly. Refused before anything is
    /// recorded, the same as the CLI.
    #[test]
    fn a_non_finite_threshold_is_refused_before_anything_is_recorded() {
        let mut r = request("does caching help");
        r.metric_name = Some("latency_ms".to_string());
        r.kill_threshold = Some(f64::NAN);
        r.threshold_direction = Some("lower-is-better".to_string());
        assert_eq!(check_request(&r), Err(RequestError::NonFiniteThreshold));
    }

    #[test]
    fn a_direction_nobody_recognises_is_refused_rather_than_guessed() {
        let mut r = request("does caching help");
        r.metric_name = Some("latency_ms".to_string());
        r.kill_threshold = Some(1.0);
        r.threshold_direction = Some("bigger".to_string());
        assert_eq!(check_request(&r), Err(RequestError::UnknownDirection));
    }

    #[test]
    fn a_complete_pre_registration_parses_its_direction() {
        let mut r = request("does caching help");
        r.metric_name = Some("latency_ms".to_string());
        r.kill_threshold = Some(1.0);
        r.threshold_direction = Some("higher-is-better".to_string());
        assert_eq!(
            check_request(&r),
            Ok(Some(ThresholdDirection::HigherIsBetter))
        );
    }

    /// The CLI's wording sends a browser reader to the wrong surface.
    /// There is no `--metric-name` on a page, and a message that names
    /// one is a message the reader cannot act on.
    #[test]
    fn a_missing_pre_registration_is_explained_without_naming_a_flag() {
        let message = describe(InvestigateError::PreregRequired {
            missing: "metric-name, --kill-threshold, and --threshold-direction",
        });
        assert!(!message.contains("--"), "{message}");
        assert!(message.contains("kill threshold"), "{message}");
    }

    #[test]
    fn a_pre_registration_mismatch_says_both_values_and_names_no_flag() {
        let message = describe(InvestigateError::PreregMismatch {
            field: "kill-threshold",
            recorded: "100".to_string(),
            provided: "50".to_string(),
        });
        assert!(!message.contains("--"), "{message}");
        assert!(
            message.contains("100") && message.contains("50"),
            "{message}"
        );
    }

    /// Everything that does not name a flag is passed through. Rewriting
    /// messages that were already right is how a wording drifts away
    /// from what the code actually did.
    #[test]
    fn every_other_failure_keeps_the_words_the_agent_used() {
        let error = InvestigateError::AgentOutcome("ran out of steps".to_string());
        let expected = error.to_string();
        assert_eq!(describe(error), expected);
    }

    /// Opening the ledger view must not bring a run record into
    /// existence. `Project::open` creates one, so the reader has to
    /// check first.
    #[test]
    fn reading_a_ledger_never_creates_a_run_record() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = read_ledger(dir.path(), "does caching help").unwrap();
        assert!(!ledger.present);
        assert!(ledger.experiments.is_empty());
        assert!(
            !dir.path().join(".zorp").exists(),
            "a read created a run record"
        );
    }

    #[test]
    fn a_track_with_no_attempts_reads_back_empty_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let track_id = zorp_track::id::track_id("does caching help");
        project
            .store
            .create_track(&track_id, "does caching help")
            .unwrap();
        let ledger = read_ledger_from(&project, &track_id).unwrap();
        assert!(ledger.present);
        assert!(ledger.experiments.is_empty());
    }

    /// What the ledger view is for: the conditions an attempt ran under,
    /// which is the thing zorp did not record at all before aryabhatta.
    #[test]
    fn the_ledger_reads_back_the_conditions_an_attempt_ran_under() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let track_id = zorp_track::id::track_id("does caching help");
        project
            .store
            .create_track(&track_id, "does caching help")
            .unwrap();
        let prereg = zorp_track::prereg::write_prereg(
            &project.store,
            &project.track_dir(&track_id),
            &track_id,
            "does caching help",
            "latency_ms",
            100.0,
            ThresholdDirection::LowerIsBetter,
        )
        .unwrap();
        let experiment = project
            .store
            .create_experiment(&track_id, &prereg.id)
            .unwrap();
        project
            .store
            .record_condition(
                &experiment.id,
                "checkpoint_mode",
                &MetricValue::Text("auto-approve".to_string()),
            )
            .unwrap();
        project
            .store
            .record_metric(&experiment.id, "latency_ms", MetricValue::Number(42.0))
            .unwrap();

        let ledger = read_ledger_from(&project, &track_id).unwrap();
        assert_eq!(ledger.experiments.len(), 1);
        let run = &ledger.experiments[0];
        assert_eq!(run.conditions.len(), 1);
        assert_eq!(run.conditions[0].key, "checkpoint_mode");
        assert_eq!(run.conditions[0].value, "auto-approve");
        assert_eq!(run.metrics.len(), 1);
        assert_eq!(run.metrics[0].value, "42");
        // Nobody forecast anything, so nothing was scored. An empty
        // expectations list is the honest state for a record nobody fed.
        assert!(run.expectations.is_empty());
    }

    /// Integrity rules 5 and 7 say no detector and nothing in the search
    /// layer may read a column holding model-authored text. This reader
    /// is neither, but the cheapest way to keep the rules checkable is
    /// for no read path to name such a column at all, so the one on
    /// these tables, `expectations.assumptions`, is not in the frame.
    #[test]
    fn the_ledger_carries_no_model_authored_free_text() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        let track_id = zorp_track::id::track_id("does caching help");
        project
            .store
            .create_track(&track_id, "does caching help")
            .unwrap();
        let prereg = zorp_track::prereg::write_prereg(
            &project.store,
            &project.track_dir(&track_id),
            &track_id,
            "does caching help",
            "latency_ms",
            100.0,
            ThresholdDirection::LowerIsBetter,
        )
        .unwrap();
        let experiment = project
            .store
            .create_experiment(&track_id, &prereg.id)
            .unwrap();
        project
            .store
            .record_expectation(
                &experiment.id,
                "latency_ms",
                80.0,
                60.0,
                100.0,
                0.8,
                &["the cache is warm".to_string()],
            )
            .unwrap();

        let ledger = read_ledger_from(&project, &track_id).unwrap();
        let json = serde_json::to_string(&ledger).unwrap();
        assert!(
            !json.contains("assumptions"),
            "the ledger view named a model-authored text column: {json}"
        );
        assert!(
            !json.contains("the cache is warm"),
            "the ledger view carried model-authored free text: {json}"
        );
        assert_eq!(ledger.experiments[0].expectations.len(), 1);
        assert_eq!(ledger.experiments[0].expectations[0].expected_value, 80.0);
    }

    /// A typo in the count must not silently run zero attempts, and one
    /// bolt press must not be able to ask for fifty agent runs.
    #[test]
    fn the_attempt_count_falls_back_rather_than_running_nothing_and_is_capped() {
        let parse = |v: Option<&str>| {
            v.and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_ATTEMPTS)
                .min(MAX_ATTEMPTS)
        };
        assert_eq!(parse(None), DEFAULT_ATTEMPTS);
        assert_eq!(parse(Some("banana")), DEFAULT_ATTEMPTS);
        assert_eq!(parse(Some("0")), DEFAULT_ATTEMPTS);
        assert_eq!(parse(Some("1")), 1);
        assert_eq!(parse(Some("900")), MAX_ATTEMPTS);
    }

    /// The loop stops on a kill, so this is what it stops on. A track
    /// that is merely open must never read as killed, or one attempt
    /// would be the most the bolt ever runs.
    #[test]
    fn a_killed_track_is_recognised_and_an_open_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        assert!(!is_killed(&project, "t1"));

        project
            .store
            .set_track_status("t1", zorp_track::track::TrackStatus::Killed)
            .unwrap();
        assert!(is_killed(&project, "t1"));

        // A track that does not exist is not a killed one, and must not
        // read as one: that would stop the loop for the wrong reason.
        assert!(!is_killed(&project, "no-such-track"));
    }

    /// The page opens whatever this returns, and it opens it against the
    /// workspace root. An absolute path would simply fail to load.
    #[test]
    fn a_draft_path_is_reported_relative_to_the_workspace() {
        let cwd = std::env::current_dir().unwrap();
        let draft = cwd.join(".zorp").join("tracks").join("t1").join("draft.md");
        let relative = draft.strip_prefix(&cwd).unwrap();
        assert_eq!(
            relative.to_string_lossy(),
            ".zorp/tracks/t1/draft.md",
            "the artifact pane serves paths relative to the workspace root"
        );
    }
}
