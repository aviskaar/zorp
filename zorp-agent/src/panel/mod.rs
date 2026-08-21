//! A panel of independent reviewers, run concurrently over one target.
//!
//! Adversarial review, meaning several agents look at the same work
//! from deliberately different angles and none of them sees what the
//! others said. That last part is the whole mechanism. Reviewers that
//! read each other converge, and a panel that converges is one reviewer
//! with extra cost. Agreement is worth something only when it was
//! reached separately, so it is measured from outside, in code, and
//! never negotiated inside the panel.
//!
//! **This is not `critique`.** Critique is a gate: it audits a draft
//! against a track's own evidence record, the audit is arithmetic, and
//! it refuses if the record moved underneath it. The panel is a reader.
//! It produces opinions, it changes nothing, and it can be wrong. Two
//! different jobs, and conflating them would let an opinion block a
//! deliverable the evidence record was happy with.
//!
//! # What a reviewer may do
//!
//! Less than the agent that launched it, always. A reviewer gets a
//! read-only tool set, inherits the launching approval mode, and cannot
//! be given anything the panel was not given. The rule is the one
//! `zorp-skill` already follows for skill bodies: a thing you point the
//! agent at can grant no tool, loosen no approval, and bypass no
//! denylist entry.
//!
//! # What is deliberately absent
//!
//! There is no model-callable tool to spawn a reviewer. A panel is
//! launched by a person, from the CLI or the browser, and a reviewer
//! runs with no panel of its own. A model that can spawn agents can
//! spawn agents that spawn agents, and nothing in the loop bounds that.
//! `agent.rs` already carries a test asserting a filtered agent has no
//! `spawn_subagent`; this module does not add one.

mod verdict;

pub use verdict::{
    parse_verdict, Agreement, PanelFinding, PanelReport, ParseError, ReviewerFailure,
    ReviewerVerdict, Severity,
};

use crate::agent::{Agent, Outcome};
use crate::approval::ApprovalMode;
use crate::model::Model;
use crate::sandbox::CancelToken;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};

/// One reviewer's angle.
///
/// A name and the instruction that makes it different from its
/// neighbours. Diversity is the point: three reviewers told the same
/// thing are one reviewer sampled three times, and their agreement
/// measures the model's consistency rather than the work's quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lens {
    pub name: String,
    pub instruction: String,
}

impl Lens {
    pub fn new(name: &str, instruction: &str) -> Self {
        Lens {
            name: name.to_string(),
            instruction: instruction.to_string(),
        }
    }
}

/// The default panel for reviewing research work.
///
/// Five angles, chosen so that a defect of each kind has somebody whose
/// job it is to find it. They are code-defined and a model never picks
/// them: letting the work under review influence who reviews it is the
/// same failure as letting it influence what counts as evidence.
pub fn default_lenses() -> Vec<Lens> {
    vec![
        Lens::new(
            "evidence",
            "Check every number, quantity and named result against what the \
             material itself supports. Flag anything asserted without a source \
             in the material, and anything whose source does not say what it is \
             cited for. A number that appears from nowhere is blocking.",
        ),
        Lens::new(
            "reasoning",
            "Check whether the conclusions follow from the stated premises. \
             Look for a claim that is stronger than its evidence, a causal claim \
             resting on a correlation, and a conclusion that would survive the \
             evidence being reversed.",
        ),
        Lens::new(
            "alternatives",
            "Argue the other side. For the central claim, state the best \
             explanation that is not the one given, and say what in the material \
             rules it out. If nothing rules it out, that is a finding.",
        ),
        Lens::new(
            "method",
            "Check how the result was produced: what was measured, under what \
             conditions, how many times, and what was held fixed. Flag a \
             comparison whose conditions differ, a sample too small for the \
             claim, and a metric that does not measure what the claim needs.",
        ),
        Lens::new(
            "completeness",
            "Say what is missing. A limitation not stated, a case not covered, \
             a result reported without the one alongside it that would make it \
             interpretable, a question raised and not answered.",
        ),
    ]
}

/// Tools a reviewer is allowed.
///
/// Read-only, and named one at a time rather than derived by excluding
/// the dangerous ones. An allow-list stays correct when a new tool is
/// added; a deny-list silently grants it.
///
/// No `write_file`, no `apply_patch`, no `run_command`. A reviewer's
/// output is an opinion, and an opinion that can edit the thing it is
/// reviewing is not a review.
pub fn reviewer_tools() -> Vec<String> {
    vec![
        "read_file".to_string(),
        "list_files".to_string(),
        "search_text".to_string(),
        "git_diff".to_string(),
        "git_status".to_string(),
    ]
}

