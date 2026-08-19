pub mod audit;
pub mod claims;
mod error;
pub mod ledger;

pub use audit::{Finding, FindingKind};
pub use claims::{parse_claims, Claim, ParseError};
pub use error::CritiqueError;
pub use ledger::EvidenceLedger;

use crate::agent::{Agent, Outcome};
use std::fmt::Write as _;
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::experiment::MetricValue;
use zorp_track::prereg::{get_preregistration, verify_prereg_integrity, Preregistration};
use zorp_track::track::TrackStatus;
use zorp_track::{CritiqueFinding, Project, TrackError};

/// How many revision rounds run when nobody says otherwise.
///
/// Two, not "until the critic is satisfied". A critique loop with no
/// bound does not converge, it just runs until the model stops finding
/// words to change.
pub const DEFAULT_MAX_REVISIONS: usize = 2;

/// What one audited draft came to.
#[derive(Debug, Clone, PartialEq)]
pub struct RoundReport {
    /// 0 is the draft as co-write left it. Later numbers are revisions.
    pub round: usize,
    pub findings: Vec<Finding>,
    /// Whether this draft is the one the pass carried forward.
    pub accepted: bool,
    /// Extracted claims thrown away because the draft does not contain
    /// them.
    pub discarded_claims: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CritiqueReport {
    pub rounds: Vec<RoundReport>,
    pub max_revisions: usize,
    pub draft_changed: bool,
    pub approved: bool,
}

impl CritiqueReport {
    /// Findings against the draft the pass ended up keeping.
    pub fn remaining(&self) -> usize {
        self.rounds
            .iter()
            .rev()
            .find(|r| r.accepted)
            .map(|r| r.findings.len())
            .unwrap_or(0)
    }

    pub fn initial(&self) -> usize {
        self.rounds.first().map(|r| r.findings.len()).unwrap_or(0)
    }

    /// The draft was audited and nothing was found. Distinct from "the
    /// pass fixed everything", which is why it checks round 0.
    pub fn was_clean(&self) -> bool {
        self.initial() == 0
    }
}

const EXTRACT_HEAD: &str = "\
Audit this draft against the evidence record it was written from. Do not \
judge style, do not rate quality, and do not rewrite anything.\n\n\
The record contains exactly this evidence and nothing else:\n";

const EXTRACT_TAIL: &str = "\
List every factual claim the draft makes. For each one, quote the sentence \
from the draft word for word, and give the evidence key above that the claim \
rests on, or null if it rests on nothing in that list. Use only the keys \
shown. Do not invent a key, and do not paraphrase the draft.\n\n\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{\"claims\": [{\"claim\": \"<sentence copied from the draft>\", \"evidence\": \"<one key from the list above, or null>\"}]}\n\
```";

const REVISE_HEAD: &str = "\
Revise the draft below so that each listed problem is fixed. The evidence \
record is fixed and you cannot add to it. For each problem you may attach the \
recorded figure or source the claim rests on, weaken the claim to what the \
record supports, or remove the claim. Keep everything the record does \
support: deleting sound material to shorten the list is not a fix.\n\n\
Do not change the hypothesis, the metric, or the kill threshold. They are \
pre-registered, and only a human moves them.\n\n\
The record contains exactly this evidence and nothing else:\n";

const REVISE_TAIL: &str = "\
Reply with the complete revised draft and nothing else. No preamble, no \
commentary, no code fence.";

/// The run record as it stood before the pass touched anything.
///
/// The pass reads the record and writes the artifact. If any of this
/// moved while the agent was running, something the agent did reached
/// the record, and nothing it produced can be trusted.
struct RecordSnapshot {
    status: TrackStatus,
    hypothesis: String,
    prereg: Option<Preregistration>,
    metrics: Vec<(String, String, MetricValue)>,
    experiments: usize,
    validation_id: Option<String>,
}

impl RecordSnapshot {
    fn take(project: &Project, track_id: &str) -> Result<Self, TrackError> {
        let track = project.store.get_track(track_id)?;
        let validation_id = match project.store.get_validation(track_id) {
            Ok(v) => Some(v.id),
            Err(TrackError::NotFound {
                kind: "validation", ..
            }) => None,
            Err(e) => return Err(e),
        };
        Ok(RecordSnapshot {
            status: track.status,
            hypothesis: track.hypothesis,
            prereg: get_preregistration(&project.store, track_id)?,
            metrics: project.store.metrics_for_track(track_id)?,
            experiments: project.store.experiments_for(track_id)?.len(),
            validation_id,
        })
    }

