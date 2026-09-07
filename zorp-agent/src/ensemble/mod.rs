//! Ensemble: one model does the work, other models test it, and the
//! findings go back to the first model for a revision. A DAG with a return
//! edge, driven by code.
//!
//! Reuses `panel` for lenses, verdict parsing and agreement counting. What
//! it adds: a reviewer that may run commands, a hash check in code that the
//! reviewer changed nothing, the return edge to the main model, memoized
//! verdicts, a findings ledger, pruning on code-visible failure, and a
//! record per run. See
//! `docs/superpowers/specs/2026-09-05-ensemble-dag-design.md` and
//! `docs/DECISIONS.md` (2026-09-05).
//!
//! Two rules are not negotiable. Code launches every run and review here;
//! no tool starts one, and `agent.rs` has a test saying so. And no roster
//! changes on a model's opinion: a reviewer is dropped for altering an
//! output, or for two unusable replies, and for nothing else.

pub mod hashes;
pub mod ledger;
pub mod record;

use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use crate::agent::{Agent, Outcome};
use crate::approval::ApprovalMode;
use crate::model::Model;
use crate::panel::{parse_verdict, reviewer_prompt, Lens, ReviewerVerdict, Target};
use crate::render::LineRenderer;
use crate::sandbox::CancelToken;

use hashes::Snapshot;
use ledger::{Finding, Ledger};
use record::{EnsembleRecord, Prune, ReviewerRecord, RoundRecord};

/// Rounds of review and revision, when the roster does not say.
pub const DEFAULT_ROUNDS: usize = 2;
/// Steps a reviewer may take. Lower than a working agent's on purpose: a
/// review that needs sixty steps is doing the task over.
pub const DEFAULT_REVIEW_STEPS: usize = 20;
/// Names the roster file. Roles come from here and never from the
/// instruction text.
pub const ROSTER_VAR: &str = "ZORP_ENSEMBLE";
pub const REVIEW_STEPS_VAR: &str = "ZORP_ENSEMBLE_REVIEW_STEPS";
/// Where reviewer transcripts and the record go. Defaults to
/// `<cwd>/scratch/ensemble/<run-id>/`.
pub const LOG_DIR_VAR: &str = "ZORP_ENSEMBLE_LOG_DIR";

/// Who plays which role. One lens per reviewer, in this order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Roster {
    pub main: String,
    pub reviewers: Vec<String>,
    pub rounds: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RosterFile {
    main: Role,
    #[serde(default)]
    reviewer: Vec<Role>,
    #[serde(default = "default_rounds")]
    rounds: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Role {
    model: String,
}

fn default_rounds() -> usize {
    DEFAULT_ROUNDS
}

impl Roster {
    pub fn parse(text: &str) -> Result<Roster, String> {
        let file: RosterFile = toml::from_str(text).map_err(|e| format!("roster: {e}"))?;
        if file.main.model.trim().is_empty() {
            return Err("roster: [main] model is empty".to_string());
        }
        let reviewers: Vec<String> = file.reviewer.into_iter().map(|r| r.model).collect();
        if reviewers.is_empty() {
            return Err("roster: at least one [[reviewer]] is required".to_string());
        }
        let lens_count = lenses().len();
        if reviewers.len() > lens_count {
            return Err(format!(
                "roster: {} reviewers but only {} lenses; one lens per reviewer",
                reviewers.len(),
                lens_count
            ));
        }
        if file.rounds == 0 {
            return Err("roster: rounds must be at least 1".to_string());
        }
        Ok(Roster {
            main: file.main.model,
            reviewers,
            rounds: file.rounds,
        })
    }

    pub fn load(path: &Path) -> Result<Roster, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("roster {}: {e}", path.display()))?;
        Roster::parse(&text)
    }
}

