//! aryabhatta step 7b: where findings go.
//!
//! A candidate question is handed to `validate`, which already scores
//! redundancy and feasibility and already exists. Boredom is a question
//! generator and zorp already has the question judge, so this is the
//! missing input to a built capability rather than a new pipeline.
//!
//! Nothing here acts on a finding. It proposes.
//!
//! This module is the code half of the handoff, and it is deliberately
//! the larger half. It decides *which* findings become candidates and
//! *what facts* each candidate carries, both from the record alone. The
//! model's only job is downstream: turning a brief into a sentence with
//! a question mark. It receives the invariant and the counts, and it may
//! not add invariants of its own.
//!
//! What that last rule can and cannot be held to is worth stating
//! plainly rather than implying. Code enforces that the model never
//! chooses what to look at, never sees a column of model-authored text
//! on the way in, and never supplies the facts a candidate carries: the
//! brief is generated from the record and the generation is
//! deterministic, so the same store produces the same brief. Code does
//! not enforce that the sentence the model writes back contains no
//! invented claim. That is a real gap, and the mitigation is that the
//! candidate travels with its brief, so a reader can always see what
//! the question was derived from and check the question against it.

use crate::detectors::Finding;
use crate::families::{AnomalyFamilies, AnomalyFamily};
use crate::track::Store;
use crate::TrackError;
use std::fmt::Write as _;

/// What kind of reading produced a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A boredom detector: something the record shows has never varied.
    Boredom,
    /// The search layer: a group of ledger rows that hang together.
    AnomalyFamily,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Boredom => "boredom",
            Origin::AnomalyFamily => "anomaly_family",
        }
    }
}

/// One thing worth asking about, with the record's own account of why.
///
/// Every field is code-derived. Nothing on this struct came from a
/// model, which is what makes it safe to hand to one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub origin: Origin,
    /// Which detector or which reading. Carried separately from
    /// `origin` because four detectors share one origin and they are
    /// not interchangeable.
    pub produced_by: String,
    /// What the candidate is about: a checkpoint kind, a condition key,
    /// a metric.
    pub subject: String,
    /// The facts, one per line, in a fixed order. Code-authored.
    pub facts: Vec<String>,
    /// How many rows back it. The single number a reader compares
    /// candidates by.
    pub support: u64,
    /// The query or the parameters that produced it, so the claim can
    /// be re-derived rather than taken on trust.
    pub evidence: String,
}

impl Candidate {
    /// The text handed to the model.
    ///
    /// Deterministic: the same candidate always renders the same brief,
    /// which is what lets a reader check a question against what it was
    /// derived from. It states the facts and asks for one sentence, and
    /// it says out loud that adding anything is not allowed. That last
    /// line is an instruction rather than an enforcement, and the
    /// module docs say so.
    pub fn brief(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Finding from: {}", self.produced_by);
        let _ = writeln!(out, "Subject: {}", self.subject);
        let _ = writeln!(out, "Supported by: {} records", self.support);
        let _ = writeln!(out, "Facts from the research record:");
        for fact in &self.facts {
            let _ = writeln!(out, "  - {fact}");
        }
        let _ = writeln!(out, "Re-derive with: {}", self.evidence);
        out.push_str(
            "\nWrite one question this finding raises about the research, as a single \
             sentence ending in a question mark. Use only the facts above. Do not add \
             a fact, a cause, or a number that is not listed.",
        );
        out
    }
}

/// A boredom finding as a candidate.
///
/// The detector already carries the invariant, the value and the count,
/// so this is a rename rather than an interpretation. Nothing is added.
fn from_finding(finding: &Finding) -> Candidate {
    Candidate {
        origin: Origin::Boredom,
        produced_by: finding.detector.to_string(),
        subject: finding.subject.clone(),
        facts: vec![
            format!(
                "'{}' held the single value '{}' across every record examined",
                finding.invariant_column, finding.invariant_value
            ),
            format!("it never varied in {} records", finding.support),
        ],
        support: finding.support,
        evidence: finding.query.clone(),
    }
}

