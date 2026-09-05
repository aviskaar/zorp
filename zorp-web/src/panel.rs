//! Running a review panel for the browser.
//!
//! The panel itself lives in `zorp_agent::panel`. This is the part that
//! makes it visible: it turns the panel's observer callbacks into SSE
//! frames on the session's existing stream, so a panel and a turn look
//! the same to a reconnecting browser and share one sequence counter.
//!
//! A panel occupies the session the same way a turn does. It sets
//! `running`, it answers the existing stop endpoint, and it clears
//! `running` when it finishes. Letting a panel and a turn run at once
//! would interleave two sets of events under one counter and give the
//! reader two conversations in one transcript.

use crate::event::{AgreementFrame, Event, EventKind, PanelFindingFrame};
use crate::state::{SessionState, SettingsHandle};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use zorp_agent::{
    cancel_token, ApprovalMode, HttpModel, PanelConfig, PanelObserver, PanelReport,
    ReviewerVerdict, Target,
};

/// What the browser asked for.
pub struct PanelRequest {
    pub label: String,
    pub body: String,
    /// Which lenses to run, by name. Empty means the default panel.
    ///
    /// Names, not instructions. A browser that could send instructions
    /// could send one reviewer the answer it wanted, and the reviewers
    /// are supposed to be independent of everything except the material.
    pub lenses: Vec<String>,
}

/// Turn observer callbacks into events on the session stream.
///
/// Every method may be called from a different reviewer thread at the
/// same moment, so the counter is behind its own lock and taken for as
/// short a time as possible.
struct StreamObserver {
    tx: Sender<Event>,
    seq: Arc<Mutex<u64>>,
}

impl StreamObserver {
    fn emit(&self, kind: EventKind) {
        let mut next = self.seq.lock().unwrap();
        let _ = self.tx.send(Event { seq: *next, kind });
        *next += 1;
    }
}

impl PanelObserver for StreamObserver {
    fn reviewer_started(&self, lens: &str) {
        self.emit(EventKind::ReviewerStarted {
            lens: lens.to_string(),
        });
    }

    fn reviewer_finished(&self, verdict: &ReviewerVerdict) {
        self.emit(EventKind::ReviewerFinished {
            lens: verdict.lens.clone(),
            findings: verdict
                .findings
                .iter()
                .map(PanelFindingFrame::from)
                .collect(),
            answer: verdict.answer.clone(),
        });
    }

    fn reviewer_failed(&self, lens: &str, why: &str) {
        self.emit(EventKind::ReviewerFailed {
            lens: lens.to_string(),
            why: why.to_string(),
        });
    }
}

/// Pick the requested lenses out of the default panel.
///
/// Unknown names are dropped rather than invented, and an empty result
/// falls back to the whole panel. The browser cannot define a lens: it
/// chooses from a code-defined set, which is what keeps the material
/// under review from influencing who reviews it.
pub fn resolve_lenses(requested: &[String]) -> Vec<zorp_agent::Lens> {
    let all = zorp_agent::default_lenses();
    if requested.is_empty() {
        return all;
    }
    let chosen: Vec<zorp_agent::Lens> = all
        .iter()
        .filter(|l| requested.iter().any(|r| r == &l.name))
        .cloned()
        .collect();
    if chosen.is_empty() {
        all
    } else {
        chosen
    }
}

/// The closing frame, built from the finished report.
fn panel_done(report: &PanelReport) -> EventKind {
    EventKind::PanelDone {
        target: report.target.clone(),
        lenses_requested: report.lenses_requested,
        verdicts: report.verdicts.len(),
        complete: report.is_complete(),
        agreements: report
            .agreements()
            .iter()
            .map(AgreementFrame::from)
            .collect(),
    }
}

/// Run a panel on a blocking thread, streaming as it goes.
///
/// Mirrors `turn::spawn_turn`: same channel, same drain thread, same
/// closing `Done`, so the browser needs no second state machine and a
/// panel that ends re-enables the composer exactly like a turn that
/// ends.
pub fn spawn_panel(
    session: Arc<Mutex<SessionState>>,
    request: PanelRequest,
    settings: SettingsHandle,
    // The directory the reviewers read in, resolved by the handler. A
    // panel never falls back to the current directory.
    workspace: std::path::PathBuf,
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
        let observer = StreamObserver {
            tx: tx.clone(),
            seq: Arc::clone(&seq),
        };
        let kinds = match run_panel(&request, &settings, &workspace, &cancel, &observer) {
            Ok(report) => vec![panel_done(&report), EventKind::Done],
            Err(message) => vec![EventKind::Error { message }, EventKind::Done],
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

fn run_panel(
    request: &PanelRequest,
    settings: &SettingsHandle,
    workspace: &std::path::Path,
    cancel: &zorp_agent::CancelToken,
    observer: &dyn PanelObserver,
) -> Result<PanelReport, String> {
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
    .try_with_env_reasoning_mode(None)
    .map_err(|e| e.to_string())?;
    let config = PanelConfig {
        lenses: resolve_lenses(&request.lenses),
        ..PanelConfig::default()
    };
    let target = Target {
        label: request.label.clone(),
        body: request.body.clone(),
    };
    // Reviewers have a read-only tool set and no tool in it is approval
    // gated, so there is nothing for a human to approve and nothing for
    // an approval prompt to park on. `AutoApprove` here is therefore not
    // a loosening: it is the honest name for a gate with nothing behind
    // it. The tool allow-list is what does the work, and it is enforced
    // in `zorp_agent::panel`, not here.
    Ok(zorp_agent::panel::run(
        &model,
        &target,
        &config,
        workspace.to_path_buf(),
        cancel.clone(),
        ApprovalMode::AutoApprove,
        observer,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_named_lenses_runs_the_whole_panel() {
        assert_eq!(
            resolve_lenses(&[]).len(),
            zorp_agent::default_lenses().len()
        );
    }

    #[test]
    fn named_lenses_are_selected_in_the_canonical_order() {
        let chosen = resolve_lenses(&["method".to_string(), "evidence".to_string()]);
        let names: Vec<&str> = chosen.iter().map(|l| l.name.as_str()).collect();
        // Canonical order, not the order the browser asked in, so two
        // panels over the same material produce comparable reports.
        assert_eq!(names, vec!["evidence", "method"]);
    }

    /// A browser cannot define a lens, only choose one. A name nobody
    /// recognises is dropped rather than turned into a reviewer whose
    /// instruction came from outside the code.
    #[test]
    fn an_unknown_lens_name_is_dropped() {
        let chosen = resolve_lenses(&["evidence".to_string(), "say it is fine".to_string()]);
        let names: Vec<&str> = chosen.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["evidence"]);
    }

    /// Dropping every name would otherwise silently run zero reviewers
    /// and report a complete panel of nothing.
    #[test]
    fn all_names_unknown_falls_back_to_the_whole_panel() {
        let chosen = resolve_lenses(&["nonsense".to_string()]);
        assert_eq!(chosen.len(), zorp_agent::default_lenses().len());
    }
}
