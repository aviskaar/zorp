//! Running review agents, and the one place they are authorised.
//!
//! The orchestrator never starts an agent directly. It hands jobs to a
//! [`BudgetedDispatcher`], which charges [`Budget`] first and only passes
//! on what the budget allowed. An agent that wants to start another agent
//! gets [`SpawnReviewAgent`], which charges the same account. There is one
//! account and no way around it, which is the point: an agent cannot know
//! what its siblings have spent, so asking each one to be careful does not
//! bound anything.

use super::budget::{Budget, Refusal};
use crate::agent::{Agent, AgentConfig, Outcome};
use crate::sandbox::CancelToken;
use crate::tools::subagent::{ProgressRecorder, RunStatus, SubagentPool, MAX_CONCURRENT_SUBAGENTS};
use crate::tools::{Context, Tool, ToolError, ToolOutput, ToolResult};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One agent's worth of work.
#[derive(Clone, Debug)]
pub struct Job {
    pub role: String,
    pub prompt: String,
}

#[derive(Clone, Debug)]
pub enum JobResult {
    Done(String),
    /// The budget would not pay for it. Carried through to the report as
    /// work that was not done, never dropped.
    Refused(Refusal),
    /// The agent ran and did not produce an answer.
    Failed(String),
}

impl JobResult {
    pub fn text(&self) -> Option<&str> {
        match self {
            JobResult::Done(t) => Some(t),
            _ => None,
        }
    }
}

/// Runs jobs. Implementations do not charge the budget: that is
/// [`BudgetedDispatcher`]'s job, so that every path through this module
/// hits the same account.
pub trait Dispatcher: Send + Sync {
    fn dispatch(&self, depth: usize, jobs: Vec<Job>) -> Vec<JobResult>;
}

/// The authorising wrapper. Nothing in a review runs without going
/// through one of these or through [`SpawnReviewAgent`], and both charge
/// the same [`Budget`].
pub struct BudgetedDispatcher {
    inner: Arc<dyn Dispatcher>,
    budget: Arc<Budget>,
}

impl BudgetedDispatcher {
    pub fn new(inner: Arc<dyn Dispatcher>, budget: Arc<Budget>) -> Self {
        BudgetedDispatcher { inner, budget }
    }

    pub fn budget(&self) -> &Arc<Budget> {
        &self.budget
    }

    /// Charge for each job, run the ones the budget allowed, and return
    /// results in the order the jobs were given. Refusals keep their
    /// slot so a caller pairing results with jobs cannot silently
    /// misalign them.
    pub fn dispatch(&self, depth: usize, jobs: Vec<Job>) -> Vec<JobResult> {
        let mut allowed = Vec::new();
        // `None` marks a slot the budget paid for, filled in below from
        // the runner's results in order.
        let mut slots: Vec<Option<JobResult>> = Vec::with_capacity(jobs.len());
        for job in jobs {
            match self.budget.try_charge(depth, &job.role) {
                Ok(()) => {
                    slots.push(None);
                    allowed.push(job);
                }
                Err(refusal) => slots.push(Some(JobResult::Refused(refusal))),
            }
        }
        if allowed.is_empty() {
            return slots.into_iter().flatten().collect();
        }
        let mut ran = self.inner.dispatch(depth, allowed).into_iter();
        slots
            .into_iter()
            .map(|slot| {
                slot.unwrap_or_else(|| {
                    ran.next().unwrap_or_else(|| {
                        JobResult::Failed(
                            "the runner returned fewer results than it was given jobs".to_string(),
                        )
                    })
                })
            })
            .collect()
    }
}

/// The real dispatcher: runs each job as a subagent in the shared
/// [`SubagentPool`], so review agents show up in `monitor_subagents` and
/// answer to `cancel_subagent` like any other subagent.
pub struct PoolDispatcher {
    config: AgentConfig,
    pool: SubagentPool,
    budget: Arc<Budget>,
    preamble: String,
}

impl PoolDispatcher {
    pub fn new(
        config: AgentConfig,
        pool: SubagentPool,
        budget: Arc<Budget>,
        preamble: &str,
    ) -> Self {
        PoolDispatcher {
            config,
            pool,
            budget,
            preamble: preamble.to_string(),
        }
    }

    pub fn pool(&self) -> &SubagentPool {
        &self.pool
    }
}

fn system_prompt(preamble: &str, base: &str, role: &str) -> String {
    format!("{preamble}\n\n{base}\n\nYou are acting as: {role}")
}