/// An anomaly family as a candidate.
///
/// The θ band is part of the facts, not a footnote. A family that held
/// across two thresholds and one that held across all nine are
/// different claims, and flattening them to "a family was found" would
/// throw away the only thing the sweep bought.
fn from_family(family: &AnomalyFamily, swept: &crate::families::ThetaSweep) -> Candidate {
    let mut facts = vec![
        format!(
            "{} recorded deviations of '{}' all fell {} their forecast interval",
            family.members.len(),
            family.metric_key,
            family.direction.as_str()
        ),
        format!(
            "they held together as one group across {} of {} swept similarity thresholds, \
             from {:.2} to {:.2}",
            family.band.steps, swept.steps, family.band.low, family.band.high
        ),
    ];
    if family.shared_conditions.is_empty() {
        facts.push("they share no recorded condition".to_string());
    } else {
        let shared = family
            .shared_conditions
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        facts.push(format!("every one was recorded under {shared}"));
    }
    Candidate {
        origin: Origin::AnomalyFamily,
        produced_by: "anomaly_families".to_string(),
        subject: family.metric_key.clone(),
        facts,
        support: family.members.len() as u64,
        evidence: format!(
            "anomaly_families over thresholds {:.2} to {:.2} in {} steps; members: {}",
            swept.low,
            swept.high,
            swept.steps,
            family.members.join(", ")
        ),
    }
}

impl Store {
    /// Everything the record currently proposes asking about.
    ///
    /// Boredom findings on their own, which need no gate: they are
    /// reads of what has never varied and do not depend on any forecast
    /// being calibrated.
    ///
    /// Anomaly families are **not** included here. They sit behind the
    /// calibration gate, and a caller that wants them has to ask for
    /// them by name through [`Store::family_candidates`] and pass a
    /// track. Making the gated and ungated readings one call would let
    /// a caller cross the gate without noticing.
    ///
    /// Ordered by support, largest first, then by subject so the order
    /// does not depend on the detectors' order. Reads only.
    pub fn boredom_candidates(&self) -> Result<Vec<Candidate>, TrackError> {
        let mut candidates: Vec<Candidate> =
            self.boredom_findings()?.iter().map(from_finding).collect();
        sort_candidates(&mut candidates);
        Ok(candidates)
    }

    /// Anomaly families as candidates. Behind the calibration gate.
    ///
    /// Takes the families rather than computing them, so the caller
    /// holds the sweep and the backend record and can report what
    /// produced the candidates alongside them.
    ///
    /// Reads nothing: this is a pure rendering of what it is given.
    pub fn family_candidates(&self, families: &AnomalyFamilies) -> Vec<Candidate> {
        let mut candidates: Vec<Candidate> = families
            .families
            .iter()
            .map(|f| from_family(f, &families.swept))
            .collect();
        sort_candidates(&mut candidates);
        candidates
    }
}