/// The three angles, code-defined, assigned one per reviewer in roster
/// order. A corroborated finding was then reached from two angles by two
/// models, which is the only agreement worth counting.
pub fn lenses() -> Vec<Lens> {
    vec![
        Lens::new(
            "contract",
            "Every output the instruction requires must exist at its path, in the \
named format, with the named columns, keys and units. Read the instruction, list \
what it requires, then check each requirement against the workspace. Report each \
output that is missing, misnamed, in the wrong format, or that lacks a column, key \
or unit the instruction asked for. Name the output path in `locus`.",
        ),
        Lens::new(
            "reproduction",
            "Pick the key number, table or figure the instruction asks for and recompute \
it from the data by a route the main run did not take: a different tool, a \
different library, or a hand calculation over a subset. Compare it with what was \
submitted. Report a disagreement with both values and how you got yours. Name the \
output path in `locus`. If you cannot reproduce it, say what stopped you and \
report nothing else.",
        ),
        Lens::new(
            "adversary",
            "Assume the submitted work is wrong and try to show it. Check assumptions \
the instruction did not license, off-by-one and index errors, unit and scale \
mistakes, a wrong column, a default a library applied silently, and edge cases in \
the data such as empty groups, missing values, duplicates and ties. Run the \
checks; do not reason about them from the code alone. Report what broke and how. \
Name the file in `locus`.",
        ),
    ]
}

/// The panel's read-only allow-list plus a shell. The verifier's tests are
/// hidden and the only way to test is to run checks in the workspace. A
/// shell can write, so the check that it did not is in code: see
/// `hashes` and the loop in `run`.
pub fn reviewer_tools() -> Vec<String> {
    let mut tools = crate::panel::reviewer_tools();
    tools.push("run_command".to_string());
    tools
}

/// How one run is shaped.
#[derive(Debug, Clone)]
pub struct EnsembleConfig {
    pub roster: Roster,
    pub review_steps: usize,
    /// Reviewer transcripts and `ensemble.json` go here.
    pub log_dir: PathBuf,
}

/// The agents and models a run is given. The main agent is built by the
/// caller exactly as a plain run builds it, tools and policy included.
pub struct Roles {
    pub main: Agent,
    pub reviewers: Vec<Box<dyn Model>>,
}

/// What a run leaves behind.
pub struct Finished {
    pub record: EnsembleRecord,
    /// The last main outcome: the first attempt's, or the last revision's.
    pub outcome: Outcome,
    pub record_path: Option<PathBuf>,
}

/// What a reviewer is, told to the reviewer. Panel's, plus the one thing a
/// reviewer with a shell is most tempted to do.
const REVIEWER_SYSTEM_PROMPT: &str = "\
You are a reviewer. You read and test work and report what is wrong with it. \
You may run commands to check it. You do not fix it, and you must not create, \
change or delete any file: the outputs are hashed before and after you, and a \
reviewer that altered one is dropped with its findings. Report what you find \
and stop.";

/// A reviewer's verdict, kept against the hashes of what it examined.
struct Memo {
    /// The watched files that were hashed to make this verdict, so a
    /// later round can tell whether any of them moved.
    key: Snapshot,
    /// What the reviewer really examined. Not read back out of `key`:
    /// a reviewer that examined nothing gets the whole snapshot as its
    /// key, and the record would then say it read every watched file.
    examined: Vec<String>,
    verdict: ReviewerVerdict,
}

struct Reviewed {
    result: Result<ReviewerVerdict, String>,
    examined: BTreeSet<String>,
    requests: usize,
}