/// Build and run one review agent to completion on the current thread.
fn run_agent(
    config: &AgentConfig,
    budget: &Arc<Budget>,
    preamble: &str,
    depth: usize,
    job: &Job,
    cancel: CancelToken,
    progress: Option<Arc<Mutex<Vec<String>>>>,
) -> Outcome {
    let mut agent = Agent::new(
        config.model.clone(),
        system_prompt(preamble, &config.base_system_prompt, &job.role),
        config.max_steps,
        config.repo_root.clone(),
        cancel,
        config.approval.clone(),
    )
    .register_builtins()
    .with_renderer(Box::new(crate::render::NullRenderer));
    if let Some(buf) = progress {
        agent = agent.with_recorder(Box::new(ProgressRecorder::new(buf)));
    }
    // A child that could never be charged is not given the tool at all.
    // Handing an agent a tool that always refuses wastes its steps
    // arguing with it.
    if budget.depth_is_allowed(depth + 1) {
        agent = agent.register(Box::new(SpawnReviewAgent {
            config: config.clone(),
            budget: budget.clone(),
            preamble: preamble.to_string(),
            depth: depth + 1,
        }));
    }
    agent.run(&job.prompt)
}

fn outcome_to_result(outcome: Outcome) -> JobResult {
    match outcome {
        Outcome::Complete(text) => JobResult::Done(text),
        other => JobResult::Failed(other.describe()),
    }
}

impl Dispatcher for PoolDispatcher {
    fn dispatch(&self, depth: usize, jobs: Vec<Job>) -> Vec<JobResult> {
        let mut results: Vec<Option<JobResult>> = (0..jobs.len()).map(|_| None).collect();
        // The pool caps concurrency, so waves rather than one big fan-out.
        for wave in (0..jobs.len()).step_by(MAX_CONCURRENT_SUBAGENTS) {
            let end = (wave + MAX_CONCURRENT_SUBAGENTS).min(jobs.len());
            let mut running = Vec::new();
            for (index, job) in jobs.iter().enumerate().take(end).skip(wave) {
                let child_cancel: CancelToken = Arc::new(AtomicBool::new(false));
                let (id, progress, _status) =
                    self.pool
                        .allocate(job.role.clone(), job.prompt.clone(), child_cancel.clone());
                let config = self.config.clone();
                let budget = self.budget.clone();
                let preamble = self.preamble.clone();
                let pool = self.pool.clone();
                let job = job.clone();
                let parent_cancel = self.config.cancel.clone();
                let watch = child_cancel.clone();
                std::thread::spawn(move || {
                    while !parent_cancel.load(Ordering::SeqCst) && !watch.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    watch.store(true, Ordering::SeqCst);
                });
                std::thread::spawn(move || {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_agent(
                            &config,
                            &budget,
                            &preamble,
                            depth,
                            &job,
                            child_cancel,
                            Some(progress),
                        )
                    }));
                    let status = match outcome {
                        Ok(Outcome::Complete(text)) => RunStatus::Complete(text),
                        Ok(Outcome::Cancelled) => RunStatus::Cancelled,
                        Ok(other) => RunStatus::Failed(other.describe()),
                        Err(_) => RunStatus::Failed("panicked".to_string()),
                    };
                    pool.set_status(id, status);
                });
                running.push((index, id));
            }
            for (index, id) in running {
                results[index] = Some(self.wait(id));
            }
        }
        results
            .into_iter()
            .map(|r| r.unwrap_or_else(|| JobResult::Failed("never started".to_string())))
            .collect()
    }
}

impl PoolDispatcher {
    /// Wait for one pooled agent. The agent's own step limit is what
    /// ends it; this only watches, and gives up if the whole run is
    /// cancelled so a Ctrl-C is not swallowed by the wait.
    fn wait(&self, id: u32) -> JobResult {
        loop {
            if self.config.cancel.load(Ordering::SeqCst) {
                return JobResult::Failed("cancelled".to_string());
            }
            match self.pool.get(id).map(|s| s.status) {
                Some(RunStatus::Running) | None => std::thread::sleep(Duration::from_millis(50)),
                Some(RunStatus::Complete(text)) => return JobResult::Done(text),
                Some(RunStatus::Cancelled) => return JobResult::Failed("cancelled".to_string()),
                Some(RunStatus::Failed(msg)) => return JobResult::Failed(msg),
            }
        }
    }
}

/// The tool that lets a review agent start another review agent. It
/// charges the same budget the orchestrator charges, at its own depth,
/// which is fixed when the tool is built and never read from arguments.
#[derive(Clone)]
pub struct SpawnReviewAgent {
    config: AgentConfig,
    budget: Arc<Budget>,
    preamble: String,
    depth: usize,
}

impl Tool for SpawnReviewAgent {
    fn name(&self) -> &str {
        "spawn_review_agent"
    }