/// What the panel is looking at.
#[derive(Debug, Clone)]
pub struct Target {
    /// A short name, carried into the report so it says what it is
    /// about.
    pub label: String,
    /// The material itself.
    pub body: String,
}

/// How the panel runs.
#[derive(Debug, Clone)]
pub struct PanelConfig {
    pub lenses: Vec<Lens>,
    /// Steps each reviewer may take. Lower than a working agent's on
    /// purpose: a reviewer reads and answers, and one that is still
    /// going after a dozen steps has started doing the work instead of
    /// reviewing it.
    pub max_steps: usize,
    /// How many reviewers run at once.
    ///
    /// Bounded because each one is a live model connection, and a panel
    /// is the one place in zorp where a single click multiplies requests
    /// by a number the user chose. Four is a compromise: enough that a
    /// five-lens panel is essentially parallel, small enough that a
    /// rate-limited endpoint does not reject half the panel.
    pub max_concurrent: usize,
}

impl Default for PanelConfig {
    fn default() -> Self {
        PanelConfig {
            lenses: default_lenses(),
            max_steps: 12,
            max_concurrent: 4,
        }
    }
}

/// Told about each reviewer as it starts, finishes or fails.
///
/// The panel's whole point is that it takes a while, so a caller that
/// can only see the finished report has nothing to show for the first
/// minute. The browser drives its live view off this.
///
/// Implementations are called from several reviewer threads at once and
/// must handle that themselves.
pub trait PanelObserver: Send + Sync {
    fn reviewer_started(&self, lens: &str);
    fn reviewer_finished(&self, verdict: &ReviewerVerdict);
    fn reviewer_failed(&self, lens: &str, why: &str);
}

/// An observer that does nothing, for callers that only want the report.
pub struct SilentObserver;

impl PanelObserver for SilentObserver {
    fn reviewer_started(&self, _lens: &str) {}
    fn reviewer_finished(&self, _verdict: &ReviewerVerdict) {}
    fn reviewer_failed(&self, _lens: &str, _why: &str) {}
}

/// The instruction one reviewer receives.
///
/// Assembled here, from the lens and the target, and never from another
/// reviewer's output. That is what makes the verdicts independent, and
/// it is enforced by this function being the only thing that builds a
/// reviewer's prompt.
fn reviewer_prompt(lens: &Lens, target: &Target) -> String {
    format!(
        "You are one reviewer on a panel. Other reviewers are looking at the same \
material from different angles; you will not see what they say and they will not \
see what you say. Do not try to guess what they will find or to cover their \
angles. Review only from yours.\n\n\
Your angle: {lens_name}\n{instruction}\n\n\
Be specific. A finding must name where in the material it applies, in the \
`locus` field, and two reviewers who found the same problem should be naming \
the same place. Do not pad the list: a finding you are not willing to defend \
weakens the ones you are. Finding nothing is an acceptable answer.\n\n\
Severity is one of \"blocking\" (the work cannot be used as it stands), \
\"concern\" (probably wrong, or right but unsupported), or \"note\".\n\n\
--- BEGIN MATERIAL: {label} ---\n{body}\n--- END MATERIAL ---\n\n\
End your answer with a single fenced JSON block, exactly this shape:\n\
```json\n\
{{\"findings\": [{{\"severity\": \"concern\", \"claim\": \"<what is wrong>\", \
\"locus\": \"<where>\"}}]}}\n\
```",
        lens_name = lens.name,
        instruction = lens.instruction,
        label = target.label,
        body = target.body,
    )
}

/// A counting semaphore, so a large panel does not open every model
/// connection at once.
struct Permits {
    available: Mutex<usize>,
    freed: Condvar,
}

impl Permits {
    fn new(n: usize) -> Self {
        Permits {
            available: Mutex::new(n.max(1)),
            freed: Condvar::new(),
        }
    }

    fn acquire(&self) {
        let mut available = self.available.lock().unwrap();
        while *available == 0 {
            available = self.freed.wait(available).unwrap();
        }
        *available -= 1;
    }

    fn release(&self) {
        *self.available.lock().unwrap() += 1;
        self.freed.notify_one();
    }
}

