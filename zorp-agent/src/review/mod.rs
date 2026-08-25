//! review: a standing adversarial review of a paper.
//!
//! The shape is a loop, not a pass. Each round runs a doer and a checker
//! on every dimension, keeps only what quotes the paper and is not
//! something an earlier round already said, and hands each survivor to
//! several agents told to refute it. The loop ends on a convergence
//! criterion rather than on a reviewer saying it is satisfied, because a
//! reviewer asked "anything else?" always answers.
//!
//! Two bounds are load-bearing and neither is a prompt. Rounds stop after
//! `quiet_rounds` consecutive rounds with nothing new, and unconditionally
//! at `max_rounds`. Agents are charged against one budget with a depth
//! limit, and when it runs out the review stops and says what it did not
//! cover.

pub mod budget;
pub mod convergence;
pub mod dimension;
pub mod dispatch;
mod error;
pub mod finding;
pub mod report;
pub mod verify;

pub use error::ReviewError;

use crate::agent::Agent;
use budget::Budget;
use convergence::{Convergence, Stop};
use dimension::{Dimension, Inputs, Selection};
use dispatch::{BudgetedDispatcher, Dispatcher, Job, JobResult, PoolDispatcher};
use finding::{anchor_is_in_paper, normalize, parse_reply, Finding};
use report::{rank, DimensionTally, ReviewReport, ReviewedFinding};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;
use verify::{lenses_for, refuter_prompt, tally, Verdict, Verification, Vote};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

/// Doers and checkers run here. Depth 0 is the orchestrator, which is
/// code and not an agent, so it costs nothing.
const DEPTH_REVIEWER: usize = 1;
/// Refuters run one level below the reviewers that produced the finding.
/// Anything they start is deeper still, which is where the depth limit
/// starts to bite.
const DEPTH_VERIFIER: usize = 2;

/// How much of the paper the reviewers are shown inline. Past this the
/// text is cut and the report says so: a review of the first half of a
/// paper that does not mention the second half is worse than no review.
const PAPER_INLINE_LIMIT: usize = 200_000;

/// How many refusals the report lists individually before summarising.
const REFUSAL_DETAIL_LIMIT: usize = 20;

const REVIEW_PREAMBLE: &str = "\
You are one reviewer in an adversarial review of a paper. You have one dimension and \
one job. Report only problems you can point at in the paper: every finding must quote a \
span of the paper verbatim, and a finding whose quoted span is not in the paper is \
discarded before anyone reads it. Advice that would apply to any paper is worthless \
here. Reporting nothing is a valid and useful answer.";

/// The bounds a review runs under. All of them are configurable and all
/// of them have a default that a person would accept without reading the
/// design note.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    /// Hard cap on rounds. The backstop under the convergence criterion.
    pub max_rounds: usize,
    /// Consecutive rounds with no finding that has not already been seen,
    /// after which the review is done.
    pub quiet_rounds: usize,
    /// How deep a chain of agents may go. See `budget::MAX_DEPTH_CEILING`.
    pub max_depth: usize,
    /// Total agents this review may start, at any depth. This is the
    /// bound that actually binds.
    pub max_agents: usize,
    /// How many agents try to refute each surviving finding.
    pub refuters_per_finding: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds {
            max_rounds: 4,
            quiet_rounds: 2,
            max_depth: 3,
            max_agents: 150,
            refuters_per_finding: 3,
        }
    }
}

/// Everything a review reads.
#[derive(Clone, Debug, Default)]
pub struct ReviewInputs {
    pub paper_path: String,
    pub paper: String,
    /// The track's recorded evidence, if there is any.
    pub evidence: Option<String>,
    /// A venue shortlist, if there is one.
    pub venues: Option<String>,
}

fn truncated(text: &str) -> (&str, bool) {
    if text.len() <= PAPER_INLINE_LIMIT {
        (text, false)
    } else {
        let mut end = PAPER_INLINE_LIMIT;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        (&text[..end], true)
    }
}