/// One reviewer, start to finish. Builds its own agent from a clone of
/// the model, on the allow-list, with its transcript going to a file
/// when one is given.
#[allow(clippy::too_many_arguments)]
fn review(
    model: &dyn Model,
    lens: &Lens,
    instruction: &str,
    watched: &BTreeSet<String>,
    cwd: &Path,
    max_steps: usize,
    cancel: CancelToken,
    approval: ApprovalMode,
    transcript: Option<PathBuf>,
) -> Reviewed {
    let tools = reviewer_tools();
    let mut agent = Agent::new(
        model.clone_box(),
        REVIEWER_SYSTEM_PROMPT,
        max_steps,
        cwd.to_path_buf(),
        cancel,
        approval,
    )
    .register_builtins_filtered(Some(&tools));
    if let Some(path) = transcript {
        if let Ok(file) = File::create(&path) {
            agent = agent.with_renderer(Box::new(LineRenderer::new(file, false)));
        }
    }
    let listing = watched
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    let target = Target {
        label: "the submitted work".to_string(),
        body: format!(
            "Instruction the main model was given:\n{instruction}\n\n\
             Files the main model wrote or the instruction names:\n{listing}"
        ),
    };
    let answer = agent.run(&reviewer_prompt(lens, &target));
    let examined = hashes::examined(agent.transcript(), watched);
    let requests = agent
        .transcript()
        .iter()
        .filter(|m| m.role == "assistant")
        .count();
    let result = match answer {
        Outcome::Complete(text) => parse_verdict(&text)
            .map(|findings| ReviewerVerdict {
                lens: lens.name.clone(),
                findings,
                answer: text,
            })
            .map_err(|e| e.to_string()),
        other => Err(other.describe()),
    };
    Reviewed {
        result,
        examined,
        requests,
    }
}

fn assistant_count(agent: &Agent) -> usize {
    agent
        .transcript()
        .iter()
        .filter(|m| m.role == "assistant")
        .count()
}