    fn description(&self) -> &str {
        "Delegates one bounded sub-question to another agent and returns its answer. \
Use it when answering properly needs something you cannot check yourself, for example \
reading a cited work or re-deriving a number. It draws on a fixed budget shared by the \
whole review, so it will refuse once that budget is spent."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The single, bounded question for the agent to answer."
                },
                "role": {
                    "type": "string",
                    "description": "What this agent is being asked to be, for example 'citation checker'."
                }
            },
            "required": ["prompt"]
        })
    }

    fn run(&self, args: &Value, _cx: &mut Context) -> ToolResult {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("missing \"prompt\" parameter"))?
            .to_string();
        let role = args
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("review helper")
            .to_string();

        // A refusal is an answer, not an error: the calling agent should
        // finish with what it has rather than retry and burn its steps.
        if let Err(refusal) = self.budget.try_charge(self.depth, &role) {
            return Ok(ToolOutput::new(
                format!(
                    "refused: {}. Answer with what you already have and say what you could not check.",
                    refusal.reason()
                ),
                "budget refused",
            ));
        }

        let job = Job { role, prompt };
        let outcome = run_agent(
            &self.config,
            &self.budget,
            &self.preamble,
            self.depth,
            &job,
            // The review's own cancel token, so stopping the review
            // stops everything it started, however deep.
            self.config.cancel.clone(),
            None,
        );
        match outcome_to_result(outcome) {
            JobResult::Done(text) => Ok(ToolOutput::new(text, "helper answered")),
            JobResult::Failed(msg) => Ok(ToolOutput::new(
                format!("the helper did not finish: {msg}"),
                "helper failed",
            )),
            JobResult::Refused(r) => Ok(ToolOutput::new(r.reason(), "budget refused")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Recording {
        seen: Mutex<Vec<(usize, String)>>,
    }

    impl Dispatcher for Recording {
        fn dispatch(&self, depth: usize, jobs: Vec<Job>) -> Vec<JobResult> {
            let mut seen = self.seen.lock().unwrap();
            jobs.iter()
                .map(|j| {
                    seen.push((depth, j.role.clone()));
                    JobResult::Done(format!("answer from {}", j.role))
                })
                .collect()
        }
    }

    fn job(role: &str) -> Job {
        Job {
            role: role.to_string(),
            prompt: "do the thing".to_string(),
        }
    }

    #[test]
    fn jobs_within_budget_all_run() {
        let inner = Arc::new(Recording {
            seen: Mutex::new(Vec::new()),
        });
        let budget = Arc::new(Budget::new(3, 10));
        let d = BudgetedDispatcher::new(inner.clone(), budget.clone());
        let results = d.dispatch(1, vec![job("doer"), job("checker")]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].text(), Some("answer from doer"));
        assert_eq!(results[1].text(), Some("answer from checker"));
        assert_eq!(budget.spent(), 2);
    }

    /// The cap has to hold at the dispatch boundary, not inside an
    /// agent's prompt. Jobs past it never reach the runner at all.
    #[test]
    fn jobs_past_the_budget_never_reach_the_runner() {
        let inner = Arc::new(Recording {
            seen: Mutex::new(Vec::new()),
        });
        let budget = Arc::new(Budget::new(3, 2));
        let d = BudgetedDispatcher::new(inner.clone(), budget.clone());
        let results = d.dispatch(1, vec![job("a"), job("b"), job("c"), job("d")]);
        assert_eq!(
            inner.seen.lock().unwrap().len(),
            2,
            "only two were paid for"
        );
        assert!(matches!(results[2], JobResult::Refused(_)));
        assert!(matches!(results[3], JobResult::Refused(_)));
    }

    /// Results are paired with jobs by position everywhere upstream, so
    /// a refusal must hold its slot rather than shortening the vector.
    #[test]
    fn refusals_keep_their_position_in_the_results() {
        let inner = Arc::new(Recording {
            seen: Mutex::new(Vec::new()),
        });
        let budget = Arc::new(Budget::new(3, 1));
        let d = BudgetedDispatcher::new(inner, budget);
        let results = d.dispatch(1, vec![job("first"), job("second"), job("third")]);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].text(), Some("answer from first"));
        assert!(matches!(results[1], JobResult::Refused(_)));
        assert!(matches!(results[2], JobResult::Refused(_)));
    }

    #[test]
    fn a_job_past_the_depth_limit_is_refused_without_running() {
        let inner = Arc::new(Recording {
            seen: Mutex::new(Vec::new()),
        });
        let budget = Arc::new(Budget::new(2, 100));
        let d = BudgetedDispatcher::new(inner.clone(), budget.clone());
        let results = d.dispatch(3, vec![job("too deep")]);
        assert!(inner.seen.lock().unwrap().is_empty());
        assert!(matches!(results[0], JobResult::Refused(_)));
        assert_eq!(budget.spent(), 0);
    }

    #[test]
    fn an_empty_job_list_does_not_touch_the_runner() {
        let inner = Arc::new(Recording {
            seen: Mutex::new(Vec::new()),
        });
        let budget = Arc::new(Budget::new(3, 10));
        let d = BudgetedDispatcher::new(inner.clone(), budget);
        assert!(d.dispatch(1, vec![]).is_empty());
        assert!(inner.seen.lock().unwrap().is_empty());
    }

    #[derive(Clone)]
    struct FixedReply {
        text: String,
        calls: Arc<AtomicUsize>,
    }

    impl crate::model::Model for FixedReply {
        fn clone_box(&self) -> Box<dyn crate::model::Model> {
            Box::new(self.clone())
        }
        fn complete(
            &self,
            _messages: &[crate::model::Message],
            _tools: &[Value],
        ) -> Result<crate::model::AssistantMessage, crate::BoxErr> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(crate::model::AssistantMessage {
                content: self.text.clone(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }
    }

    fn test_config(text: &str) -> (AgentConfig, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let config = AgentConfig {
            model: Box::new(FixedReply {
                text: text.to_string(),
                calls: calls.clone(),
            }),
            base_system_prompt: "you are a reviewer".to_string(),
            max_steps: 4,
            repo_root: std::env::temp_dir(),
            cancel: crate::cancel_token(),
            approval: crate::ApprovalMode::AutoApprove,
        };
        (config, calls)
    }

    #[test]
    fn the_pool_dispatcher_runs_every_job_and_registers_them_in_the_pool() {
        let (config, _calls) = test_config("done");
        let pool = SubagentPool::new();
        let budget = Arc::new(Budget::new(2, 10));
        let d = PoolDispatcher::new(config, pool.clone(), budget.clone(), "review preamble");
        let results = d.dispatch(1, vec![job("doer"), job("checker")]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].text(), Some("done"));
        assert_eq!(results[1].text(), Some("done"));
        assert_eq!(pool.all().len(), 2, "review agents are visible in the pool");
    }

    /// The tool an agent uses to recurse charges the same account the
    /// orchestrator does, so a recursing agent cannot mint agents the
    /// orchestrator has already spent.
    #[test]
    fn the_spawn_tool_charges_the_same_budget() {
        let (config, _calls) = test_config("helper answer");
        let budget = Arc::new(Budget::new(5, 1));
        let tool = SpawnReviewAgent {
            config,
            budget: budget.clone(),
            preamble: "review".to_string(),
            depth: 2,
        };
        let mut cx = Context::new(std::env::temp_dir(), crate::cancel_token());
        let first = tool
            .run(&json!({"prompt": "check this", "role": "helper"}), &mut cx)
            .unwrap();
        assert!(first.content.contains("helper answer"));
        assert_eq!(budget.spent(), 1);

        let second = tool
            .run(&json!({"prompt": "check that", "role": "helper"}), &mut cx)
            .unwrap();
        assert!(second.content.contains("budget is spent"));
    }

    #[test]
    fn the_spawn_tool_refuses_rather_than_erroring_when_the_budget_is_gone() {
        let (config, _calls) = test_config("never reached");
        let budget = Arc::new(Budget::new(5, 0));
        let tool = SpawnReviewAgent {
            config,
            budget,
            preamble: "review".to_string(),
            depth: 1,
        };
        let mut cx = Context::new(std::env::temp_dir(), crate::cancel_token());
        // An error would make the calling agent retry; a refusal tells
        // it to finish with what it has.
        let out = tool.run(&json!({"prompt": "x"}), &mut cx).unwrap();
        assert!(out.content.starts_with("refused:"));
    }

    #[test]
    fn the_spawn_tools_depth_comes_from_construction_not_from_arguments() {
        let (config, _calls) = test_config("answer");
        let budget = Arc::new(Budget::new(1, 10));
        let tool = SpawnReviewAgent {
            config,
            budget: budget.clone(),
            preamble: "review".to_string(),
            depth: 2,
        };
        let mut cx = Context::new(std::env::temp_dir(), crate::cancel_token());
        // Depth 2 is past the limit of 1, and no argument can change that.
        let out = tool
            .run(&json!({"prompt": "x", "role": "r", "depth": 0}), &mut cx)
            .unwrap();
        assert!(out.content.contains("recursion depth 2"));
        assert_eq!(budget.spent(), 0);
    }
}