fn context_block(dimension: &Dimension, inputs: &ReviewInputs) -> String {
    let mut out = String::new();
    match dimension.needs {
        dimension::Needs::EvidenceRecord => {
            if let Some(evidence) = &inputs.evidence {
                let _ = write!(
                    out,
                    "\n\nThe recorded evidence for this work, which is the only thing a \
                     claim may be traced to:\n{evidence}"
                );
            }
        }
        dimension::Needs::VenueList => {
            if let Some(venues) = &inputs.venues {
                let _ = write!(
                    out,
                    "\n\nThe venue shortlist to judge fit against:\n{venues}"
                );
            }
        }
        dimension::Needs::Nothing => {}
    }
    out
}

fn already_said(seen: &[String]) -> String {
    if seen.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\nEarlier rounds already raised the findings below. Repeating one of them, in \
         any wording, is discarded. Look for what they missed:\n",
    );
    for claim in seen {
        let _ = writeln!(out, "- {claim}");
    }
    out
}

const REPLY_FORMAT: &str = "\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{\"findings\": [{\"severity\": \"blocking\" | \"major\" | \"minor\", \"claim\": \"<one sentence>\", \
\"anchor\": \"<at least five words copied verbatim from the paper>\", \"evidence\": \"<why the \
quoted text is a problem>\"}]}\n\
```\n\
An empty findings list is a valid answer and is better than a padded one.";

fn doer_prompt(dimension: &Dimension, inputs: &ReviewInputs, seen: &[String]) -> String {
    let (paper, _) = truncated(&inputs.paper);
    format!(
        "Dimension: {} ({}).\n\n{}{}{}\n\n{REPLY_FORMAT}\n\nThe paper, from `{}`:\n\n{}",
        dimension.title,
        dimension.key,
        dimension.brief,
        context_block(dimension, inputs),
        already_said(seen),
        inputs.paper_path,
        paper
    )
}

fn checker_prompt(
    dimension: &Dimension,
    inputs: &ReviewInputs,
    proposed: &[Finding],
    seen: &[String],
) -> String {
    let (paper, _) = truncated(&inputs.paper);
    let mut list = String::new();
    for (i, f) in proposed.iter().enumerate() {
        let _ = writeln!(
            list,
            "{}. [{}] {} (quoting: \"{}\")",
            i + 1,
            f.severity.label(),
            f.claim,
            f.anchor
        );
    }
    if list.is_empty() {
        list.push_str("(the reviewer reported nothing)\n");
    }
    format!(
        "Another reviewer has just gone over this paper on one dimension. You have two \
         jobs, in this order.\n\n\
         First, check their work. For each numbered finding below, decide whether the \
         paper really supports it. List the numbers of the ones it does not in an \
         \"unsupported\" array.\n\n\
         Second, find what they missed on the same dimension. Report those as findings.\n\n\
         Dimension: {} ({}).\n{}\n\nWhat the reviewer reported:\n{}{}{}\n\n\
         End your answer with a single fenced JSON block:\n\
         ```json\n\
         {{\"unsupported\": [<numbers>], \"findings\": [{{\"severity\": \"blocking\" | \"major\" | \
         \"minor\", \"claim\": \"<one sentence>\", \"anchor\": \"<at least five words copied \
         verbatim from the paper>\", \"evidence\": \"<why>\"}}]}}\n\
         ```\n\nThe paper, from `{}`:\n\n{}",
        dimension.title,
        dimension.key,
        dimension.brief,
        list,
        context_block(dimension, inputs),
        already_said(seen),
        inputs.paper_path,
        paper
    )
}