/// The whole run: the main attempt, then up to `rounds` of review and
/// revision. Every launch here is code; a model never starts one.
pub fn run(
    config: &EnsembleConfig,
    mut roles: Roles,
    instruction: &str,
    cwd: &Path,
    cancel: CancelToken,
    approval: ApprovalMode,
) -> Finished {
    let lenses = lenses();
    let n = roles
        .reviewers
        .len()
        .min(config.roster.reviewers.len())
        .min(lenses.len());
    let mut record = EnsembleRecord::new(&config.roster, instruction, config.review_steps);
    let _ = std::fs::create_dir_all(&config.log_dir);

    let mut outcome = roles.main.run(instruction);
    record.main_outcomes.push(outcome.describe());
    if matches!(outcome, Outcome::Cancelled) {
        return finish(config, record, &roles.main, outcome, "cancelled");
    }

    // The one spelling of a watched path is the one the tool used, made
    // absolute only where `hashes::watched` expands a named directory.
    // `changed_paths` goes in verbatim for that reason: normalise it here
    // and it stops matching what `examined` reads out of a tool call, and
    // a reviewer that edited the file would not be caught.
    let mut watched = hashes::watched(cwd, instruction, &roles.main.changed_paths());
    let mut before = hashes::snapshot(cwd, &watched);
    let mut ledger = Ledger::default();
    let mut memo: Vec<Option<Memo>> = (0..n).map(|_| None).collect();
    let mut strikes = vec![0usize; n];
    let mut dropped = vec![false; n];
    let mut stopped = "bound";

    for round in 1..=config.roster.rounds {
        if cancel.load(Ordering::SeqCst) {
            stopped = "cancelled";
            break;
        }
        let mut verdicts: Vec<ReviewerVerdict> = Vec::new();
        let mut reviewers: Vec<ReviewerRecord> = Vec::new();
        let mut altered_this_round: Vec<String> = Vec::new();

        // ponytail: one reviewer at a time. Every role shares one free-tier
        // key and concurrent reviewers multiply its 429 rate. Panel's
        // Permits are the upgrade if a paid endpoint ever wants them.
        for i in 0..n {
            let lens = &lenses[i];
            let model_name = config.roster.reviewers[i].clone();
            let mut rec = ReviewerRecord {
                index: i,
                model: model_name.clone(),
                lens: lens.name.clone(),
                status: "skipped".to_string(),
                examined: Vec::new(),
                findings: Vec::new(),
                requests: 0,
            };
            if dropped[i] {
                rec.status = "dropped".to_string();
                reviewers.push(rec);
                continue;
            }
            if let Some(memo) = &memo[i] {
                if hashes::still_holds(&memo.key, &before) {
                    rec.status = "reused".to_string();
                    rec.examined = memo.examined.clone();
                    rec.findings = record::raw(&memo.verdict);
                    verdicts.push(memo.verdict.clone());
                    reviewers.push(rec);
                    continue;
                }
            }
            let transcript = config
                .log_dir
                .join(format!("reviewer-{i}-{}-round-{round}.txt", lens.name));
            let reviewed = review(
                roles.reviewers[i].as_ref(),
                lens,
                instruction,
                &watched,
                cwd,
                config.review_steps,
                cancel.clone(),
                approval.clone(),
                Some(transcript),
            );
            rec.requests = reviewed.requests;
            *record.requests.entry(format!("reviewer-{i}")).or_default() += reviewed.requests;

            let after = hashes::snapshot(cwd, &watched);
            let altered = hashes::changed(&before, &after);
            if !altered.is_empty() {
                dropped[i] = true;
                rec.status = "dropped".to_string();
                record.prunes.push(Prune::Tampered {
                    reviewer: i,
                    model: model_name,
                    round,
                    files: altered.clone(),
                });
                altered_this_round.extend(altered);
                // The next reviewer is measured against what it was handed,
                // not against what this one spoiled.
                before = after;
                reviewers.push(rec);
                continue;
            }

            rec.examined = reviewed.examined.iter().cloned().collect();
            match reviewed.result {
                Ok(verdict) => {
                    rec.status = "reviewed".to_string();
                    rec.findings = record::raw(&verdict);
                    let key = if reviewed.examined.is_empty() {
                        before.clone()
                    } else {
                        hashes::restrict(&before, &reviewed.examined)
                    };
                    memo[i] = Some(Memo {
                        key,
                        examined: rec.examined.clone(),
                        verdict: verdict.clone(),
                    });
                    verdicts.push(verdict);
                }
                Err(why) => {
                    // One Ctrl-C sets the token every reviewer shares, so a
                    // reviewer still running when it lands comes back
                    // cancelled, and so does every reviewer after it. That
                    // is the person's doing and not the reviewer's: no
                    // strike and no prune, and the round stops here. This
                    // module drops a reviewer for altering a watched file
                    // or for two unusable replies, and for nothing else.
                    if cancel.load(Ordering::SeqCst) {
                        reviewers.push(rec);
                        break;
                    }
                    strikes[i] += 1;
                    let for_run = strikes[i] >= 2;
                    if for_run {
                        dropped[i] = true;
                    }
                    rec.status = "unusable".to_string();
                    record.prunes.push(Prune::Unusable {
                        reviewer: i,
                        model: model_name,
                        round,
                        why,
                        for_run,
                    });
                }
            }
            reviewers.push(rec);
        }

        let corroborated = ledger::corroborated(round, &verdicts, &watched);
        let mut round_rec = RoundRecord {
            round,
            reviewers,
            corroborated: corroborated.clone(),
            outputs_changed: Vec::new(),
            addressed: 0,
        };
        // Before the corroboration count is read as a verdict on the work.
        // A round cut short has reviewers that never ran, so "nothing
        // corroborated" would describe a cancelled run as a clean
        // convergence in the one artifact this feature exists to produce.
        if cancel.load(Ordering::SeqCst) {
            record.rounds.push(round_rec);
            stopped = "cancelled";
            break;
        }
        if corroborated.is_empty() {
            record.rounds.push(round_rec);
            stopped = "nothing corroborated";
            break;
        }
        ledger.admit(corroborated);
        let message = {
            let open: Vec<&Finding> = ledger.open();
            ledger::return_message(&open, &altered_this_round, &ledger::marker(round))
        };

        outcome = roles.main.run(&message);
        record.main_outcomes.push(outcome.describe());
        watched = hashes::watched(cwd, instruction, &roles.main.changed_paths());
        let after = hashes::snapshot(cwd, &watched);
        let changed = hashes::changed(&before, &after);
        round_rec.addressed = ledger.settle(&changed);
        round_rec.outputs_changed = changed.clone();
        record.rounds.push(round_rec);
        if matches!(outcome, Outcome::Cancelled) {
            stopped = "cancelled";
            break;
        }
        if changed.is_empty() {
            stopped = "revision changed no output";
            break;
        }
        before = after;
    }

    record.open_at_end = ledger.open().into_iter().cloned().collect();
    finish(config, record, &roles.main, outcome, stopped)
}

