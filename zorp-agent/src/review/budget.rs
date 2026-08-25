//! Central enforcement of what a review is allowed to spend.
//!
//! Every agent a review starts is charged here, whether the orchestrator
//! started it or another agent did. Asking each agent to behave would not
//! work: an agent that can start agents has no view of what its siblings
//! have already spent, and a prompt is not a bound.

use std::sync::Mutex;

/// The highest recursion depth this capability will accept, whatever the
/// caller asks for.
///
/// Depth bounds the length of one chain of enquiry, not the size of the
/// tree. A chain of ten is reachable at fan-out one and costs ten agents.
/// A tree of ten at fan-out three would be 88,572 agents, which no budget
/// here will ever fund, so the number is a ceiling on how deep a single
/// narrow line of questioning may go and never a safety bound on spend.
/// The budget is the bound on spend.
pub const MAX_DEPTH_CEILING: usize = 10;

/// Why a dispatch was refused. Both variants are reported, never swallowed:
/// a review that quietly stopped covering things reads as a clean paper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The requested depth is past the configured recursion limit.
    DepthExceeded {
        depth: usize,
        max_depth: usize,
        what: String,
    },
    /// The total agent budget for this review is spent.
    AgentsExhausted {
        spent: usize,
        max_agents: usize,
        what: String,
    },
}

impl Refusal {
    pub fn what(&self) -> &str {
        match self {
            Refusal::DepthExceeded { what, .. } => what,
            Refusal::AgentsExhausted { what, .. } => what,
        }
    }

    pub fn reason(&self) -> String {
        match self {
            Refusal::DepthExceeded {
                depth, max_depth, ..
            } => format!("recursion depth {depth} is past the limit of {max_depth}"),
            Refusal::AgentsExhausted {
                spent, max_agents, ..
            } => format!("the agent budget is spent ({spent} of {max_agents})"),
        }
    }
}

#[derive(Debug, Default)]
struct BudgetState {
    spent: usize,
    refusals: Vec<Refusal>,
}

/// A shared, thread-safe account for one review run.
#[derive(Debug)]
pub struct Budget {
    max_depth: usize,
    max_agents: usize,
    state: Mutex<BudgetState>,
}

impl Budget {
    /// `max_depth` is clamped to [`MAX_DEPTH_CEILING`] so a caller cannot
    /// widen the recursion limit past what this capability will accept.
    pub fn new(max_depth: usize, max_agents: usize) -> Self {
        Budget {
            max_depth: max_depth.min(MAX_DEPTH_CEILING),
            max_agents,
            state: Mutex::new(BudgetState::default()),
        }
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    pub fn max_agents(&self) -> usize {
        self.max_agents
    }

    /// Charge one agent at `depth`, or refuse and record why.
    ///
    /// The depth check and the count check are one critical section on
    /// purpose. Two agents recursing at once would otherwise both read
    /// `spent` below the cap and both be allowed through it.
    pub fn try_charge(&self, depth: usize, what: &str) -> Result<(), Refusal> {
        let mut state = self.state.lock().unwrap();
        if depth > self.max_depth {
            let refusal = Refusal::DepthExceeded {
                depth,
                max_depth: self.max_depth,
                what: what.to_string(),
            };
            state.refusals.push(refusal.clone());
            return Err(refusal);
        }
        if state.spent >= self.max_agents {
            let refusal = Refusal::AgentsExhausted {
                spent: state.spent,
                max_agents: self.max_agents,
                what: what.to_string(),
            };
            state.refusals.push(refusal.clone());
            return Err(refusal);
        }
        state.spent += 1;
        Ok(())
    }

    pub fn spent(&self) -> usize {
        self.state.lock().unwrap().spent
    }

    /// True once the agent budget is gone. A depth refusal does not
    /// exhaust anything: it only means one branch went too deep.
    pub fn exhausted(&self) -> bool {
        self.state.lock().unwrap().spent >= self.max_agents
    }

    pub fn refusals(&self) -> Vec<Refusal> {
        self.state.lock().unwrap().refusals.clone()
    }

    /// Whether a child at `depth` could ever be charged. Used to decide
    /// whether to hand an agent a tool that starts more agents at all,
    /// rather than handing it one that always refuses.
    pub fn depth_is_allowed(&self, depth: usize) -> bool {
        depth <= self.max_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn charges_until_the_agent_cap_then_refuses() {
        let budget = Budget::new(3, 2);
        assert!(budget.try_charge(1, "doer").is_ok());
        assert!(budget.try_charge(1, "checker").is_ok());
        let err = budget.try_charge(1, "one too many").unwrap_err();
        assert!(matches!(err, Refusal::AgentsExhausted { .. }));
        assert_eq!(budget.spent(), 2);
    }

    #[test]
    fn refuses_past_the_depth_limit_without_spending_the_budget() {
        let budget = Budget::new(2, 100);
        assert!(budget.try_charge(2, "refuter").is_ok());
        let err = budget.try_charge(3, "refuter helper").unwrap_err();
        assert!(matches!(err, Refusal::DepthExceeded { .. }));
        // A branch that went too deep must not consume the budget the
        // rest of the review still needs.
        assert_eq!(budget.spent(), 1);
    }

    #[test]
    fn every_refusal_is_recorded_with_what_was_not_covered() {
        let budget = Budget::new(1, 1);
        budget.try_charge(1, "citation-integrity doer").unwrap();
        let _ = budget.try_charge(2, "refuter for finding 1");
        let _ = budget.try_charge(1, "reproducibility doer");
        let refusals = budget.refusals();
        assert_eq!(refusals.len(), 2);
        assert_eq!(refusals[0].what(), "refuter for finding 1");
        assert_eq!(refusals[1].what(), "reproducibility doer");
    }

    #[test]
    fn depth_is_clamped_to_the_ceiling() {
        let budget = Budget::new(500, 10);
        assert_eq!(budget.max_depth(), MAX_DEPTH_CEILING);
        assert!(budget
            .try_charge(MAX_DEPTH_CEILING + 1, "too deep")
            .is_err());
    }

    #[test]
    fn exhausted_reports_the_agent_cap_not_a_depth_refusal() {
        let budget = Budget::new(1, 3);
        let _ = budget.try_charge(9, "too deep");
        assert!(!budget.exhausted());
        for i in 0..3 {
            budget.try_charge(1, &format!("agent {i}")).unwrap();
        }
        assert!(budget.exhausted());
    }

    /// The cap has to hold when agents recurse concurrently, which they
    /// do: the pool runs up to eight at once and each may start more.
    #[test]
    fn concurrent_charges_never_exceed_the_cap() {
        let budget = Arc::new(Budget::new(5, 50));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let b = budget.clone();
            handles.push(std::thread::spawn(move || {
                let mut ok = 0;
                for _ in 0..20 {
                    if b.try_charge(1, "racer").is_ok() {
                        ok += 1;
                    }
                }
                ok
            }));
        }
        let total: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(total, 50, "exactly the cap should have been granted");
        assert_eq!(budget.spent(), 50);
    }

    #[test]
    fn depth_is_allowed_answers_before_a_charge_is_made() {
        let budget = Budget::new(3, 100);
        assert!(budget.depth_is_allowed(3));
        assert!(!budget.depth_is_allowed(4));
        assert_eq!(budget.spent(), 0);
    }
}