/// Run the review loop. Everything model-shaped is behind `dispatcher`,
/// so the loop, the deduplication, the budget, and the vote counting are
/// all exercisable without a model.
pub fn review(
    inputs: &ReviewInputs,
    selection: &Selection,
    bounds: &Bounds,
    dispatcher: &BudgetedDispatcher,
) -> ReviewReport {
    let normalized_paper = normalize(&inputs.paper);
    let plan = dimension::plan(
        selection,
        &Inputs {
            has_evidence_record: inputs.evidence.is_some(),
            has_venue_list: inputs.venues.is_some(),
        },
    );

    let mut tallies: BTreeMap<String, DimensionTally> = plan
        .selected
        .iter()
        .map(|d| {
            (
                d.key.to_string(),
                DimensionTally {
                    dimension: d.key.to_string(),
                    ..DimensionTally::default()
                },
            )
        })
        .collect();
    let bump = |tallies: &mut BTreeMap<String, DimensionTally>,
                key: &str,
                f: &dyn Fn(&mut DimensionTally)| {
        if let Some(t) = tallies.get_mut(key) {
            f(t);
        }
    };

    let mut convergence = Convergence::new(bounds.quiet_rounds, bounds.max_rounds);
    let mut kept: Vec<ReviewedFinding> = Vec::new();
    let mut seen_claims: Vec<String> = Vec::new();
    let mut unparseable = 0usize;
    let mut unverified_notes: Vec<String> = Vec::new();

    let stop = if plan.selected.is_empty() {
        // Nothing to run is not convergence. Saying "converged" here
        // would report an empty review as a finished one.
        Stop::BudgetExhausted { rounds_run: 0 }
    } else {
        loop {
            let mut proposals: Vec<Finding> = Vec::new();

            // The doer: one per dimension, looking for problems.
            let doer_jobs: Vec<Job> = plan
                .selected
                .iter()
                .map(|d| Job {
                    role: format!("{} reviewer", d.key),
                    prompt: doer_prompt(d, inputs, &seen_claims),
                })
                .collect();
            let doer_results = dispatcher.dispatch(DEPTH_REVIEWER, doer_jobs);

            let mut by_dimension: Vec<Vec<Finding>> = Vec::with_capacity(plan.selected.len());
            for (dimension, result) in plan.selected.iter().zip(doer_results.iter()) {
                let found = match result.text() {
                    Some(text) => match parse_reply(dimension.key, text) {
                        Ok(reply) => reply.findings,
                        Err(_) => {
                            unparseable += 1;
                            Vec::new()
                        }
                    },
                    None => Vec::new(),
                };
                bump(&mut tallies, dimension.key, &|t| t.proposed += found.len());
                by_dimension.push(found);
            }

            // The checker: given the doer's numbered list, drops what the
            // paper does not support and adds what the doer missed.
            let checker_jobs: Vec<Job> = plan
                .selected
                .iter()
                .zip(by_dimension.iter())
                .map(|(d, found)| Job {
                    role: format!("{} checker", d.key),
                    prompt: checker_prompt(d, inputs, found, &seen_claims),
                })
                .collect();
            let checker_results = dispatcher.dispatch(DEPTH_REVIEWER, checker_jobs);

            for ((dimension, doer_found), result) in plan
                .selected
                .iter()
                .zip(by_dimension.into_iter())
                .zip(checker_results.iter())
            {
                let reply = match result.text() {
                    Some(text) => match parse_reply(dimension.key, text) {
                        Ok(reply) => reply,
                        Err(_) => {
                            unparseable += 1;
                            Default::default()
                        }
                    },
                    None => Default::default(),
                };
                bump(&mut tallies, dimension.key, &|t| {
                    t.proposed += reply.findings.len()
                });
                for (i, finding) in doer_found.into_iter().enumerate() {
                    if reply.unsupported.contains(&(i + 1)) {
                        bump(&mut tallies, dimension.key, &|t| t.checker_rejected += 1);
                        // Still seen. A finding the checker threw out
                        // must not come back next round as new.
                        convergence.note_seen(&finding);
                        continue;
                    }
                    proposals.push(finding);
                }
                proposals.extend(reply.findings);
            }

            // Anchored in the paper, or gone. This is what stops a
            // reviewer filling the report with advice it could have
            // written without opening the file.
            let mut anchored = Vec::new();
            for finding in proposals {
                match anchor_is_in_paper(&finding, &normalized_paper) {
                    Ok(()) => anchored.push(finding),
                    Err(_) => {
                        let key = finding.dimension.clone();
                        bump(&mut tallies, &key, &|t| t.dropped_unanchored += 1);
                        convergence.note_seen(&finding);
                    }
                }
            }

            let outcome = convergence.register_round(anchored);
            for finding in &outcome.fresh {
                seen_claims.push(finding.claim.clone());
            }
            for key in &outcome.duplicates {
                bump(&mut tallies, key, &|t| t.duplicates += 1);
            }

            // Verification: each fresh finding handed to several agents
            // told to break it.
            for finding in outcome.fresh {
                let lenses = lenses_for(bounds.refuters_per_finding);
                let jobs: Vec<Job> = lenses
                    .iter()
                    .map(|lens| Job {
                        role: format!("refuter ({})", lens.key),
                        prompt: refuter_prompt(
                            lens,
                            &finding.dimension,
                            &finding.claim,
                            &finding.anchor,
                            &finding.evidence,
                        ),
                    })
                    .collect();
                let results = dispatcher.dispatch(DEPTH_VERIFIER, jobs);
                let mut votes = Vec::new();
                for (lens, result) in lenses.iter().zip(results.iter()) {
                    match result {
                        // A refusal is not a vote. Counting it as one
                        // would turn "we ran out of budget" into "the
                        // verifiers disagreed".
                        JobResult::Refused(_) => {}
                        JobResult::Done(text) => {
                            votes.push((lens.key.to_string(), Vote::parse(text)))
                        }
                        JobResult::Failed(_) => votes.push((lens.key.to_string(), Vote::Uncertain)),
                    }
                }
                let verdict = tally(&votes.iter().map(|(_, v)| *v).collect::<Vec<_>>());
                let key = finding.dimension.clone();
                match &verdict {
                    Verdict::Upheld => bump(&mut tallies, &key, &|t| t.surviving += 1),
                    Verdict::Refuted => bump(&mut tallies, &key, &|t| t.refuted += 1),
                    Verdict::Unverified(_) => {
                        unverified_notes.push(format!(
                            "a {key} finding was never verified: {}",
                            finding.claim
                        ));
                        bump(&mut tallies, &key, &|t| t.surviving += 1);
                    }
                }
                if matches!(verdict, Verdict::Refuted) {
                    continue;
                }
                kept.push(ReviewedFinding {
                    finding,
                    verification: Verification { verdict, votes },
                    round: convergence.rounds_run(),
                });
            }

            if dispatcher.budget().exhausted() {
                convergence.note_budget_exhausted();
            }
            if let Some(stop) = convergence.stop() {
                break stop;
            }
        }
    };

    rank(&mut kept);

    let refusals = dispatcher.budget().refusals();
    let mut not_covered: Vec<String> = refusals
        .iter()
        .take(REFUSAL_DETAIL_LIMIT)
        .map(|r| format!("{}: {}", r.what(), r.reason()))
        .collect();
    if refusals.len() > REFUSAL_DETAIL_LIMIT {
        not_covered.push(format!(
            "and {} further pieces of work the budget refused",
            refusals.len() - REFUSAL_DETAIL_LIMIT
        ));
    }
    not_covered.extend(unverified_notes);
    if truncated(&inputs.paper).1 {
        not_covered.push(format!(
            "the paper was cut at {PAPER_INLINE_LIMIT} characters before the reviewers saw \
             it, so the rest of it was not reviewed"
        ));
    }
    not_covered.push(
        "reviewer agents have local file tools only, so a citation was checked against what \
         is on disk and not against the published record"
            .to_string(),
    );

    let coverage_is_complete =
        stop.is_complete() && refusals.is_empty() && plan.skipped.is_empty() && unparseable == 0;

    ReviewReport {
        paper: inputs.paper_path.clone(),
        paper_words: inputs.paper.split_whitespace().count(),
        dimensions_run: plan.selected.iter().map(|d| d.key.to_string()).collect(),
        rounds_run: convergence.rounds_run(),
        max_rounds: bounds.max_rounds,
        coverage_is_complete,
        stop,
        agents_spent: dispatcher.budget().spent(),
        agent_budget: bounds.max_agents,
        max_depth: dispatcher.budget().max_depth(),
        findings: kept,
        tallies: tallies.into_values().collect(),
        skipped: plan
            .skipped
            .iter()
            .map(|(k, r)| (k.to_string(), r.clone()))
            .collect(),
        not_covered,
        unparseable_replies: unparseable,
    }
}

