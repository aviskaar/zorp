//! The record of one run: the roster, every round, every finding with
//! whether it was corroborated and addressed, every prune with its
//! code-visible reason, and the request count per role. Findings text is
//! stored under a label that says a model wrote it, and the reader in
//! `evals/harbor/ensemble_report.py` never selects on it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::ledger::Finding;
use super::Roster;
use crate::panel::{ReviewerVerdict, Severity};

#[derive(Debug, Serialize)]
pub struct EnsembleRecord {
    pub run_id: String,
    pub instruction_sha256: String,
    pub roster: Roster,
    pub review_steps: usize,
    /// One per main run: the first attempt, then each revision.
    pub main_outcomes: Vec<String>,
    pub rounds: Vec<RoundRecord>,
    pub prunes: Vec<Prune>,
    /// Assistant messages per role: `main`, `reviewer-0`, `reviewer-1`, ...
    pub requests: BTreeMap<String, usize>,
    pub open_at_end: Vec<Finding>,
    /// Why the loop ended: `bound`, `nothing corroborated`,
    /// `nothing corroborated; a reviewer altered outputs`,
    /// `revision changed no output`, or `cancelled`.
    pub stopped: String,
}

impl EnsembleRecord {
    pub fn new(roster: &Roster, instruction: &str, review_steps: usize) -> Self {
        EnsembleRecord {
            run_id: crate::session::new_session_id(),
            instruction_sha256: format!("{:x}", Sha256::digest(instruction.as_bytes())),
            roster: roster.clone(),
            review_steps,
            main_outcomes: Vec::new(),
            rounds: Vec::new(),
            prunes: Vec::new(),
            requests: BTreeMap::new(),
            open_at_end: Vec::new(),
            stopped: String::new(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RoundRecord {
    pub round: usize,
    pub reviewers: Vec<ReviewerRecord>,
    /// The human-readable detail: every finding corroborated this round,
    /// re-derived and so repeated for a locus still open from an earlier
    /// round. A reader must read `newly_corroborated` for a count, never
    /// `.len()` on this, or a finding open across three rounds counts as
    /// three.
    pub corroborated: Vec<Finding>,
    /// Watched paths the revision changed.
    pub outputs_changed: Vec<String>,
    /// How many distinct findings were newly admitted this round. A finding
    /// two lenses raised still counts once here: this is a count of
    /// findings, not of lens credits, which is what a task-level table
    /// needs. Summing `newly_corroborated_by_lens`'s values instead would
    /// double it for every finding more than one lens raised.
    pub newly_corroborated: usize,
    /// Open findings the revision addressed, by hash change. A count of
    /// findings, the same relationship to `addressed_by_lens` that
    /// `newly_corroborated` has to `newly_corroborated_by_lens`.
    pub addressed: usize,
    /// How many findings each lens gets credit for among those newly
    /// admitted this round, keyed by lens name off `raised_by`. Empty
    /// when nothing corroborated. This is code-derived and additive
    /// across rounds, unlike `corroborated`. A lens-level table sums
    /// this; a task-level table must use `newly_corroborated` instead.
    pub newly_corroborated_by_lens: BTreeMap<String, usize>,
    /// The same per-lens attribution, for findings this round's revision
    /// addressed. A task-level table must use `addressed`, not the sum of
    /// this.
    pub addressed_by_lens: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct ReviewerRecord {
    pub index: usize,
    pub model: String,
    pub lens: String,
    /// `reviewed`, `reused`, `unusable`, `dropped` or `skipped`.
    pub status: String,
    pub examined: Vec<String>,
    pub findings: Vec<RawFinding>,
    pub requests: usize,
}

/// Every finding a reviewer raised, corroborated or not.
#[derive(Debug, Clone, Serialize)]
pub struct RawFinding {
    pub lens: String,
    pub severity: Severity,
    pub locus: String,
    pub claim_model_authored: String,
}

pub fn raw(verdict: &ReviewerVerdict) -> Vec<RawFinding> {
    verdict
        .findings
        .iter()
        .map(|f| RawFinding {
            lens: verdict.lens.clone(),
            severity: f.severity,
            locus: f.locus.clone(),
            claim_model_authored: f.claim.clone(),
        })
        .collect()
}

/// A reviewer dropped, and the code-visible reason. Nothing else drops one.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Prune {
    /// The reviewer's run altered a watched file. Dropped for the run.
    Tampered {
        reviewer: usize,
        model: String,
        round: usize,
        files: Vec<String>,
    },
    /// Nothing parseable came back, or the reply was cut off. Dropped for
    /// the round, and for the run when `for_run` is set (the second time).
    Unusable {
        reviewer: usize,
        model: String,
        round: usize,
        why: String,
        for_run: bool,
    },
}

/// Write the record as `<dir>/ensemble.json`, creating the directory.
pub fn write(dir: &Path, record: &EnsembleRecord) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("ensemble.json");
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ensemble::ledger::Status;

    #[test]
    fn a_record_is_written_with_its_labels() {
        let roster = Roster {
            main: "m".to_string(),
            reviewers: vec!["r0".to_string()],
            rounds: 2,
        };
        let instruction = "do the thing";
        let mut record = EnsembleRecord::new(&roster, instruction, 20);
        record.main_outcomes.push("first attempt ok".to_string());
        record.requests.insert("main".to_string(), 5);
        record.requests.insert("reviewer-0".to_string(), 3);
        record.stopped = "bound".to_string();

        record.prunes.push(Prune::Tampered {
            reviewer: 0,
            model: "r0".to_string(),
            round: 1,
            files: vec!["out.csv".to_string()],
        });
        record.prunes.push(Prune::Unusable {
            reviewer: 1,
            model: "r1".to_string(),
            round: 2,
            why: "no findings block".to_string(),
            for_run: true,
        });

        record.rounds.push(RoundRecord {
            round: 1,
            reviewers: vec![ReviewerRecord {
                index: 0,
                model: "r0".to_string(),
                lens: "contract".to_string(),
                status: "reviewed".to_string(),
                examined: vec![],
                findings: vec![RawFinding {
                    lens: "contract".to_string(),
                    severity: Severity::Note,
                    locus: "out.csv".to_string(),
                    claim_model_authored: "looks off".to_string(),
                }],
                requests: 3,
            }],
            corroborated: vec![Finding {
                round: 1,
                locus: "out.csv".to_string(),
                key: "out.csv".to_string(),
                severity: Severity::Concern,
                raised_by: vec!["adversary".to_string(), "contract".to_string()],
                claims_model_authored: vec![("contract".to_string(), "missing column".to_string())],
                file: Some("out.csv".to_string()),
                status: Status::Open,
            }],
            outputs_changed: vec!["out.csv".to_string()],
            newly_corroborated: 1,
            addressed: 1,
            newly_corroborated_by_lens: BTreeMap::from([
                ("adversary".to_string(), 1),
                ("contract".to_string(), 1),
            ]),
            addressed_by_lens: BTreeMap::from([("contract".to_string(), 1)]),
        });

        record.open_at_end.push(Finding {
            round: 1,
            locus: "notes.txt".to_string(),
            key: "notes.txt".to_string(),
            severity: Severity::Blocking,
            raised_by: vec!["reproduction".to_string()],
            claims_model_authored: vec![],
            file: None,
            status: Status::Open,
        });

        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir.path().join("nested"), &record).unwrap();
        assert!(path.ends_with("ensemble.json"));
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(json["roster"]["main"], "m");
        assert_eq!(json["run_id"], record.run_id);
        assert_eq!(json["review_steps"], 20);
        assert_eq!(json["stopped"], "bound");
        assert_eq!(json["main_outcomes"][0], "first attempt ok");
        assert_eq!(json["requests"]["main"], 5);
        assert_eq!(json["requests"]["reviewer-0"], 3);

        // The literal digest of "do the thing", computed independently
        // (shasum -a 256), so a truncation or a wrong input cannot pass
        // by coincidence.
        assert_eq!(
            json["instruction_sha256"],
            "12422cebeadd97366c31ee59300562cc3b9bca05e01005cc7a4a0945265c4472"
        );
        assert!(json["instruction_sha256"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));

        assert_eq!(json["prunes"][0]["kind"], "tampered");
        assert_eq!(json["prunes"][0]["files"][0], "out.csv");
        assert_eq!(json["prunes"][1]["kind"], "unusable");
        assert_eq!(json["prunes"][1]["why"], "no findings block");
        assert_eq!(json["prunes"][1]["for_run"], true);

        let reviewer = &json["rounds"][0]["reviewers"][0];
        assert_eq!(reviewer["index"], 0);
        assert_eq!(reviewer["model"], "r0");
        assert_eq!(reviewer["lens"], "contract");
        assert_eq!(reviewer["status"], "reviewed");
        assert_eq!(reviewer["requests"], 3);
        assert_eq!(reviewer["findings"][0]["locus"], "out.csv");
        assert_eq!(reviewer["findings"][0]["claim_model_authored"], "looks off");
        assert_eq!(reviewer["findings"][0]["severity"], "note");

        let corroborated = &json["rounds"][0]["corroborated"][0];
        assert_eq!(corroborated["round"], 1);
        assert_eq!(corroborated["locus"], "out.csv");
        assert_eq!(corroborated["severity"], "concern");
        assert_eq!(corroborated["status"], "open");
        assert_eq!(corroborated["raised_by"][0], "adversary");
        assert_eq!(corroborated["raised_by"][1], "contract");

        // One finding, raised by two lenses: newly_corroborated is 1 (a
        // count of findings), while newly_corroborated_by_lens credits
        // both lenses. A reader that summed the map for a task-level
        // total would get 2 for this one finding.
        assert_eq!(json["rounds"][0]["newly_corroborated"], 1);
        assert_eq!(json["rounds"][0]["addressed"], 1);
        assert_eq!(
            json["rounds"][0]["newly_corroborated_by_lens"]["adversary"],
            1
        );
        assert_eq!(
            json["rounds"][0]["newly_corroborated_by_lens"]["contract"],
            1
        );
        assert_eq!(json["rounds"][0]["addressed_by_lens"]["contract"], 1);
        assert!(
            json["rounds"][0]["addressed_by_lens"]
                .get("adversary")
                .is_none(),
            "only contract was given credit for the addressed finding"
        );

        assert_eq!(json["open_at_end"][0]["locus"], "notes.txt");
        assert_eq!(json["open_at_end"][0]["severity"], "blocking");
        assert_eq!(json["open_at_end"][0]["status"], "open");
        assert_eq!(json["open_at_end"][0]["raised_by"][0], "reproduction");
    }
}