fn sort_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        b.support
            .cmp(&a.support)
            .then_with(|| a.subject.cmp(&b.subject))
            .then_with(|| a.produced_by.cmp(&b.produced_by))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::CheckpointMode;
    use crate::experiment::MetricValue;
    use crate::families::ThetaSweep;
    use crate::test_support::table_counts;
    use tempfile::tempdir;

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        (dir, store)
    }

    /// Resolve one checkpoint kind the same way enough times to bore
    /// the habituation detector.
    fn habituate(store: &Store, times: usize) {
        let mode = CheckpointMode::AutoApprove;
        for _ in 0..times {
            store
                .record_checkpoint("t1", "kill-threshold", &mode, "proceed?")
                .unwrap();
        }
    }

    #[test]
    fn an_empty_store_proposes_nothing() {
        let (_dir, store) = open();
        assert!(store.boredom_candidates().unwrap().is_empty());
    }

    #[test]
    fn a_boredom_finding_becomes_a_candidate_carrying_its_own_evidence() {
        let (_dir, store) = open();
        habituate(&store, 8);

        let candidates = store.boredom_candidates().unwrap();
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        let candidate = &candidates[0];
        assert_eq!(candidate.origin, Origin::Boredom);
        assert_eq!(candidate.produced_by, "checkpoint_habituation");
        assert_eq!(candidate.subject, "kill-threshold");
        assert_eq!(candidate.support, 8);
        assert!(
            candidate.evidence.to_uppercase().contains("SELECT"),
            "the candidate must carry the query that produced it: {candidate:?}"
        );
    }

    /// The brief is what the model sees, so what it does not contain
    /// matters as much as what it does.
    #[test]
    fn the_brief_states_the_facts_and_the_support() {
        let (_dir, store) = open();
        habituate(&store, 8);
        let brief = store.boredom_candidates().unwrap()[0].brief();

        assert!(brief.contains("kill-threshold"), "{brief}");
        assert!(brief.contains("checkpoint_habituation"), "{brief}");
        assert!(brief.contains("Do not add"), "{brief}");
        // The whole line, not just the digit. Asserting on "8" alone
        // passes on a brief that dropped the support line entirely,
        // because the fact line below it also says 8: the mutation that
        // removes the count survived exactly that assertion.
        assert!(brief.contains("Supported by: 8 records"), "{brief}");
    }

    /// The same store must produce the same brief, or a question cannot
    /// be checked against what it was derived from.
    #[test]
    fn the_brief_is_deterministic() {
        let (_dir, store) = open();
        habituate(&store, 8);
        let first = store.boredom_candidates().unwrap()[0].brief();
        let second = store.boredom_candidates().unwrap()[0].brief();
        assert_eq!(first, second);
    }

    #[test]
    fn candidates_are_ordered_by_support() {
        let (_dir, store) = open();
        habituate(&store, 8);
        // A second, better supported finding: one condition key held at
        // one value across twelve experiments.
        for _ in 0..12 {
            let exp = store.create_experiment("t1", "prereg").unwrap();
            store
                .record_condition(&exp.id, "model", &MetricValue::Text("opus".into()))
                .unwrap();
        }

        let candidates = store.boredom_candidates().unwrap();
        assert_eq!(candidates.len(), 2, "{candidates:?}");
        assert_eq!(candidates[0].support, 12);
        assert_eq!(candidates[1].support, 8);
    }

    #[test]
    fn proposing_writes_nothing() {
        let (_dir, store) = open();
        habituate(&store, 8);
        let before = table_counts(&store);
        store.boredom_candidates().unwrap();
        assert_eq!(table_counts(&store), before);
    }

    /// The gated reading is a separate call. A caller cannot reach an
    /// anomaly family by asking for boredom findings.
    #[test]
    fn boredom_candidates_never_include_a_family() {
        let (_dir, store) = open();
        habituate(&store, 8);
        for candidate in store.boredom_candidates().unwrap() {
            assert_eq!(candidate.origin, Origin::Boredom, "{candidate:?}");
        }
    }

    #[test]
    fn a_family_candidate_carries_its_band_and_its_shared_conditions() {
        let families = crate::families::AnomalyFamilies {
            families: vec![crate::families::AnomalyFamily {
                members: vec!["a1".into(), "a2".into(), "a3".into()],
                metric_key: "accuracy".into(),
                direction: crate::families::Direction::Below,
                shared_conditions: vec![("model".into(), "string:opus".into())],
                band: crate::families::ThetaBand {
                    low: 0.1,
                    high: 0.5,
                    steps: 5,
                },
            }],
            swept: ThetaSweep::default(),
            min_band: 2,
            anomalies_considered: 3,
            discarded_as_unstable: 0,
            backend: crate::partition::BackendRecord::Exact,
        };
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();

        let candidates = store.family_candidates(&families);
        assert_eq!(candidates.len(), 1);
        let brief = candidates[0].brief();
        assert_eq!(candidates[0].origin, Origin::AnomalyFamily);
        assert_eq!(candidates[0].support, 3);
        assert!(brief.contains("accuracy"), "{brief}");
        assert!(brief.contains("below"), "{brief}");
        assert!(brief.contains("5 of 9"), "{brief}");
        assert!(brief.contains("model=string:opus"), "{brief}");
        assert!(
            candidates[0].evidence.contains("a1, a2, a3"),
            "{:?}",
            candidates[0].evidence
        );
    }

    /// A family with nothing in common is still a family, and saying so
    /// is more useful than an empty list where the shared conditions
    /// should be.
    #[test]
    fn a_family_with_no_shared_condition_says_so() {
        let families = crate::families::AnomalyFamilies {
            families: vec![crate::families::AnomalyFamily {
                members: vec!["a1".into(), "a2".into()],
                metric_key: "latency".into(),
                direction: crate::families::Direction::Above,
                shared_conditions: vec![],
                band: crate::families::ThetaBand {
                    low: 0.1,
                    high: 0.2,
                    steps: 2,
                },
            }],
            swept: ThetaSweep::default(),
            min_band: 2,
            anomalies_considered: 2,
            discarded_as_unstable: 0,
            backend: crate::partition::BackendRecord::Exact,
        };
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();

        let brief = store.family_candidates(&families)[0].brief();
        assert!(brief.contains("share no recorded condition"), "{brief}");
    }

    /// Every model-authored column in the schema, with the table it
    /// sits on. Written out here because the shared list carries the
    /// column names only, and the fixture below has to find a row to
    /// write into.
    const PROSE_COLUMNS: [(&str, &str); 9] = [
        ("checkpoints", "prompt_shown"),
        ("checkpoints", "decision_notes"),
        ("tracks", "hypothesis"),
        ("preregistrations", "hypothesis_snapshot"),
        ("expectations", "assumptions"),
        ("validations", "redundancy_citations"),
        ("validations", "feasibility_citations"),
        ("anomalies", "explanation"),
        ("critiques", "findings"),
    ];

    /// One admitted anomaly, through the gate, because that is the only
    /// way into the ledger. Two calls with the same conditions give a
    /// family. Trimmed from the same helper in `families`.
    fn admit(store: &Store, observed: f64, repeat: f64) {
        let original = store.create_experiment("t1", "prereg").unwrap();
        let replay = store.create_experiment("t1", "prereg").unwrap();
        for experiment in [&original, &replay] {
            store
                .record_condition(&experiment.id, "model", &MetricValue::Text("opus".into()))
                .unwrap();
        }
        store
            .record_expectation(&original.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap();
        store
            .record_metric(&original.id, "accuracy", MetricValue::Number(observed))
            .unwrap();
        store
            .record_metric(&replay.id, "accuracy", MetricValue::Number(repeat))
            .unwrap();
        let verdict = store
            .rerun_gate(&original.id, "accuracy", &[replay.id.as_str()])
            .unwrap();
        store.record_gate_verdict(&verdict).unwrap().unwrap();
    }

    /// A store with a row in every table that holds prose, so the
    /// sentinel below has somewhere to land in each of them.
    fn a_row_in_every_prose_table(store: &Store) {
        habituate(store, 8);
        crate::prereg::insert_preregistration_row(
            store,
            crate::prereg::PreregistrationRow {
                track_id: "t1",
                hypothesis: "hyp",
                metric_name: "accuracy",
                kill_threshold: 0.9,
                threshold_direction: Some(crate::prereg::ThresholdDirection::HigherIsBetter),
                file_path: std::path::Path::new("prereg.md"),
                file_hash: "hash",
                git_commit_hash: None,
                committed_at: 0,
            },
        )
        .unwrap();
        admit(store, 0.50, 0.51);
        admit(store, 0.52, 0.53);
        store
            .record_validation("t1", 0.1, &[], 0.9, &[], "proceed")
            .unwrap();
        store
            .record_critique_round("t1", 1, "draft", &[], true)
            .unwrap();
    }

    /// Integrity rule 5 at the last step before a model sees anything,
    /// checked against the prose itself rather than against the names
    /// of the columns holding it.
    ///
    /// The earlier version of this test asserted a brief did not
    /// contain the strings "explanation", "prompt_shown" and
    /// "decision_notes". Those are column names. A brief that
    /// interpolated the actual sentences stored in those columns passed
    /// it, which is the leak the rule exists to stop and the one thing
    /// the test could not see. So write a sentence nothing else could
    /// produce into every prose column the schema has, then read every
    /// brief the record can generate and require the sentence to be
    /// absent from all of them.
    ///
    /// The production change that makes this fail: have `from_finding`
    /// or `from_family` carry any stored string a person or a model
    /// wrote. Put a checkpoint's `decision_notes` in a fact line, or
    /// the track's `hypothesis` in the subject, and it goes red. It
    /// also goes red if a detector starts selecting one of those
    /// columns, since the value would arrive as a finding's
    /// `invariant_value`.
    #[test]
    fn no_brief_can_carry_model_authored_prose() {
        const SENTINEL: &str = "SENTINEL_MODEL_PROSE_DO_NOT_LEAK";

        let (_dir, store) = open();
        a_row_in_every_prose_table(&store);

        // Widening the shared list without extending the fixture would
        // leave the new column untested and this test still green, so
        // fail on that instead.
        for column in crate::detectors::MODEL_AUTHORED_COLUMNS {
            assert!(
                PROSE_COLUMNS.iter().any(|(_, c)| *c == column),
                "{column} is on the model-authored list but has no fixture row here"
            );
        }

        for (table, column) in PROSE_COLUMNS {
            store
                .conn
                .execute_batch(&format!("UPDATE {table} SET {column} = '{SENTINEL}';"))
                .unwrap();
            // An UPDATE over an empty table succeeds and changes
            // nothing, which would make every assertion below pass for
            // the wrong reason. Count the rows that actually hold the
            // sentinel.
            let planted: i64 = store
                .conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {column} = '{SENTINEL}'"),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(planted > 0, "{table}.{column} had no row to write into");
        }

        let boredom = store.boredom_candidates().unwrap();
        assert!(
            !boredom.is_empty(),
            "the fixture must propose something or this proves nothing"
        );
        for candidate in &boredom {
            assert!(
                !candidate.brief().contains(SENTINEL),
                "a boredom brief carries stored prose: {}",
                candidate.brief()
            );
        }

        let families = store.anomaly_families("t1", 2).unwrap();
        assert!(
            !families.families.is_empty(),
            "the fixture must group something or the family path is untested"
        );
        for candidate in store.family_candidates(&families) {
            assert!(
                !candidate.brief().contains(SENTINEL),
                "a family brief carries stored prose: {}",
                candidate.brief()
            );
        }
    }
}