/// Run the panel and collect what came back.
///
/// Every requested lens produces exactly one entry, in either
/// `verdicts` or `failures`. A reviewer that fell over is reported
/// rather than dropped: a panel of five where two failed is not a panel
/// of three, and a report that cannot tell those apart lets "every
/// reviewer agreed" mean "the one that ran agreed".
///
/// Verdicts come back in lens order, not in finishing order, so two runs
/// of the same panel produce comparable reports.
pub fn run(
    model: &dyn Model,
    target: &Target,
    config: &PanelConfig,
    cwd: PathBuf,
    cancel: CancelToken,
    approval: ApprovalMode,
    observer: &dyn PanelObserver,
) -> PanelReport {
    let requested = config.lenses.len();
    let permits = Permits::new(config.max_concurrent);
    let tools = reviewer_tools();

    // Indexed results, so the order of the report does not depend on
    // which reviewer happened to finish first.
    let slots: Vec<Mutex<Option<Result<ReviewerVerdict, ReviewerFailure>>>> =
        (0..requested).map(|_| Mutex::new(None)).collect();

    let cwd = &cwd;
    std::thread::scope(|scope| {
        for (index, lens) in config.lenses.iter().enumerate() {
            let permits = &permits;
            let slots = &slots;
            let tools = &tools;
            let cancel = cancel.clone();
            let approval = approval.clone();
            scope.spawn(move || {
                permits.acquire();
                // Checked after the permit rather than before, so a stop
                // pressed while a reviewer was queued is noticed instead
                // of the reviewer starting anyway.
                if cancel.load(Ordering::SeqCst) {
                    permits.release();
                    *slots[index].lock().unwrap() = Some(Err(ReviewerFailure {
                        lens: lens.name.clone(),
                        why: "stopped before this reviewer started".to_string(),
                    }));
                    return;
                }
                observer.reviewer_started(&lens.name);
                let outcome = review(
                    model,
                    lens,
                    target,
                    config.max_steps,
                    cwd.clone(),
                    cancel,
                    approval,
                    tools,
                );
                permits.release();

                let slot = match outcome {
                    Ok(v) => {
                        observer.reviewer_finished(&v);
                        Ok(v)
                    }
                    Err(why) => {
                        observer.reviewer_failed(&lens.name, &why);
                        Err(ReviewerFailure {
                            lens: lens.name.clone(),
                            why,
                        })
                    }
                };
                *slots[index].lock().unwrap() = Some(slot);
            });
        }
    });

    let mut verdicts = Vec::new();
    let mut failures = Vec::new();
    for slot in slots {
        match slot.into_inner().unwrap() {
            Some(Ok(v)) => verdicts.push(v),
            Some(Err(f)) => failures.push(f),
            // Not reachable: the scope above joins every thread, and
            // each one fills its slot on every path. Recorded as a
            // failure rather than dropped, because a silently missing
            // reviewer is the one thing this function must not produce.
            None => failures.push(ReviewerFailure {
                lens: "unknown".to_string(),
                why: "reviewer thread ended without filling its slot".to_string(),
            }),
        }
    }

    PanelReport {
        target: target.label.clone(),
        verdicts,
        failures,
        lenses_requested: requested,
        stopped: cancel.load(Ordering::SeqCst),
    }
}

/// One reviewer, start to finish.
#[allow(clippy::too_many_arguments)]
fn review(
    model: &dyn Model,
    lens: &Lens,
    target: &Target,
    max_steps: usize,
    cwd: PathBuf,
    cancel: CancelToken,
    approval: ApprovalMode,
    tools: &[String],
) -> Result<ReviewerVerdict, String> {
    let mut agent = Agent::new(
        model.clone_box(),
        REVIEWER_SYSTEM_PROMPT,
        max_steps,
        cwd,
        cancel,
        approval,
    )
    // The allow-list, not the full set. A reviewer that can write is
    // not a reviewer.
    .register_builtins_filtered(Some(tools));

    let answer = match agent.run(&reviewer_prompt(lens, target)) {
        Outcome::Complete(text) => text,
        other => return Err(other.describe()),
    };
    let findings = parse_verdict(&answer).map_err(|e| e.to_string())?;
    Ok(ReviewerVerdict {
        lens: lens.name.clone(),
        findings,
        answer,
    })
}