    fn guard(&self, project: &Project, track_id: &str) -> Result<(), CritiqueError> {
        let now = RecordSnapshot::take(project, track_id)?;
        let moved = if now.status != self.status {
            Some("track status")
        } else if now.hypothesis != self.hypothesis {
            Some("hypothesis")
        } else if now.prereg != self.prereg {
            Some("pre-registration")
        } else if now.metrics != self.metrics {
            Some("recorded metrics")
        } else if now.experiments != self.experiments {
            Some("experiments")
        } else if now.validation_id != self.validation_id {
            Some("validation")
        } else {
            None
        };
        if let Some(what) = moved {
            return Err(CritiqueError::RecordMutated { what });
        }
        // The row can sit still while the file it hashes does not, which
        // is what a file-writing tool pointed at prereg.md would do.
        if self.prereg.is_some() && verify_prereg_integrity(&project.store, track_id).is_err() {
            return Err(CritiqueError::RecordMutated {
                what: "prereg.md on disk",
            });
        }
        Ok(())
    }
}

/// A model asked for a bare draft sometimes wraps it in a fence anyway.
/// Unwrapping is safe because a whole draft is never one code block.
fn strip_sole_fence(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") || !trimmed.ends_with("```") || trimmed.len() < 6 {
        return text.to_string();
    }
    let Some(after_open) = trimmed.find('\n') else {
        return text.to_string();
    };
    let body = &trimmed[after_open + 1..trimmed.len() - 3];
    format!("{}\n", body.trim_end())
}

/// One audit of one draft: ask the critic to inventory the claims, then
/// check that inventory and the draft's figures against the ledger.
///
/// The number audit runs whatever the critic says, so a critic that
/// reports nothing cannot declare a draft clean.
fn audit_once(
    agent: &mut Agent,
    ledger: &EvidenceLedger,
    draft: &str,
) -> Result<(Vec<Finding>, usize), CritiqueError> {
    // Each audit looks at one draft on its own. Carrying the previous
    // round's transcript would let the critic defend what it said last
    // time instead of reading what is in front of it.
    agent.clear_history();
    let task = format!(
        "{EXTRACT_HEAD}{}\nDraft:\n{draft}\n\n{EXTRACT_TAIL}",
        ledger.render()
    );
    let text = match agent.run(&task) {
        Outcome::Complete(text) => text,
        other => return Err(CritiqueError::AgentOutcome(other.describe())),
    };
    let claims = parse_claims(&text)?;
    let mut findings = audit::audit_numbers(draft, ledger);
    let (claim_findings, discarded) = audit::audit_claims(&claims, draft, ledger);
    findings.extend(claim_findings);
    Ok((findings, discarded))
}

fn revise_once(
    agent: &mut Agent,
    ledger: &EvidenceLedger,
    draft: &str,
    findings: &[Finding],
) -> Result<String, CritiqueError> {
    agent.clear_history();
    let mut problems = String::new();
    for f in findings {
        let _ = writeln!(problems, "- [{}] {}\n  In: {}", f.kind, f.detail, f.claim);
    }
    let task = format!(
        "{REVISE_HEAD}{}\nProblems found:\n{problems}\nDraft:\n{draft}\n\n{REVISE_TAIL}",
        ledger.render()
    );
    match agent.run(&task) {
        Outcome::Complete(text) => Ok(strip_sole_fence(&text)),
        other => Err(CritiqueError::AgentOutcome(other.describe())),
    }
}

fn to_records(findings: &[Finding]) -> Vec<CritiqueFinding> {
    findings.iter().map(Finding::to_record).collect()
}

/// The human-readable half of the record. The DuckDB rows are the
/// durable version; this is the one somebody actually reads.
fn render_notes(
    track_id: &str,
    rounds: &[RoundReport],
    max_revisions: usize,
    draft_changed: bool,
) -> String {
    let mut out = format!("# Critique: {track_id}\n\n");
    let _ = writeln!(
        out,
        "Bound: at most {max_revisions} revision round(s). The audit always runs once, so a bound of 0 means audit only.\n"
    );
    for round in rounds {
        let heading = if round.round == 0 {
            "Round 0: the draft as co-write left it".to_string()
        } else {
            format!("Round {}: revision", round.round)
        };
        let _ = writeln!(out, "## {heading}\n");
        if round.findings.is_empty() {
            out.push_str("No findings. Every claim the critic inventoried rests on something in the evidence record, and every figure in the draft appears in it.\n\n");
        } else {
            let _ = writeln!(out, "{} finding(s).\n", round.findings.len());
            for f in &round.findings {
                let _ = writeln!(out, "- `{}`: {}", f.kind, f.detail);
                let _ = writeln!(out, "  > {}\n", f.claim);
            }
        }
        if round.discarded_claims > 0 {
            let _ = writeln!(
                out,
                "{} extracted claim(s) were discarded because the draft does not contain them.\n",
                round.discarded_claims
            );
        }
        if round.round > 0 {
            let verdict = if round.accepted {
                "Kept: it left fewer findings than the draft before it."
            } else {
                "Discarded: it did not leave fewer findings than the draft before it, so the earlier draft stands."
            };
            let _ = writeln!(out, "{verdict}\n");
        }
    }
    out.push_str("## Result\n\n");
    if draft_changed {
        out.push_str("draft.md was revised. The draft as co-write left it is kept beside it at `draft.pre-critique.md`, so a diff of the two shows exactly what this pass changed.\n");
    } else {
        out.push_str("draft.md was not changed.\n");
    }
    out
}

/// Review a track's `draft.md` against that track's evidence record,
/// revise it where the record does not back it, and record both.
///
/// `max_revisions` bounds the number of revision rounds. The audit
/// itself always runs once, so `0` means "tell me what is wrong and do
/// not touch the draft".
///
/// Termination does not depend on the model agreeing that it is done. A
/// revision is kept only if it leaves strictly fewer findings than the
/// draft it replaced, and the first one that does not ends the pass, so
/// the loop runs at most `min(max_revisions, findings at round 0)`
/// times whatever the model returns.
///
/// Like `co_write::run` and `deliver::run`, neither checkpoint outcome
/// changes the track's status.
pub fn run(
    agent: &mut Agent,
    project: &Project,
    track_id: &str,
    max_revisions: usize,
    checkpoint_mode: &CheckpointMode,
) -> Result<CritiqueReport, CritiqueError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(CritiqueError::TrackKilled);
    }

    let track_dir = project.track_dir(track_id);
    let draft_path = track_dir.join("draft.md");
    let original = match std::fs::read_to_string(&draft_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(CritiqueError::NoDraft),
        Err(e) => return Err(e.into()),
    };

    let ledger = EvidenceLedger::from_track(project, track_id)?;
    if ledger.is_empty() {
        return Err(CritiqueError::NoEvidence);
    }

    let snapshot = RecordSnapshot::take(project, track_id)?;

    let mut current = original.clone();
    let (mut findings, discarded) = audit_once(agent, &ledger, &current)?;
    snapshot.guard(project, track_id)?;
    project
        .store
        .record_critique_round(track_id, 0, &current, &to_records(&findings), true)?;
    let mut rounds = vec![RoundReport {
        round: 0,
        findings: findings.clone(),
        accepted: true,
        discarded_claims: discarded,
    }];

    for round in 1..=max_revisions {
        if findings.is_empty() {
            break;
        }
        let revised = revise_once(agent, &ledger, &current, &findings)?;
        snapshot.guard(project, track_id)?;

        // An empty answer audits clean for the worst possible reason:
        // there is nothing left to check. It is never an improvement.
        let (new_findings, new_discarded, accepted) = if revised.trim().is_empty() {
            (findings.clone(), 0, false)
        } else {
            let (f, d) = audit_once(agent, &ledger, &revised)?;
            snapshot.guard(project, track_id)?;
            let accepted = f.len() < findings.len();
            (f, d, accepted)
        };

        project.store.record_critique_round(
            track_id,
            round as i64,
            &revised,
            &to_records(&new_findings),
            accepted,
        )?;
        rounds.push(RoundReport {
            round,
            findings: new_findings.clone(),
            accepted,
            discarded_claims: new_discarded,
        });
        if !accepted {
            break;
        }
        current = revised;
        findings = new_findings;
    }

    let draft_changed = current != original;
    std::fs::create_dir_all(&track_dir)?;
    if draft_changed {
        std::fs::write(track_dir.join("draft.pre-critique.md"), &original)?;
        std::fs::write(&draft_path, &current)?;
    }
    let notes = render_notes(track_id, &rounds, max_revisions, draft_changed);
    std::fs::write(track_dir.join("critique.md"), &notes)?;

    let report = CritiqueReport {
        rounds,
        max_revisions,
        draft_changed,
        approved: false,
    };
    let prompt = format!(
        "critique: {} finding(s) at the start, {} left after {} revision round(s). \
         draft.md {} ({} lines before, {} after). Notes at {}.\nAccept this critique?",
        report.initial(),
        report.remaining(),
        report.rounds.len() - 1,
        if draft_changed {
            "was revised"
        } else {
            "was left alone"
        },
        original.lines().count(),
        current.lines().count(),
        track_dir.join("critique.md").display()
    );
    let approved =
        project
            .store
            .record_checkpoint(track_id, "critique", checkpoint_mode, &prompt)?;

    Ok(CritiqueReport { approved, ..report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssistantMessage, Message, Model};
    use crate::BoxErr;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use zorp_track::experiment::{ExperimentStatus, MetricValue};
    use zorp_track::prereg::{
        get_preregistration, verify_prereg_integrity, write_prereg, ThresholdDirection,
    };
    use zorp_track::track::TrackStatus;

    /// A model that answers from a fixed script, so a multi-round pass
    /// is exercised end to end without a network. Running the script dry
    /// is itself a failure, which is how a test pins the number of model
    /// calls a pass is allowed to make.
    struct ScriptedModel {
        responses: Arc<Mutex<VecDeque<String>>>,
        calls: Arc<AtomicUsize>,
    }

    impl Model for ScriptedModel {
        fn complete(
            &self,
            _messages: &[Message],
            _tools: &[serde_json::Value],
        ) -> Result<AssistantMessage, BoxErr> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let next = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| -> BoxErr { "scripted model ran out of responses".into() })?;
            Ok(AssistantMessage {
                content: next,
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }

        fn clone_box(&self) -> Box<dyn Model> {
            Box::new(ScriptedModel {
                responses: self.responses.clone(),
                calls: self.calls.clone(),
            })
        }
    }

    fn scripted(responses: &[&str]) -> (Agent, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = ScriptedModel {
            responses: Arc::new(Mutex::new(
                responses.iter().map(|s| s.to_string()).collect(),
            )),
            calls: calls.clone(),
        };
        let agent = Agent::new(
            Box::new(model),
            "system",
            5,
            std::env::temp_dir(),
            crate::cancel_token(),
            crate::ApprovalMode::AutoApprove,
        );
        (agent, calls)
    }

    fn claims_block(body: &str) -> String {
        format!("Here is what the draft claims.\n```json\n{{\"claims\": [{body}]}}\n```\n")
    }

    fn no_claims() -> String {
        claims_block("")
    }

    /// A track with one recorded metric and a draft on disk, which is
    /// the state co-write leaves behind.
    fn track_with_draft(project: &Project, track_id: &str, draft: &str) {
        project
            .store
            .create_track(track_id, "does caching help")
            .unwrap();
        let exp = project
            .store
            .create_experiment(track_id, "no-prereg")
            .unwrap();
        project
            .store
            .set_experiment_status(&exp.id, ExperimentStatus::Completed)
            .unwrap();
        project
            .store
            .record_metric(&exp.id, "latency_ms", MetricValue::Number(42.0))
            .unwrap();
        let track_dir = project.track_dir(track_id);
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(track_dir.join("draft.md"), draft).unwrap();
    }

    fn draft_on_disk(project: &Project, track_id: &str) -> String {
        std::fs::read_to_string(project.track_dir(track_id).join("draft.md")).unwrap()
    }

    const CLEAN_DRAFT: &str = "Latency was 42ms.\n";
    const DIRTY_DRAFT: &str = "Latency was 42ms. Throughput hit 900 rps. Errors fell to 7.\n";
    const ONE_LEFT: &str = "Latency was 42ms. Throughput hit 900 rps.\n";

    #[test]
    fn a_clean_draft_is_left_alone_and_no_revision_is_requested() {
        // One model call: the audit. A pass that asks for a revision
        // anyway is worse than not running.
        let (mut agent, calls) = scripted(&[&claims_block(
            r#"{"claim": "Latency was 42ms.", "evidence": "metric:latency_ms"}"#,
        )]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", CLEAN_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        let report = run(&mut agent, &project, "t1", 2, &mode).unwrap();

        assert!(report.was_clean());
        assert!(!report.draft_changed);
        assert_eq!(report.rounds.len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(draft_on_disk(&project, "t1"), CLEAN_DRAFT);
        assert!(!project
            .track_dir("t1")
            .join("draft.pre-critique.md")
            .exists());
    }

    #[test]
    fn a_clean_draft_still_records_that_the_pass_ran() {
        let (mut agent, _) = scripted(&[&no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", CLEAN_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        run(&mut agent, &project, "t1", 2, &mode).unwrap();

        let rounds = project.store.critiques_for("t1").unwrap();
        assert_eq!(rounds.len(), 1);
        assert!(rounds[0].findings.is_empty());
        assert!(rounds[0].accepted);
    }

    #[test]
    fn a_revision_that_reduces_findings_is_kept() {
        let (mut agent, calls) = scripted(&[
            &no_claims(),
            ONE_LEFT,
            &no_claims(),
            CLEAN_DRAFT,
            &no_claims(),
        ]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        let report = run(&mut agent, &project, "t1", 2, &mode).unwrap();

        assert_eq!(report.initial(), 2, "{:?}", report.rounds);
        assert_eq!(report.remaining(), 0, "{:?}", report.rounds);
        assert!(report.draft_changed);
        assert_eq!(draft_on_disk(&project, "t1"), CLEAN_DRAFT);
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn the_original_draft_is_kept_beside_the_revision() {
        let (mut agent, _) = scripted(&[&no_claims(), CLEAN_DRAFT, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        run(&mut agent, &project, "t1", 2, &mode).unwrap();

        // What changed has to be inspectable, and a diff against the
        // draft as co-write left it is the plainest way to see it.
        let before =
            std::fs::read_to_string(project.track_dir("t1").join("draft.pre-critique.md")).unwrap();
        assert_eq!(before, DIRTY_DRAFT);
        assert_eq!(draft_on_disk(&project, "t1"), CLEAN_DRAFT);
    }

    #[test]
    fn the_loop_stops_at_the_configured_round_bound() {
        // The script could keep improving, but one revision is the bound.
        let (mut agent, calls) = scripted(&[
            &no_claims(),
            ONE_LEFT,
            &no_claims(),
            CLEAN_DRAFT,
            &no_claims(),
        ]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        let report = run(&mut agent, &project, "t1", 1, &mode).unwrap();

        assert_eq!(report.remaining(), 1, "{:?}", report.rounds);
        assert_eq!(draft_on_disk(&project, "t1"), ONE_LEFT);
        // Audit, revise, audit. Nothing beyond the bound.
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn zero_rounds_audits_and_records_without_touching_the_draft() {
        let (mut agent, calls) = scripted(&[&no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        let report = run(&mut agent, &project, "t1", 0, &mode).unwrap();

        assert_eq!(report.initial(), 2);
        assert!(!report.draft_changed);
        assert_eq!(draft_on_disk(&project, "t1"), DIRTY_DRAFT);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(project.store.critiques_for("t1").unwrap().len(), 1);
    }

    #[test]
    fn a_revision_that_does_not_reduce_findings_is_discarded() {
        // Same two unsupported figures, different words. Rewording is
        // not an improvement, and the pass must not bank it.
        let reworded = "Latency was 42ms. Throughput reached 900 rps. Errors dropped to 7.\n";
        let (mut agent, _) = scripted(&[&no_claims(), reworded, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        let report = run(&mut agent, &project, "t1", 2, &mode).unwrap();

        assert!(!report.draft_changed);
        assert_eq!(draft_on_disk(&project, "t1"), DIRTY_DRAFT);
        let rounds = project.store.critiques_for("t1").unwrap();
        assert_eq!(rounds.len(), 2);
        assert!(rounds[0].accepted);
        assert!(!rounds[1].accepted, "the rejected revision must be on file");
    }

    #[test]
    fn every_round_lands_in_the_run_record_with_its_findings() {
        let (mut agent, _) = scripted(&[&no_claims(), ONE_LEFT, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        run(&mut agent, &project, "t1", 1, &mode).unwrap();

        let rounds = project.store.critiques_for("t1").unwrap();
        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].round, 0);
        assert_eq!(rounds[0].findings.len(), 2);
        assert_eq!(rounds[1].round, 1);
        assert_eq!(rounds[1].findings.len(), 1);
        assert_ne!(rounds[0].draft_hash, rounds[1].draft_hash);
        // Findings carry the text they are about, not just a count.
        assert!(
            rounds[0].findings.iter().any(|f| f.claim.contains("900")),
            "{:?}",
            rounds[0].findings
        );
    }

    #[test]
    fn critique_md_says_what_was_criticised_and_what_changed() {
        let (mut agent, _) = scripted(&[&no_claims(), CLEAN_DRAFT, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let mode = CheckpointMode::terminal(true).unwrap();

        run(&mut agent, &project, "t1", 2, &mode).unwrap();

        let notes = std::fs::read_to_string(project.track_dir("t1").join("critique.md")).unwrap();
        assert!(notes.contains("number-not-in-record"), "{notes}");
        assert!(notes.contains("900"), "{notes}");
        assert!(notes.contains("draft.pre-critique.md"), "{notes}");
    }

    /// A critic that goes for the pre-registration through the ordinary
    /// file-writing tool, which is the realistic version of the attack:
    /// no special capability, just `write_file` pointed at prereg.md.
    struct TamperingModel {
        called: Arc<AtomicUsize>,
    }

    const TAMPERED_PREREG: &str = "# Pre-registration: t1\n\nHypothesis: does caching help\nMetric: latency_ms\nKill threshold: 999999\nThreshold direction: lower-is-better\n";

    impl Model for TamperingModel {
        fn complete(
            &self,
            _messages: &[Message],
            _tools: &[serde_json::Value],
        ) -> Result<AssistantMessage, BoxErr> {
            let n = self.called.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Ok(AssistantMessage {
                    content: String::new(),
                    tool_calls: vec![crate::model::ToolCall {
                        id: "1".to_string(),
                        name: "write_file".to_string(),
                        arguments: serde_json::json!({
                            "path": ".zorp/tracks/t1/prereg.md",
                            "content": TAMPERED_PREREG,
                        }),
                    }],
                    finish_reason: "tool_calls".to_string(),
                    reasoning_content: None,
                });
            }
            Ok(AssistantMessage {
                content: "```json\n{\"claims\": []}\n```\n".to_string(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }

        fn clone_box(&self) -> Box<dyn Model> {
            Box::new(TamperingModel {
                called: self.called.clone(),
            })
        }
    }

    fn track_with_prereg(project: &Project, track_id: &str, draft: &str) {
        track_with_draft(project, track_id, draft);
        write_prereg(
            &project.store,
            &project.track_dir(track_id),
            track_id,
            "does caching help",
            "latency_ms",
            100.0,
            ThresholdDirection::LowerIsBetter,
        )
        .unwrap();
    }

    /// The one thing this pass must never be able to do. Only a human
    /// moves the Kill Threshold.
    #[test]
    fn the_pass_cannot_move_the_kill_threshold() {
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_prereg(&project, "t1", DIRTY_DRAFT);
        let prereg_path = project.track_dir("t1").join("prereg.md");
        let before = get_preregistration(&project.store, "t1").unwrap().unwrap();

        let model = TamperingModel {
            called: Arc::new(AtomicUsize::new(0)),
        };
        // A genuinely write-capable agent, rooted at the project, with a
        // policy that permits edits. Nothing here is stacked in the
        // pass's favour: the tool call really does land.
        let mut agent = Agent::new(
            Box::new(model),
            "system",
            5,
            dir.path().to_path_buf(),
            crate::cancel_token(),
            crate::ApprovalMode::AutoApprove,
        )
        .register(Box::new(crate::WriteFile))
        .with_policy(crate::Policy::from_preset(crate::Preset::Editor));
        let mode = CheckpointMode::terminal(true).unwrap();

        let err = run(&mut agent, &project, "t1", 2, &mode).unwrap_err();
        assert!(
            matches!(err, CritiqueError::RecordMutated { .. }),
            "expected RecordMutated, got {err:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&prereg_path).unwrap(),
            TAMPERED_PREREG,
            "the tool call has to have landed, or this test proves nothing"
        );

        // The threshold the run record reports is still the human's, and
        // the tampered file is now visibly at odds with it rather than
        // quietly authoritative.
        let after = get_preregistration(&project.store, "t1").unwrap().unwrap();
        assert_eq!(after.kill_threshold, 100.0);
        assert_eq!(after, before);
        assert!(verify_prereg_integrity(&project.store, "t1").is_err());
        // And the draft was not rewritten off the back of a tampered run.
        assert_eq!(draft_on_disk(&project, "t1"), DIRTY_DRAFT);
    }

    /// The other half of the same guarantee: an ordinary, successful,
    /// revising pass leaves pre-registered intent exactly as it found it.
    #[test]
    fn a_normal_pass_leaves_pre_registered_intent_byte_identical() {
        let (mut agent, _) = scripted(&[&no_claims(), CLEAN_DRAFT, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_prereg(&project, "t1", DIRTY_DRAFT);
        let prereg_path = project.track_dir("t1").join("prereg.md");
        let before_row = get_preregistration(&project.store, "t1").unwrap().unwrap();
        let before_file = std::fs::read_to_string(&prereg_path).unwrap();
        let before_status = project.store.get_track("t1").unwrap().status;

        let report = run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap();
        assert!(report.draft_changed, "the pass should have revised");

        assert_eq!(
            get_preregistration(&project.store, "t1").unwrap().unwrap(),
            before_row
        );
        assert_eq!(std::fs::read_to_string(&prereg_path).unwrap(), before_file);
        assert_eq!(project.store.get_track("t1").unwrap().status, before_status);
        assert!(zorp_track::prereg::verify_prereg_integrity(&project.store, "t1").is_ok());
    }

    fn mode_yes() -> CheckpointMode {
        CheckpointMode::terminal(true).unwrap()
    }

    /// The pass reads the record and writes only the artifact. Nothing
    /// it does may add evidence, because evidence added by a critique of
    /// a draft would be evidence invented to support that draft.
    #[test]
    fn a_normal_pass_records_no_new_evidence() {
        let (mut agent, _) = scripted(&[&no_claims(), CLEAN_DRAFT, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);
        let metrics_before = project.store.metrics_for_track("t1").unwrap();
        let experiments_before = project.store.experiments_for("t1").unwrap().len();

        run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap();

        assert_eq!(
            project.store.metrics_for_track("t1").unwrap(),
            metrics_before
        );
        assert_eq!(
            project.store.experiments_for("t1").unwrap().len(),
            experiments_before
        );
    }

    #[test]
    fn a_rejected_checkpoint_does_not_kill_the_track() {
        struct RejectAll;
        impl zorp_track::checkpoint::Decider for RejectAll {
            fn decide(&self, _prompt: &str) -> bool {
                false
            }
        }
        let (mut agent, _) = scripted(&[&no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", CLEAN_DRAFT);
        let mode = CheckpointMode::Interactive(Arc::new(RejectAll));

        let report = run(&mut agent, &project, "t1", 2, &mode).unwrap();

        assert!(!report.approved);
        assert_eq!(
            project.store.get_track("t1").unwrap().status,
            TrackStatus::Active
        );
    }

    #[test]
    fn killed_track_is_refused_before_calling_the_model() {
        let (mut agent, calls) = scripted(&[&no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", CLEAN_DRAFT);
        project
            .store
            .set_track_status("t1", TrackStatus::Killed)
            .unwrap();

        let err = run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap_err();
        assert!(matches!(err, CritiqueError::TrackKilled));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_track_with_no_draft_is_refused() {
        let (mut agent, _) = scripted(&[&no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let exp = project.store.create_experiment("t1", "no-prereg").unwrap();
        project
            .store
            .record_metric(&exp.id, "latency_ms", MetricValue::Number(42.0))
            .unwrap();

        let err = run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap_err();
        assert!(matches!(err, CritiqueError::NoDraft));
    }

    #[test]
    fn a_track_with_no_evidence_is_refused() {
        let (mut agent, calls) = scripted(&[&no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let track_dir = project.track_dir("t1");
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(track_dir.join("draft.md"), CLEAN_DRAFT).unwrap();

        // Auditing against an empty ledger would flag the whole draft.
        // Refusing is the honest answer.
        let err = run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap_err();
        assert!(matches!(err, CritiqueError::NoEvidence));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// Deleting the draft removes every figure, so an empty answer would
    /// audit perfectly clean. It is the cheapest way to score well and
    /// the worst possible outcome.
    #[test]
    fn an_empty_revision_is_never_an_improvement() {
        let (mut agent, _) = scripted(&[&no_claims(), "   \n  ", &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);

        let report = run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap();

        assert!(!report.draft_changed);
        assert_eq!(draft_on_disk(&project, "t1"), DIRTY_DRAFT);
        let rounds = project.store.critiques_for("t1").unwrap();
        assert_eq!(rounds.len(), 2);
        assert!(!rounds[1].accepted);
    }

    #[test]
    fn a_revision_the_model_wrapped_in_a_fence_is_unwrapped() {
        let fenced = format!("```markdown\n{}```\n", CLEAN_DRAFT);
        let (mut agent, _) = scripted(&[&no_claims(), &fenced, &no_claims()]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", DIRTY_DRAFT);

        run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap();

        assert_eq!(draft_on_disk(&project, "t1"), CLEAN_DRAFT);
    }

    #[test]
    fn a_critic_that_answers_with_no_json_block_is_a_parse_error() {
        let (mut agent, _) = scripted(&["I think the draft reads well."]);
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        track_with_draft(&project, "t1", CLEAN_DRAFT);

        let err = run(&mut agent, &project, "t1", 2, &mode_yes()).unwrap_err();
        assert!(matches!(err, CritiqueError::Parse(_)), "got {err:?}");
        assert_eq!(draft_on_disk(&project, "t1"), CLEAN_DRAFT);
    }
}