/// Run review for a track: read the paper, review it, write the report,
/// and checkpoint. Like co-write and deliver, neither checkpoint outcome
/// changes the track's status. The report is written to disk and the run
/// is recorded before the human is asked anything, so a rejected
/// checkpoint still leaves the review on the record.
#[allow(clippy::too_many_arguments)]
pub fn run(
    agent: &Agent,
    project: &Project,
    track_id: &str,
    paper_path: Option<&str>,
    venues_path: Option<&str>,
    selection: &Selection,
    bounds: &Bounds,
    checkpoint_mode: &CheckpointMode,
) -> Result<bool, ReviewError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(ReviewError::TrackKilled);
    }

    let track_dir = project.track_dir(track_id);
    let resolved = paper_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| track_dir.join("draft.md"));
    let paper = match std::fs::read_to_string(&resolved) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ReviewError::NoPaper(resolved.display().to_string()))
        }
        Err(e) => return Err(e.into()),
    };

    let venues = venues_path
        .map(std::path::PathBuf::from)
        .or_else(|| Some(track_dir.join("venues.md")))
        .and_then(|p| std::fs::read_to_string(p).ok());

    let inputs = ReviewInputs {
        paper_path: resolved.display().to_string(),
        paper,
        evidence: evidence_digest(project, track_id)?,
        venues,
    };

    let budget = Arc::new(Budget::new(bounds.max_depth, bounds.max_agents));
    let pool = crate::tools::subagent::SubagentPool::new();
    let runner: Arc<dyn Dispatcher> = Arc::new(PoolDispatcher::new(
        agent.config(),
        pool,
        budget.clone(),
        REVIEW_PREAMBLE,
    ));
    let dispatcher = BudgetedDispatcher::new(runner, budget);

    let report = review(&inputs, selection, bounds, &dispatcher);

    std::fs::create_dir_all(&track_dir)?;
    let md_path = track_dir.join("review.md");
    std::fs::write(&md_path, report::render_markdown(&report))?;
    // The markdown is for a person. The JSON is the record: severities,
    // verdicts, vote breakdowns and the budget accounting survive it
    // intact, and re-parsing the prose would not recover them.
    let json_path = track_dir.join("review.json");
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    )?;

    let (blocking, major, minor) = report::severity_counts(&report);
    let prompt = format!(
        "review: {} findings ({blocking} blocking, {major} major, {minor} minor) written to {}. {} Accept this review?",
        report.findings.len(),
        md_path.display(),
        report.stop.describe()
    );
    let approved = project
        .store
        .record_checkpoint(track_id, "review", checkpoint_mode, &prompt)?;
    Ok(approved)
}