/// What a reviewer is, told to the reviewer.
///
/// Short, because the lens carries the specifics and repeating them
/// here would let the two drift. The one thing it insists on is the
/// thing a reviewer is most tempted to do: fix the work rather than
/// report on it.
const REVIEWER_SYSTEM_PROMPT: &str = "\
You are a reviewer. You read material and report what is wrong with it. \
You do not rewrite it, you do not fix it, and you do not have the tools to. \
Report what you find and stop.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssistantMessage, Message};
    use crate::sandbox::cancel_token;
    use crate::BoxErr;
    use serde_json::Value;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// A model that answers every reviewer with the same text, and
    /// counts how many times it was asked.
    struct Canned {
        answer: String,
        calls: Arc<AtomicUsize>,
        /// Set while a call is in flight, so a test can see how many
        /// reviewers overlapped.
        in_flight: Arc<AtomicUsize>,
        peak: Arc<Mutex<usize>>,
    }

    impl Canned {
        fn new(answer: &str) -> Self {
            Canned {
                answer: answer.to_string(),
                calls: Arc::new(AtomicUsize::new(0)),
                in_flight: Arc::new(AtomicUsize::new(0)),
                peak: Arc::new(Mutex::new(0)),
            }
        }
    }

    impl Model for Canned {
        fn complete(&self, _m: &[Message], _t: &[Value]) -> Result<AssistantMessage, BoxErr> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            {
                let mut peak = self.peak.lock().unwrap();
                *peak = (*peak).max(now);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(AssistantMessage {
                content: self.answer.clone(),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }

        fn clone_box(&self) -> Box<dyn Model> {
            Box::new(Canned {
                answer: self.answer.clone(),
                calls: Arc::clone(&self.calls),
                in_flight: Arc::clone(&self.in_flight),
                peak: Arc::clone(&self.peak),
            })
        }
    }

    fn target() -> Target {
        Target {
            label: "draft.md".into(),
            body: "The accuracy was 0.91.".into(),
        }
    }

    fn lenses(n: usize) -> Vec<Lens> {
        (0..n)
            .map(|i| Lens::new(&format!("lens{i}"), "look at it"))
            .collect()
    }

    fn config(n: usize) -> PanelConfig {
        PanelConfig {
            lenses: lenses(n),
            max_steps: 4,
            max_concurrent: 4,
        }
    }

    fn run_panel(model: &dyn Model, config: &PanelConfig) -> PanelReport {
        let dir = tempdir().unwrap();
        run(
            model,
            &target(),
            config,
            dir.path().to_path_buf(),
            cancel_token(),
            ApprovalMode::AutoApprove,
            &SilentObserver,
        )
    }

    const GOOD: &str = "```json\n{\"findings\": [{\"severity\": \"concern\", \
        \"claim\": \"0.91 is not in the record\", \"locus\": \"line 1\"}]}\n```";

    #[test]
    fn every_lens_produces_exactly_one_verdict() {
        let model = Canned::new(GOOD);
        let report = run_panel(&model, &config(3));
        assert_eq!(report.verdicts.len(), 3);
        assert!(report.failures.is_empty());
        assert!(report.is_complete());
    }

    /// Reports must be comparable between runs, so the order cannot be
    /// whichever reviewer happened to finish first.
    #[test]
    fn verdicts_come_back_in_lens_order() {
        let model = Canned::new(GOOD);
        let report = run_panel(&model, &config(4));
        let names: Vec<&str> = report.verdicts.iter().map(|v| v.lens.as_str()).collect();
        assert_eq!(names, vec!["lens0", "lens1", "lens2", "lens3"]);
    }

    /// A reviewer that fell over is reported, not dropped. Otherwise a
    /// corroboration count silently describes a smaller panel than the
    /// one that was asked for.
    #[test]
    fn a_reviewer_that_answers_in_prose_is_a_reported_failure() {
        let model = Canned::new("Looks fine to me.");
        let report = run_panel(&model, &config(3));
        assert!(report.verdicts.is_empty());
        assert_eq!(report.failures.len(), 3);
        assert_eq!(report.lenses_requested, 3);
        assert!(!report.is_complete());
        assert!(report.failures[0].why.contains("fenced"), "{report:?}");
    }

    #[test]
    fn a_panel_runs_its_reviewers_concurrently() {
        let model = Canned::new(GOOD);
        run_panel(&model, &config(4));
        assert!(
            *model.peak.lock().unwrap() > 1,
            "reviewers ran one at a time; the panel is sequential"
        );
    }

    /// Each reviewer is a live model connection, and a panel is the one
    /// place a single click multiplies requests by a number the user
    /// chose.
    #[test]
    fn concurrency_is_bounded_by_the_configured_limit() {
        let model = Canned::new(GOOD);
        let mut config = config(8);
        config.max_concurrent = 2;
        let report = run_panel(&model, &config);
        assert_eq!(report.verdicts.len(), 8);
        assert!(
            *model.peak.lock().unwrap() <= 2,
            "peak was {}, limit was 2",
            model.peak.lock().unwrap()
        );
    }

    /// The independence guarantee, checked on the only thing that could
    /// break it: what a reviewer is told.
    #[test]
    fn a_reviewer_prompt_contains_no_other_reviewer() {
        let all = default_lenses();
        let prompt = reviewer_prompt(&all[0], &target());
        for other in &all[1..] {
            assert!(
                !prompt.contains(&other.instruction),
                "the {} prompt carries the {} instruction",
                all[0].name,
                other.name
            );
        }
        assert!(prompt.contains("you will not see what they say"));
    }

    /// A reviewer's output is an opinion, and an opinion that can edit
    /// the thing it is reviewing is not a review.
    #[test]
    fn a_reviewer_has_no_tool_that_writes() {
        let tools = reviewer_tools();
        for forbidden in [
            "write_file",
            "apply_patch",
            "run_command",
            "start_background_process",
            "kill_background_process",
            "take_note",
        ] {
            assert!(
                !tools.contains(&forbidden.to_string()),
                "a reviewer must not have {forbidden}"
            );
        }
    }

    /// The allow-list has to actually reach the agent, not just exist.
    #[test]
    fn the_reviewer_allow_list_reaches_the_agent() {
        let dir = tempdir().unwrap();
        let agent = Agent::new(
            Box::new(Canned::new(GOOD)),
            REVIEWER_SYSTEM_PROMPT,
            4,
            dir.path().to_path_buf(),
            cancel_token(),
            ApprovalMode::AutoApprove,
        )
        .register_builtins_filtered(Some(&reviewer_tools()));
        let names = agent.tool_names();
        assert!(names.contains(&"read_file".to_string()));
        assert!(!names.contains(&"write_file".to_string()));
        assert!(!names.contains(&"run_command".to_string()));
        assert!(!names.contains(&"apply_patch".to_string()));
    }

    /// A stop pressed while reviewers are queued must not let the queued
    /// ones start anyway.
    #[test]
    fn a_stopped_panel_reports_itself_as_stopped() {
        let model = Canned::new(GOOD);
        let dir = tempdir().unwrap();
        let cancel = cancel_token();
        cancel.store(true, Ordering::SeqCst);
        let report = run(
            &model,
            &target(),
            &config(3),
            dir.path().to_path_buf(),
            cancel,
            ApprovalMode::AutoApprove,
            &SilentObserver,
        );
        assert!(report.stopped);
        assert!(!report.is_complete());
        assert_eq!(report.failures.len(), 3);
    }

    /// The lenses have to differ, or the panel measures the model's
    /// consistency rather than the work's quality.
    #[test]
    fn the_default_lenses_are_distinct() {
        let all = default_lenses();
        assert!(all.len() >= 3, "a panel of two is not a panel");
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.name, b.name);
                assert_ne!(a.instruction, b.instruction);
            }
        }
    }

    #[test]
    fn an_observer_hears_about_every_reviewer() {
        struct Counting {
            started: Mutex<Vec<String>>,
            finished: Mutex<Vec<String>>,
            failed: Mutex<Vec<String>>,
        }
        impl PanelObserver for Counting {
            fn reviewer_started(&self, lens: &str) {
                self.started.lock().unwrap().push(lens.to_string());
            }
            fn reviewer_finished(&self, v: &ReviewerVerdict) {
                self.finished.lock().unwrap().push(v.lens.clone());
            }
            fn reviewer_failed(&self, lens: &str, _why: &str) {
                self.failed.lock().unwrap().push(lens.to_string());
            }
        }
        let observer = Counting {
            started: Mutex::new(Vec::new()),
            finished: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        };
        let model = Canned::new(GOOD);
        let dir = tempdir().unwrap();
        run(
            &model,
            &target(),
            &config(3),
            dir.path().to_path_buf(),
            cancel_token(),
            ApprovalMode::AutoApprove,
            &observer,
        );
        assert_eq!(observer.started.lock().unwrap().len(), 3);
        assert_eq!(observer.finished.lock().unwrap().len(), 3);
        assert!(observer.failed.lock().unwrap().is_empty());
    }

    #[test]
    fn the_target_body_reaches_the_reviewer() {
        let prompt = reviewer_prompt(&default_lenses()[0], &target());
        assert!(prompt.contains("The accuracy was 0.91."));
        assert!(prompt.contains("draft.md"));
    }
}