fn finish(
    config: &EnsembleConfig,
    mut record: EnsembleRecord,
    main: &Agent,
    outcome: Outcome,
    stopped: &str,
) -> Finished {
    record
        .requests
        .insert("main".to_string(), assistant_count(main));
    record.stopped = stopped.to_string();
    let record_path = match record::write(&config.log_dir, &record) {
        Ok(path) => Some(path),
        Err(e) => {
            eprintln!("zorp-agent: ensemble record not written: {e}");
            None
        }
    };
    Finished {
        record,
        outcome,
        record_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssistantMessage, ContentPart, Message, Model, ToolCall};
    use crate::sandbox::cancel_token;
    use crate::{Agent, ApprovalMode, BoxErr};
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    const SPEC_ROSTER: &str = r#"
rounds = 2

[main]
model = "nvidia/nemotron-3-super-120b-a12b:free"

[[reviewer]]
model = "minimax/minimax-m3:free"
[[reviewer]]
model = "dots-studio/dots-3-note-preview:free"
[[reviewer]]
model = "minimax/minimax-m2.7:free"
"#;

    #[test]
    fn the_spec_roster_parses_in_order() {
        let r = Roster::parse(SPEC_ROSTER).unwrap();
        assert_eq!(r.main, "nvidia/nemotron-3-super-120b-a12b:free");
        assert_eq!(
            r.reviewers,
            vec![
                "minimax/minimax-m3:free",
                "dots-studio/dots-3-note-preview:free",
                "minimax/minimax-m2.7:free"
            ]
        );
        assert_eq!(r.rounds, 2);
    }

    #[test]
    fn rounds_default_to_two() {
        let r = Roster::parse("[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\n").unwrap();
        assert_eq!(r.rounds, DEFAULT_ROUNDS);
    }

    #[test]
    fn more_reviewers_than_lenses_is_refused_by_name() {
        let text = "[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\n[[reviewer]]\nmodel = \"c\"\n[[reviewer]]\nmodel = \"d\"\n[[reviewer]]\nmodel = \"e\"\n";
        let err = Roster::parse(text).unwrap_err();
        assert!(err.contains("4 reviewers"), "{err}");
        assert!(err.contains("3 lenses"), "{err}");
    }

    #[test]
    fn a_roster_needs_a_reviewer_and_a_round() {
        assert!(Roster::parse("[main]\nmodel = \"a\"\n").is_err());
        assert!(
            // `rounds` must come before any table header: TOML scopes a
            // bare key to the most recently opened table, so placed after
            // `[[reviewer]]` it would land on that reviewer, not the root.
            Roster::parse("rounds = 0\n[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\n")
                .is_err()
        );
        assert!(Roster::parse("[main]\nmodel = \"\"\n[[reviewer]]\nmodel = \"b\"\n").is_err());
    }

    #[test]
    fn the_lenses_are_contract_reproduction_adversary() {
        let names: Vec<String> = lenses().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["contract", "reproduction", "adversary"]);
    }

    #[test]
    fn reviewer_tools_are_the_panels_plus_a_shell() {
        let mut expected = crate::panel::reviewer_tools();
        expected.push("run_command".to_string());
        assert_eq!(reviewer_tools(), expected);
    }

    #[test]
    fn a_rounds_key_after_a_reviewer_table_is_refused() {
        let text = "[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\nrounds = 4\n";
        let err = Roster::parse(text).unwrap_err();
        assert!(err.contains("rounds"), "{err}");
    }

    /// A model that answers from a script and remembers every prompt it
    /// was handed. `clone_box` shares the script, so the count survives the
    /// clone the loop makes per review.
    #[derive(Clone)]
    struct Scripted {
        replies: Arc<Mutex<VecDeque<AssistantMessage>>>,
        prompts: Arc<Mutex<Vec<String>>>,
        /// Raised as this model answers, so a test can cancel a run from
        /// inside a reviewer the way a person's Ctrl-C does.
        cancel: Option<CancelToken>,
    }

    impl Scripted {
        fn new(replies: Vec<AssistantMessage>) -> Self {
            Scripted {
                replies: Arc::new(Mutex::new(replies.into())),
                prompts: Arc::new(Mutex::new(Vec::new())),
                cancel: None,
            }
        }
        fn calls(&self) -> usize {
            self.prompts.lock().unwrap().len()
        }
        fn last_prompt(&self) -> String {
            self.prompts
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl Model for Scripted {
        fn clone_box(&self) -> Box<dyn Model> {
            Box::new(self.clone())
        }
        fn complete(
            &self,
            messages: &[Message],
            _tools: &[Value],
        ) -> Result<AssistantMessage, BoxErr> {
            let last_user = messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| {
                    m.content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            self.prompts.lock().unwrap().push(last_user);
            if let Some(cancel) = &self.cancel {
                cancel.store(true, Ordering::SeqCst);
            }
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no more scripted replies".into())
        }
    }

    fn text(s: &str) -> AssistantMessage {
        AssistantMessage {
            content: s.to_string(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        }
    }

    fn call(name: &str, args: Value) -> AssistantMessage {
        AssistantMessage {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".to_string(),
                name: name.to_string(),
                arguments: args,
            }],
            finish_reason: "tool_calls".to_string(),
            reasoning_content: None,
        }
    }

    fn write(path: &str, content: &str) -> AssistantMessage {
        call("write_file", json!({"path": path, "content": content}))
    }

    fn verdict(severity: &str, locus: &str, claim: &str) -> AssistantMessage {
        text(&format!(
            "```json\n{{\"findings\":[{{\"severity\":\"{severity}\",\"claim\":\"{claim}\",\"locus\":\"{locus}\"}}]}}\n```"
        ))
    }

    fn nothing() -> AssistantMessage {
        text("```json\n{\"findings\":[]}\n```")
    }

    struct Setup {
        dir: tempfile::TempDir,
        main: Scripted,
        reviewers: Vec<Scripted>,
        /// One token for the whole run, which is what makes a cancel from
        /// inside any role reach every other one.
        cancel: CancelToken,
    }

    impl Setup {
        fn new(main: Vec<AssistantMessage>, reviewers: Vec<Vec<AssistantMessage>>) -> Self {
            Setup {
                dir: tempfile::tempdir().unwrap(),
                main: Scripted::new(main),
                reviewers: reviewers.into_iter().map(Scripted::new).collect(),
                cancel: cancel_token(),
            }
        }

        fn run(&self, rounds: usize) -> Finished {
            let cwd = self.dir.path().to_path_buf();
            let roster = Roster {
                main: "main".to_string(),
                reviewers: (0..self.reviewers.len()).map(|i| format!("r{i}")).collect(),
                rounds,
            };
            let config = EnsembleConfig {
                roster,
                review_steps: 6,
                log_dir: cwd.join("log"),
            };
            let main = Agent::new(
                Box::new(self.main.clone()),
                "you do tasks",
                8,
                cwd.clone(),
                self.cancel.clone(),
                ApprovalMode::AutoApprove,
            )
            .register_builtins();
            let roles = Roles {
                main,
                reviewers: self.reviewers.iter().map(|r| r.clone_box()).collect(),
            };
            run(
                &config,
                roles,
                "Write out.csv and notes.txt",
                &cwd,
                self.cancel.clone(),
                ApprovalMode::AutoApprove,
            )
        }
    }

    /// The main model writes two files and is done: three calls.
    fn main_writes() -> Vec<AssistantMessage> {
        vec![
            write("out.csv", "1,2\n"),
            write("notes.txt", "draft"),
            text("done"),
        ]
    }

    #[test]
    fn a_reviewer_that_edits_an_output_is_dropped_and_its_finding_never_reaches_main() {
        let setup = Setup::new(
            main_writes(),
            vec![
                vec![
                    call("run_command", json!({"command": "printf x >> out.csv"})),
                    verdict("blocking", "out.csv", "TAMPER-CLAIM"),
                ],
                vec![nothing()],
            ],
        );
        let finished = setup.run(2);
        let record = &finished.record;
        assert!(
            matches!(&record.prunes[0], record::Prune::Tampered { reviewer: 0, files, .. } if files == &vec!["out.csv".to_string()]),
            "{:?}",
            record.prunes
        );
        assert_eq!(record.rounds[0].reviewers[0].status, "dropped");
        assert!(record.rounds[0].corroborated.is_empty());
        assert_eq!(record.stopped, "nothing corroborated");
        assert_eq!(setup.main.calls(), 3, "main was never asked again");
        assert!(!setup.main.last_prompt().contains("TAMPER-CLAIM"));
    }

    #[test]
    fn unchanged_hashes_skip_the_second_reviewer_run() {
        let mut main = main_writes();
        main.extend([
            write("notes.txt", "revised"),
            text("fixed"),
            text("nothing more"),
        ]);
        let setup = Setup::new(
            main,
            vec![
                vec![
                    call("read_file", json!({"path": "out.csv"})),
                    verdict("concern", "notes.txt", "vague"),
                ],
                vec![verdict("concern", "notes.txt", "also vague")],
            ],
        );
        let finished = setup.run(2);
        let record = &finished.record;
        assert_eq!(record.rounds.len(), 2, "{}", record.stopped);
        assert_eq!(record.rounds[0].reviewers[0].status, "reviewed");
        assert_eq!(record.rounds[0].reviewers[0].examined, vec!["out.csv"]);
        assert_eq!(record.rounds[1].reviewers[0].status, "reused");
        assert_eq!(setup.reviewers[0].calls(), 2, "read and answer, once");
        assert!(
            setup.reviewers[1].calls() >= 2,
            "examined nothing, so it ran again"
        );
    }

    #[test]
    fn a_reviewer_unusable_twice_is_absent_from_the_third_round() {
        let mut main = main_writes();
        main.extend([
            write("notes.txt", "v2"),
            text("fixed"),
            write("notes.txt", "v3"),
            text("fixed again"),
            write("notes.txt", "v4"),
            text("and again"),
        ]);
        let setup = Setup::new(
            main,
            vec![
                vec![text("no json here"), text("still none"), text("never")],
                vec![
                    verdict("blocking", "notes.txt", "wrong"),
                    verdict("blocking", "notes.txt", "still wrong"),
                    verdict("blocking", "notes.txt", "wrong again"),
                ],
            ],
        );
        let finished = setup.run(3);
        let record = &finished.record;
        assert_eq!(record.rounds.len(), 3, "{}", record.stopped);
        let unusable: Vec<bool> = record
            .prunes
            .iter()
            .filter_map(|p| match p {
                record::Prune::Unusable {
                    reviewer: 0,
                    for_run,
                    ..
                } => Some(*for_run),
                _ => None,
            })
            .collect();
        assert_eq!(unusable, vec![false, true]);
        assert_eq!(record.rounds[2].reviewers[0].status, "dropped");
        assert_eq!(setup.reviewers[0].calls(), 2);
    }

    #[test]
    fn a_revision_that_changes_no_output_ends_the_loop_before_the_bound() {
        let mut main = main_writes();
        main.push(text("I disagree with every finding."));
        let setup = Setup::new(main, vec![vec![verdict("blocking", "notes.txt", "wrong")]]);
        let finished = setup.run(2);
        let record = &finished.record;
        assert_eq!(record.rounds.len(), 1);
        assert_eq!(record.stopped, "revision changed no output");
        assert_eq!(setup.main.calls(), 4);
        assert_eq!(record.open_at_end.len(), 1);
        assert_eq!(record.main_outcomes, vec!["complete", "complete"]);
    }

    #[test]
    fn one_lens_at_low_severity_never_reaches_main_but_two_lenses_do() {
        let setup = Setup::new(
            main_writes(),
            vec![
                vec![verdict("concern", "notes.txt", "LONE-CLAIM")],
                vec![nothing()],
            ],
        );
        let finished = setup.run(2);
        assert_eq!(finished.record.stopped, "nothing corroborated");
        assert_eq!(setup.main.calls(), 3);

        let mut main = main_writes();
        main.extend([write("notes.txt", "revised"), text("fixed")]);
        let setup = Setup::new(
            main,
            vec![
                vec![verdict("concern", "notes.txt", "FIRST-CLAIM")],
                vec![verdict("note", "Notes.txt", "SECOND-CLAIM")],
            ],
        );
        let finished = setup.run(1);
        assert_eq!(setup.main.calls(), 5);
        let prompt = setup.main.last_prompt();
        assert!(prompt.contains(ledger::FENCE_OPEN), "{prompt}");
        assert!(prompt.contains("[contract] FIRST-CLAIM"), "{prompt}");
        assert!(prompt.contains("[reproduction] SECOND-CLAIM"), "{prompt}");
        assert_eq!(finished.record.rounds[0].addressed, 1);
        assert!(finished.record.open_at_end.is_empty());
        assert_eq!(finished.record.stopped, "bound");
        assert!(finished.record_path.unwrap().ends_with("ensemble.json"));
        assert!(setup
            .dir
            .path()
            .join("log")
            .join("reviewer-0-contract-round-1.txt")
            .is_file());
        assert_eq!(finished.record.requests["main"], 5);
    }

    #[test]
    fn a_cancel_while_reviewers_are_pending_is_not_the_reviewers_fault() {
        let mut setup = Setup::new(
            main_writes(),
            vec![
                vec![verdict("concern", "notes.txt", "vague")],
                vec![verdict("concern", "notes.txt", "also vague")],
            ],
        );
        // Reviewer 0 raises the shared flag as it answers, which is where a
        // Ctrl-C lands mid-round: reviewer 0 finishes, and reviewer 1 comes
        // back cancelled without ever reaching its own script.
        setup.reviewers[0].cancel = Some(setup.cancel.clone());
        let finished = setup.run(2);
        let record = &finished.record;
        assert_eq!(record.stopped, "cancelled");
        assert!(record.prunes.is_empty(), "{:?}", record.prunes);
        assert!(!record.rounds[0]
            .reviewers
            .iter()
            .any(|r| r.status == "dropped"));
        assert_eq!(
            record.rounds[0].reviewers[0].status, "reviewed",
            "reviewer 0 finished, so its work is kept"
        );
        assert_eq!(
            setup.reviewers[1].calls(),
            0,
            "cancelled before its model was asked"
        );
    }

    #[test]
    fn a_reused_verdict_records_what_the_reviewer_examined_and_not_its_key() {
        let mut main = main_writes();
        // The revision creates a file rather than changing one, so every
        // previously watched hash still holds and a reviewer that examined
        // nothing, whose key is the whole snapshot, is reused.
        main.extend([write("extra.txt", "new"), text("fixed"), text("no more")]);
        let setup = Setup::new(
            main,
            vec![
                // Examines nothing, so its key is the whole snapshot.
                vec![verdict("blocking", "out.csv", "wrong")],
                // Examines one file, so its key is that one file.
                vec![
                    call("read_file", json!({"path": "notes.txt"})),
                    verdict("note", "notes.txt", "thin"),
                ],
            ],
        );
        let finished = setup.run(2);
        let rounds = &finished.record.rounds;
        assert_eq!(rounds.len(), 2, "{}", finished.record.stopped);
        assert_eq!(rounds[1].reviewers[0].status, "reused");
        assert!(
            rounds[1].reviewers[0].examined.is_empty(),
            "it examined nothing, and the whole snapshot is its key, not its reading: {:?}",
            rounds[1].reviewers[0].examined
        );
        assert_eq!(rounds[1].reviewers[1].status, "reused");
        assert_eq!(
            rounds[1].reviewers[1].examined,
            vec!["notes.txt"],
            "it read one file, and the record says one file"
        );
    }
}