/// The track's recorded evidence, rendered for a reviewer to trace claims
/// against. `None` when the track has recorded nothing yet, which makes
/// the traceability dimension unrunnable rather than vacuously clean.
fn evidence_digest(
    project: &Project,
    track_id: &str,
) -> Result<Option<String>, zorp_track::TrackError> {
    let mut out = String::new();
    match project.store.get_validation(track_id) {
        Ok(v) => {
            let _ = writeln!(
                out,
                "Validation verdict: {}\nRedundancy: {:.0}/100. Feasibility: {:.0}/100.",
                v.verdict, v.redundancy_score, v.feasibility_score
            );
            for c in v
                .redundancy_citations
                .iter()
                .chain(v.feasibility_citations.iter())
            {
                let _ = writeln!(out, "- citation: \"{}\" ({})", c.text, c.source);
            }
        }
        Err(zorp_track::TrackError::NotFound {
            kind: "validation", ..
        }) => {}
        Err(e) => return Err(e),
    }

    let metrics = project.store.metrics_for_track(track_id)?;
    if !metrics.is_empty() {
        out.push_str("Recorded metrics:\n");
        for (experiment_id, key, value) in &metrics {
            let rendered = match value {
                zorp_track::experiment::MetricValue::Number(n) => n.to_string(),
                zorp_track::experiment::MetricValue::Text(s) => s.clone(),
                zorp_track::experiment::MetricValue::Bool(b) => b.to_string(),
            };
            let _ = writeln!(out, "- [{experiment_id}] {key} = {rendered}");
        }
    }

    Ok(if out.is_empty() { None } else { Some(out) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const PAPER: &str = "We observe a 14% reduction in latency across the suite. \
        The evaluation ran once on a single machine. Prior work is discussed in section 2. \
        Table 3 lists throughput for each configuration. \
        Our sampler uses a temperature of 0.7 throughout. \
        We release no code alongside this paper.";

    /// A dispatcher that answers from a script keyed on the agent's role
    /// prefix, so a test can drive the whole loop without a model.
    struct Scripted {
        by_role: Vec<(String, String)>,
        default: String,
        calls: Mutex<Vec<(usize, String)>>,
    }

    impl Scripted {
        fn new(default: &str) -> Self {
            Scripted {
                by_role: Vec::new(),
                default: default.to_string(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn on(mut self, role_contains: &str, reply: &str) -> Self {
            self.by_role
                .push((role_contains.to_string(), reply.to_string()));
            self
        }
    }

    impl Dispatcher for Scripted {
        fn dispatch(&self, depth: usize, jobs: Vec<Job>) -> Vec<JobResult> {
            let mut calls = self.calls.lock().unwrap();
            jobs.iter()
                .map(|job| {
                    calls.push((depth, job.role.clone()));
                    let reply = self
                        .by_role
                        .iter()
                        .find(|(needle, _)| job.role.contains(needle.as_str()))
                        .map(|(_, r)| r.clone())
                        .unwrap_or_else(|| self.default.clone());
                    JobResult::Done(reply)
                })
                .collect()
        }
    }

    fn findings_json(claim: &str, anchor: &str) -> String {
        format!(
            "```json\n{{\"findings\": [{{\"severity\": \"major\", \"claim\": \"{claim}\", \
             \"anchor\": \"{anchor}\", \"evidence\": \"it matters\"}}]}}\n```"
        )
    }

    const NOTHING: &str = "```json\n{\"findings\": []}\n```";
    const UPHELD: &str = "```json\n{\"vote\": \"upheld\", \"reason\": \"could not break it\"}\n```";
    const REFUTED: &str =
        "```json\n{\"vote\": \"refuted\", \"reason\": \"the appendix covers it\"}\n```";

    fn inputs() -> ReviewInputs {
        ReviewInputs {
            paper_path: "paper.md".to_string(),
            paper: PAPER.to_string(),
            evidence: None,
            venues: None,
        }
    }

    fn one_dimension() -> Selection {
        Selection::parse("statistical-validity").unwrap()
    }

    fn wire(inner: Scripted, bounds: &Bounds) -> (BudgetedDispatcher, Arc<Budget>) {
        let budget = Arc::new(Budget::new(bounds.max_depth, bounds.max_agents));
        (
            BudgetedDispatcher::new(Arc::new(inner), budget.clone()),
            budget,
        )
    }

    /// The requirement the whole design turns on: a reviewer that always
    /// finds something tells you nothing, so finding nothing has to be
    /// expressible and has to converge.
    #[test]
    fn a_clean_paper_produces_zero_findings_and_converges() {
        let bounds = Bounds::default();
        let (d, _) = wire(Scripted::new(NOTHING), &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert!(report.findings.is_empty());
        assert!(matches!(report.stop, Stop::Converged { .. }));
        assert_eq!(report.rounds_run, 2, "two quiet rounds and no more");
    }

    #[test]
    fn an_upheld_finding_survives_into_the_report() {
        let bounds = Bounds::default();
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json("no error bars", "a 14% reduction in latency across"),
            )
            .on("refuter", UPHELD);
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].verification.verdict, Verdict::Upheld);
        assert_eq!(report.findings[0].verification.votes.len(), 3);
    }

    /// Verification is not a formality: a finding the refuters break does
    /// not reach the report.
    #[test]
    fn a_refuted_finding_is_kept_out_of_the_report() {
        let bounds = Bounds::default();
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json("no error bars", "a 14% reduction in latency across"),
            )
            .on("refuter", REFUTED);
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert!(report.findings.is_empty());
        assert_eq!(report.tallies[0].refuted, 1);
    }

    /// Distinct lenses, not the same question three times.
    #[test]
    fn each_refuter_gets_a_different_lens() {
        let bounds = Bounds::default();
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json("no error bars", "a 14% reduction in latency across"),
            )
            .on("refuter", UPHELD);
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        let lenses: Vec<&str> = report.findings[0]
            .verification
            .votes
            .iter()
            .map(|(l, _)| l.as_str())
            .collect();
        let mut sorted = lenses.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), lenses.len());
    }

    /// The termination guarantee. A reviewer that answers "yes, one more"
    /// forever still stops, and the report says the bound was hit.
    #[test]
    fn a_reviewer_that_never_runs_out_still_stops_at_the_round_cap() {
        let bounds = Bounds {
            max_rounds: 3,
            max_agents: 10_000,
            ..Bounds::default()
        };
        // A genuinely different finding every round: different sentence
        // of the paper, different words. Nothing here is a repeat, so
        // only the round cap can end this.
        const DISTINCT: [(&str, &str); 4] = [
            (
                "the headline figure carries no spread",
                "a 14% reduction in latency across the suite",
            ),
            (
                "one machine cannot support a general claim",
                "The evaluation ran once on a single machine",
            ),
            (
                "no units accompany that table",
                "Table 3 lists throughput for each configuration",
            ),
            (
                "nothing justifies that sampler setting",
                "Our sampler uses a temperature of 0.7 throughout",
            ),
        ];
        struct Endless {
            round: Mutex<usize>,
        }
        impl Dispatcher for Endless {
            fn dispatch(&self, _depth: usize, jobs: Vec<Job>) -> Vec<JobResult> {
                jobs.iter()
                    .map(|job| {
                        if job.role.contains("refuter") {
                            return JobResult::Done(UPHELD.to_string());
                        }
                        if job.role.contains("checker") {
                            return JobResult::Done(NOTHING.to_string());
                        }
                        let mut n = self.round.lock().unwrap();
                        let (claim, anchor) = DISTINCT[*n % DISTINCT.len()];
                        *n += 1;
                        JobResult::Done(findings_json(claim, anchor))
                    })
                    .collect()
            }
        }
        let budget = Arc::new(Budget::new(bounds.max_depth, bounds.max_agents));
        let d = BudgetedDispatcher::new(
            Arc::new(Endless {
                round: Mutex::new(0),
            }),
            budget,
        );
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert_eq!(report.stop, Stop::RoundCap { max_rounds: 3 });
        assert!(!report.coverage_is_complete);
        assert!(report::render_markdown(&report).contains("This review is incomplete"));
    }

    /// A reviewer resubmitting the same finding every round must not keep
    /// the loop alive. Rounds two and three are quiet even though the
    /// reviewer keeps talking.
    #[test]
    fn a_repeated_finding_does_not_keep_the_loop_going() {
        let bounds = Bounds {
            max_rounds: 10,
            ..Bounds::default()
        };
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json(
                    "no error bars are reported anywhere",
                    "a 14% reduction in latency across",
                ),
            )
            .on("refuter", REFUTED);
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert_eq!(report.stop, Stop::Converged { quiet_rounds: 2 });
        assert_eq!(report.rounds_run, 3, "one productive round then two quiet");
    }

    /// Generic advice never reaches verification, so it never costs
    /// anything and never pads the report.
    #[test]
    fn a_finding_that_quotes_nothing_in_the_paper_is_dropped_before_verification() {
        let bounds = Bounds::default();
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json(
                    "consider adding error bars",
                    "you should always report variance",
                ),
            )
            .on("refuter", UPHELD);
        let (d, budget) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert!(report.findings.is_empty());
        // Proposed and dropped in each of the two rounds. Counted per
        // proposal, because a dimension that keeps producing advice with
        // nothing behind it should be visible as doing so.
        assert_eq!(report.tallies[0].dropped_unanchored, 2);
        // Two rounds of a doer and a checker, and not one refuter.
        assert_eq!(budget.spent(), 4);
    }

    #[test]
    fn a_finding_the_checker_rejects_is_dropped_before_verification() {
        let bounds = Bounds::default();
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json("no error bars", "a 14% reduction in latency across"),
            )
            .on(
                "checker",
                "```json\n{\"unsupported\": [1], \"findings\": []}\n```",
            )
            .on("refuter", UPHELD);
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert!(report.findings.is_empty());
        // Once per round: the reviewer proposed it twice and the checker
        // threw it out twice.
        assert_eq!(report.tallies[0].checker_rejected, 2);
    }

    /// Running out of budget must stop the review and say so, not quietly
    /// review less and present the result as a finished pass.
    #[test]
    fn exhausting_the_budget_stops_the_review_and_reports_what_was_missed() {
        let bounds = Bounds {
            max_agents: 2,
            max_rounds: 10,
            ..Bounds::default()
        };
        let script = Scripted::new(NOTHING).on(
            "statistical-validity reviewer",
            &findings_json("no error bars", "a 14% reduction in latency across"),
        );
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert!(matches!(report.stop, Stop::BudgetExhausted { .. }));
        assert!(!report.coverage_is_complete);
        let md = report::render_markdown(&report);
        assert!(md.contains("What this review did not cover"));
        assert!(md.contains("This review is incomplete"));
    }

    /// A finding nobody could verify is reported as unverified. Dropping
    /// it would hide it; calling it upheld would invent support.
    #[test]
    fn a_finding_the_budget_could_not_verify_is_reported_as_unverified() {
        // Two agents pays for one doer and one checker and nothing else.
        let bounds = Bounds {
            max_agents: 2,
            max_rounds: 10,
            ..Bounds::default()
        };
        let script = Scripted::new(NOTHING).on(
            "statistical-validity reviewer",
            &findings_json("no error bars", "a 14% reduction in latency across"),
        );
        let (d, _) = wire(script, &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert_eq!(report.findings.len(), 1);
        assert!(matches!(
            report.findings[0].verification.verdict,
            Verdict::Unverified(_)
        ));
    }

    #[test]
    fn dimensions_are_run_at_depth_one_and_refuters_at_depth_two() {
        let bounds = Bounds::default();
        let script = Scripted::new(NOTHING)
            .on(
                "statistical-validity reviewer",
                &findings_json("no error bars", "a 14% reduction in latency across"),
            )
            .on("refuter", UPHELD);
        let inner = Arc::new(script);
        let budget = Arc::new(Budget::new(bounds.max_depth, bounds.max_agents));
        let d = BudgetedDispatcher::new(inner.clone(), budget);
        review(&inputs(), &one_dimension(), &bounds, &d);
        let calls = inner.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|(depth, role)| *depth == 1 && role.contains("reviewer")));
        assert!(calls
            .iter()
            .any(|(depth, role)| *depth == 2 && role.contains("refuter")));
        assert!(calls.iter().all(|(depth, _)| *depth <= 2));
    }

    #[test]
    fn a_selection_with_no_runnable_dimension_reports_rather_than_claiming_convergence() {
        let bounds = Bounds::default();
        let (d, _) = wire(Scripted::new(NOTHING), &bounds);
        let report = review(
            &inputs(),
            &Selection::parse("venue-fit").unwrap(),
            &bounds,
            &d,
        );
        assert!(!report.coverage_is_complete);
        assert_eq!(report.rounds_run, 0);
        assert!(report.skipped.iter().any(|(k, _)| k == "venue-fit"));
    }

    #[test]
    fn an_unreadable_reviewer_answer_is_counted_not_read_as_silence() {
        let bounds = Bounds::default();
        let (d, _) = wire(Scripted::new("I could not do this"), &bounds);
        let report = review(&inputs(), &one_dimension(), &bounds, &d);
        assert!(report.unparseable_replies > 0);
        assert!(!report.coverage_is_complete);
        assert!(report::render_markdown(&report).contains("could not be read"));
    }
}
