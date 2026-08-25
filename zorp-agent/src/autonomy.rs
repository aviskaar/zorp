//! How autonomous a run is: what it does on its own, and what it stops for.
//!
//! This sits between the hard policy in `policy.rs` and the human in
//! `approval.rs`, and it only ever answers one question: given that the
//! policy did not refuse this action, is it worth interrupting a human for?
//!
//! It cannot turn a refusal into an action. `Agent` consults it only on
//! `Decision::Ask`; a `Decision::Deny` never reaches this module. That is the
//! whole reason the denylist stays supreme no matter how autonomous a run is.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// How much a run does without asking.
///
/// Ordered from most cautious to least, and the ordering is load bearing:
/// `restrict` and `delegate` both rely on `min` never producing a level
/// higher than the one it started from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Ask about everything that is not a read.
    Confirm,
    /// Act on changes confined to the workspace. Ask about anything that
    /// reaches past it.
    Guarded,
    /// Ask once per class of action, then act on that class for the rest of
    /// this agent's run.
    Scoped,
    /// Ask about nothing. Report everything.
    Autonomous,
}

impl Level {
    pub const ALL: [Level; 4] = [
        Level::Confirm,
        Level::Guarded,
        Level::Scoped,
        Level::Autonomous,
    ];

    /// The level a run gets when nobody chose one.
    pub const DEFAULT: Level = Level::Confirm;

    pub fn parse(name: &str) -> Option<Level> {
        match name.trim().to_ascii_lowercase().as_str() {
            "confirm" | "ask" => Some(Level::Confirm),
            "guarded" => Some(Level::Guarded),
            "scoped" => Some(Level::Scoped),
            "autonomous" | "auto" => Some(Level::Autonomous),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Confirm => "confirm",
            Level::Guarded => "guarded",
            Level::Scoped => "scoped",
            Level::Autonomous => "autonomous",
        }
    }

    /// One line saying what this level does, for the CLI banner and the
    /// browser badge. A user who cannot tell whether the thing is asking or
    /// acting is not in any loop.
    pub fn headline(self) -> &'static str {
        match self {
            Level::Confirm => "asks before every edit, command, subagent, search, and MCP call",
            Level::Guarded => {
                "edits the workspace on its own; asks before commands, subagents, searches, \
                 and MCP calls"
            }
            Level::Scoped => {
                "asks once per kind of action, then keeps going with that kind on its own"
            }
            Level::Autonomous => "asks nothing and reports everything; the denylist still refuses",
        }
    }

    /// The names accepted on the command line, for error messages.
    pub fn names() -> String {
        Level::ALL
            .iter()
            .map(|level| level.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The unit a `Scoped` grant covers, and the thing a level reasons about.
///
/// Deliberately coarser than the tool list: a human being asked "may it run
/// shell commands" is answering a question they can hold in their head, and
/// "may it run `cargo test`, and separately may it run `ls`" is not.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ActionClass {
    Read,
    WorkspaceEdit,
    Shell,
    Delegate,
    Search,
    /// One named tool on an external MCP server.
    ///
    /// Named, not lumped together, because MCP tool names are discovered at
    /// runtime: one server's tools say nothing about another's, so a grant
    /// for one must never cover the other.
    External(String),
    /// A tool the built-in policy does not recognize. `Policy::decide`
    /// refuses these outright so this is unreachable today, and it is
    /// treated as the most sensitive class so that stays true if it ever
    /// stops being unreachable.
    Unrecognized,
}

impl ActionClass {
    pub fn of(tool: &str) -> ActionClass {
        match tool {
            "read_file" | "list_files" | "search_text" | "git_diff" | "git_status"
            | "search_notes" | "list_background_processes" | "monitor_subagents" => {
                ActionClass::Read
            }
            "write_file" | "apply_patch" | "take_note" => ActionClass::WorkspaceEdit,
            "run_command" | "start_background_process" | "kill_background_process" => {
                ActionClass::Shell
            }
            "spawn_subagent" | "cancel_subagent" => ActionClass::Delegate,
            "web_search" => ActionClass::Search,
            name if name.starts_with("mcp__") => ActionClass::External(name.to_string()),
            _ => ActionClass::Unrecognized,
        }
    }

    /// What the CLI banner and the browser call this.
    pub fn label(&self) -> &str {
        match self {
            ActionClass::Read => "reads",
            ActionClass::WorkspaceEdit => "workspace edits",
            ActionClass::Shell => "shell commands",
            ActionClass::Delegate => "subagents",
            ActionClass::Search => "web searches",
            ActionClass::External(name) => name,
            ActionClass::Unrecognized => "unrecognized tools",
        }
    }
}

/// What the level says to do about one action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gate {
    /// The level covers this. Run it, and report it.
    Proceed,
    /// Put it to the human.
    Ask,
}

/// Generations of subagents allowed below the run that started.
///
/// Two means the run spawns children and those children spawn grandchildren,
/// and there it stops. See `DelegationBudget` for why a depth cap alone is
/// not enough.
pub const DEFAULT_MAX_DELEGATION_DEPTH: u32 = 2;

/// Subagents allowed in one run tree, over its whole life.
pub const DEFAULT_MAX_SUBAGENTS: u32 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DelegationError {
    DepthReached { depth: u32, max_depth: u32 },
    BudgetSpent { max_total: u32 },
}

impl std::fmt::Display for DelegationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DelegationError::DepthReached { depth, max_depth } => write!(
                f,
                "cannot delegate: this agent is already {depth} levels deep and the limit is \
                 {max_depth}; do this work here instead of spawning another subagent"
            ),
            DelegationError::BudgetSpent { max_total } => write!(
                f,
                "cannot delegate: this run has already used its whole budget of {max_total} \
                 subagents; do this work here instead of spawning another"
            ),
        }
    }
}

impl std::error::Error for DelegationError {}

/// The central ledger for subagent spawning.
///
/// Central because the cost of recursive delegation is exponential in depth,
/// and an exponential cannot be held down by asking each agent to behave. One
/// of these is created for a run and shared, by `Arc`, with every descendant.
/// A subagent cannot make itself a fresh one: the only way to get a child
/// `Authority` is `Authority::delegate`, which clones the `Arc`.
#[derive(Debug)]
pub struct DelegationBudget {
    max_depth: u32,
    max_total: u32,
    spawned: AtomicU32,
}

impl DelegationBudget {
    pub fn new(max_depth: u32, max_total: u32) -> Arc<DelegationBudget> {
        Arc::new(DelegationBudget {
            max_depth,
            max_total,
            spawned: AtomicU32::new(0),
        })
    }

    pub fn standard() -> Arc<DelegationBudget> {
        DelegationBudget::new(DEFAULT_MAX_DELEGATION_DEPTH, DEFAULT_MAX_SUBAGENTS)
    }

    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    pub fn max_total(&self) -> u32 {
        self.max_total
    }

    pub fn spawned(&self) -> u32 {
        self.spawned.load(Ordering::SeqCst)
    }

    pub fn remaining(&self) -> u32 {
        self.max_total.saturating_sub(self.spawned())
    }

    /// Claim one subagent on behalf of a parent sitting at `depth`.
    ///
    /// The counter moves under `fetch_update` rather than a load followed by
    /// a store, because eight threads can reach the last unit at once and
    /// only one of them may have it.
    fn claim(&self, depth: u32) -> Result<(), DelegationError> {
        if depth >= self.max_depth {
            return Err(DelegationError::DepthReached {
                depth,
                max_depth: self.max_depth,
            });
        }
        self.spawned
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.max_total).then_some(n + 1)
            })
            .map(|_| ())
            .map_err(|_| DelegationError::BudgetSpent {
                max_total: self.max_total,
            })
    }
}

/// Everything about how much a single agent may do on its own.
///
/// Cloning shares the budget and the grants, so an `Agent` and the tools
/// registered on it see one ledger. Deriving a child, which is what
/// `delegate` does and the only thing that does, shares the budget and
/// starts fresh grants.
#[derive(Clone)]
pub struct Authority {
    level: Level,
    depth: u32,
    budget: Arc<DelegationBudget>,
    /// Classes a human already said yes to, under `Level::Scoped`.
    ///
    /// Never handed to a child. A human's yes answered one agent's proposal
    /// about its own next move; passing it down would authorize work nobody
    /// was ever shown.
    grants: Arc<Mutex<HashSet<ActionClass>>>,
}

impl Default for Authority {
    fn default() -> Authority {
        Authority::root(Level::DEFAULT, DelegationBudget::standard())
    }
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority")
            .field("level", &self.level)
            .field("depth", &self.depth)
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl Authority {
    /// The authority of a run that nobody delegated: depth zero, a fresh
    /// grant set, and whatever level the human chose outside the run.
    pub fn root(level: Level, budget: Arc<DelegationBudget>) -> Authority {
        Authority {
            level,
            depth: 0,
            budget,
            grants: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn budget(&self) -> &Arc<DelegationBudget> {
        &self.budget
    }

    /// Lower the level to `at_most`, leaving it alone when it is already
    /// lower.
    ///
    /// A meet, never a join. This is the only thing in the crate that
    /// produces an `Authority` with a different level from an existing one,
    /// and it cannot produce a higher one. Raising the level is something a
    /// human does from outside the run, by starting it differently, and
    /// there is deliberately no API for a run to do it to itself.
    pub fn restrict(&self, at_most: Level) -> Authority {
        Authority {
            level: self.level.min(at_most),
            depth: self.depth,
            budget: Arc::clone(&self.budget),
            grants: Arc::clone(&self.grants),
        }
    }

    pub fn gate(&self, class: &ActionClass) -> Gate {
        match class {
            // A read changed nothing before this module existed and changes
            // nothing now.
            ActionClass::Read => Gate::Proceed,
            // Unreachable today, and if that changes it should surface as a
            // question rather than as an action.
            ActionClass::Unrecognized => Gate::Ask,
            _ => match self.level {
                Level::Autonomous => Gate::Proceed,
                Level::Guarded if matches!(class, ActionClass::WorkspaceEdit) => Gate::Proceed,
                Level::Scoped if self.granted(class) => Gate::Proceed,
                _ => Gate::Ask,
            },
        }
    }

    fn granted(&self, class: &ActionClass) -> bool {
        self.grants.lock().unwrap().contains(class)
    }

    /// Remember that a human allowed `class`.
    ///
    /// A no-op at every level except `Scoped`, which is what stops a
    /// `Confirm` run from talking its way into acting freely by answering
    /// its own questions. `Scoped` is the level where the human agreed that
    /// one yes covers a kind of action; nowhere else did they agree to that.
    pub fn grant(&self, class: ActionClass) {
        if self.level != Level::Scoped {
            return;
        }
        self.grants.lock().unwrap().insert(class);
    }

    /// The classes this agent has been granted, sorted for display.
    pub fn granted_classes(&self) -> Vec<ActionClass> {
        let mut classes: Vec<ActionClass> = self.grants.lock().unwrap().iter().cloned().collect();
        classes.sort_by(|a, b| a.label().cmp(b.label()));
        classes
    }

    /// Derive the authority of a subagent, claiming it against the budget.
    ///
    /// The only constructor of a delegated `Authority`, and it takes no
    /// parameter that could widen one. The child's level is the parent's,
    /// its depth is one greater, and its budget is the same object. That is
    /// what makes "a subagent never holds more authority than its parent" a
    /// property of the type rather than a rule each caller has to remember.
    pub fn delegate(&self) -> Result<Authority, DelegationError> {
        self.budget.claim(self.depth)?;
        Ok(Authority {
            level: self.level,
            depth: self.depth + 1,
            budget: Arc::clone(&self.budget),
            grants: Arc::new(Mutex::new(HashSet::new())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered_from_most_cautious_to_least() {
        assert!(Level::Confirm < Level::Guarded);
        assert!(Level::Guarded < Level::Scoped);
        assert!(Level::Scoped < Level::Autonomous);
    }

    #[test]
    fn level_names_round_trip_and_unknown_names_are_rejected() {
        for level in Level::ALL {
            assert_eq!(Level::parse(level.as_str()), Some(level));
            assert_eq!(Level::parse(&level.as_str().to_uppercase()), Some(level));
        }
        assert_eq!(Level::parse("yolo"), None);
        assert_eq!(Level::parse(""), None);
    }

    #[test]
    fn the_default_level_asks_about_everything() {
        assert_eq!(Level::DEFAULT, Level::Confirm);
        assert_eq!(Authority::default().level(), Level::Confirm);
    }

    #[test]
    fn tools_map_onto_the_classes_the_levels_talk_about() {
        assert_eq!(ActionClass::of("read_file"), ActionClass::Read);
        assert_eq!(ActionClass::of("monitor_subagents"), ActionClass::Read);
        assert_eq!(ActionClass::of("write_file"), ActionClass::WorkspaceEdit);
        assert_eq!(ActionClass::of("apply_patch"), ActionClass::WorkspaceEdit);
        assert_eq!(ActionClass::of("run_command"), ActionClass::Shell);
        assert_eq!(ActionClass::of("spawn_subagent"), ActionClass::Delegate);
        assert_eq!(ActionClass::of("cancel_subagent"), ActionClass::Delegate);
        assert_eq!(ActionClass::of("web_search"), ActionClass::Search);
        assert_eq!(
            ActionClass::of("mcp__server__tool"),
            ActionClass::External("mcp__server__tool".to_string())
        );
        assert_eq!(ActionClass::of("nonsense"), ActionClass::Unrecognized);
    }

    fn authority(level: Level) -> Authority {
        Authority::root(level, DelegationBudget::standard())
    }

    #[test]
    fn confirm_asks_about_everything_that_is_not_a_read() {
        let a = authority(Level::Confirm);
        assert_eq!(a.gate(&ActionClass::Read), Gate::Proceed);
        for class in [
            ActionClass::WorkspaceEdit,
            ActionClass::Shell,
            ActionClass::Delegate,
            ActionClass::Search,
            ActionClass::External("mcp__x__y".into()),
        ] {
            assert_eq!(a.gate(&class), Gate::Ask, "{class:?}");
        }
    }

    #[test]
    fn guarded_edits_the_workspace_but_asks_about_everything_else() {
        let a = authority(Level::Guarded);
        assert_eq!(a.gate(&ActionClass::Read), Gate::Proceed);
        assert_eq!(a.gate(&ActionClass::WorkspaceEdit), Gate::Proceed);
        for class in [
            ActionClass::Shell,
            ActionClass::Delegate,
            ActionClass::Search,
            ActionClass::External("mcp__x__y".into()),
        ] {
            assert_eq!(a.gate(&class), Gate::Ask, "{class:?}");
        }
    }

    #[test]
    fn scoped_asks_once_per_class_and_then_stops_asking() {
        let a = authority(Level::Scoped);
        assert_eq!(a.gate(&ActionClass::Shell), Gate::Ask);
        a.grant(ActionClass::Shell);
        assert_eq!(a.gate(&ActionClass::Shell), Gate::Proceed);
        // A yes about one class says nothing about another.
        assert_eq!(a.gate(&ActionClass::Delegate), Gate::Ask);
        assert_eq!(a.gate(&ActionClass::WorkspaceEdit), Gate::Ask);
    }

    /// MCP tool names are discovered at runtime, so one server's tool must
    /// not be able to ride in on another's approval.
    #[test]
    fn a_scoped_grant_for_one_mcp_tool_does_not_cover_another() {
        let a = authority(Level::Scoped);
        a.grant(ActionClass::External("mcp__notes__search".into()));
        assert_eq!(
            a.gate(&ActionClass::External("mcp__notes__search".into())),
            Gate::Proceed
        );
        assert_eq!(
            a.gate(&ActionClass::External("mcp__shell__exec".into())),
            Gate::Ask
        );
    }

    #[test]
    fn autonomous_asks_about_nothing() {
        let a = authority(Level::Autonomous);
        for class in [
            ActionClass::Read,
            ActionClass::WorkspaceEdit,
            ActionClass::Shell,
            ActionClass::Delegate,
            ActionClass::Search,
            ActionClass::External("mcp__x__y".into()),
        ] {
            assert_eq!(a.gate(&class), Gate::Proceed, "{class:?}");
        }
    }

    /// The catch-all class exists so that a tool nobody classified surfaces
    /// as a question. Even the most autonomous level asks about it.
    #[test]
    fn an_unrecognized_tool_is_asked_about_at_every_level() {
        for level in Level::ALL {
            assert_eq!(
                authority(level).gate(&ActionClass::Unrecognized),
                Gate::Ask,
                "{level}"
            );
        }
    }

    /* ---------------------------------------------------------------- */
    /* security property: a run cannot raise its own level               */
    /* ---------------------------------------------------------------- */

    /// `restrict` is a meet. Whatever a run asks for, it does not come back
    /// with more than it had. Mutation check: make `restrict` use `max`.
    #[test]
    fn restricting_an_authority_never_raises_its_level() {
        for held in Level::ALL {
            for asked in Level::ALL {
                let after = authority(held).restrict(asked);
                assert!(
                    after.level() <= held,
                    "restrict({asked}) on {held} produced {}",
                    after.level()
                );
                assert!(
                    after.level() <= asked,
                    "restrict({asked}) on {held} produced {}",
                    after.level()
                );
            }
        }
    }

    /// The only mutator a running agent reaches is `grant`, and granting is
    /// what happens after a human says yes. It must not turn a cautious run
    /// into a free one. Mutation check: drop the level guard in `grant`.
    #[test]
    fn granting_never_raises_the_level_or_loosens_a_lower_one() {
        for level in [Level::Confirm, Level::Guarded] {
            let a = authority(level);
            for class in [
                ActionClass::WorkspaceEdit,
                ActionClass::Shell,
                ActionClass::Delegate,
                ActionClass::Search,
                ActionClass::External("mcp__x__y".into()),
            ] {
                a.grant(class);
            }
            assert_eq!(a.level(), level, "grant changed the level");
            assert!(
                a.granted_classes().is_empty(),
                "{level} banked grants it never agreed to honor"
            );
            // What the run actually gets: the same answers as before.
            assert_eq!(a.gate(&ActionClass::Shell), Gate::Ask);
            assert_eq!(a.gate(&ActionClass::Delegate), Gate::Ask);
        }
    }

    /* ---------------------------------------------------------------- */
    /* security property: a child never holds more than its parent       */
    /* ---------------------------------------------------------------- */

    /// Mutation check: give the child `Level::Autonomous` in `delegate`.
    #[test]
    fn a_delegated_authority_never_outranks_the_one_that_made_it() {
        for level in Level::ALL {
            let parent = authority(level);
            let child = parent.delegate().expect("depth 0 can delegate");
            assert!(
                child.level() <= parent.level(),
                "a {level} parent produced a {} child",
                child.level()
            );
            assert_eq!(child.depth(), parent.depth() + 1);
            let grandchild = child.delegate().expect("depth 1 can delegate");
            assert!(grandchild.level() <= parent.level());
            assert_eq!(grandchild.depth(), 2);
        }
    }

    /// A grant is a human's answer to one agent about one of its own moves.
    /// Handing it down would authorize work nobody was shown.
    #[test]
    fn a_child_does_not_inherit_its_parents_grants() {
        let parent = authority(Level::Scoped);
        parent.grant(ActionClass::Shell);
        assert_eq!(parent.gate(&ActionClass::Shell), Gate::Proceed);
        let child = parent.delegate().unwrap();
        assert_eq!(child.gate(&ActionClass::Shell), Gate::Ask);
    }

    /// A child granting itself something must not leak back up.
    #[test]
    fn a_childs_grant_does_not_reach_its_parent() {
        let parent = authority(Level::Scoped);
        let child = parent.delegate().unwrap();
        child.grant(ActionClass::Shell);
        assert_eq!(child.gate(&ActionClass::Shell), Gate::Proceed);
        assert_eq!(parent.gate(&ActionClass::Shell), Gate::Ask);
    }

    /* ---------------------------------------------------------------- */
    /* the budget                                                        */
    /* ---------------------------------------------------------------- */

    #[test]
    fn delegation_stops_at_the_depth_limit() {
        let budget = DelegationBudget::new(2, 100);
        let root = Authority::root(Level::Autonomous, budget);
        let child = root.delegate().unwrap();
        let grandchild = child.delegate().unwrap();
        assert_eq!(
            grandchild.delegate().unwrap_err(),
            DelegationError::DepthReached {
                depth: 2,
                max_depth: 2
            }
        );
    }

    #[test]
    fn delegation_stops_at_the_total_budget_even_within_the_depth_limit() {
        let budget = DelegationBudget::new(8, 3);
        let root = Authority::root(Level::Autonomous, Arc::clone(&budget));
        for _ in 0..3 {
            root.delegate().unwrap();
        }
        assert_eq!(
            root.delegate().unwrap_err(),
            DelegationError::BudgetSpent { max_total: 3 }
        );
        assert_eq!(budget.spawned(), 3);
        assert_eq!(budget.remaining(), 0);
    }

    /// The budget is one ledger for the whole tree, not one per agent. A run
    /// that spawns a child which spawns its own children spends the same
    /// pot, which is the only way an exponential stays bounded.
    #[test]
    fn every_descendant_spends_the_same_budget() {
        let budget = DelegationBudget::new(4, 4);
        let root = Authority::root(Level::Autonomous, Arc::clone(&budget));
        let child = root.delegate().unwrap();
        let grandchild = child.delegate().unwrap();
        grandchild.delegate().unwrap();
        root.delegate().unwrap();
        assert_eq!(budget.spawned(), 4);
        assert_eq!(
            root.delegate().unwrap_err(),
            DelegationError::BudgetSpent { max_total: 4 }
        );
    }

    /// A depth cap alone bounds nothing: subagents finish and free their
    /// concurrency slot, so a shallow run can spawn forever. Both caps are
    /// needed and this is the one that says why.
    #[test]
    fn a_shallow_run_cannot_spawn_without_end() {
        let budget = DelegationBudget::new(2, 5);
        let root = Authority::root(Level::Autonomous, Arc::clone(&budget));
        for _ in 0..5 {
            root.delegate().unwrap();
        }
        assert!(root.delegate().is_err());
    }

    /// Threads racing for the last unit: exactly one may have it.
    #[test]
    fn the_budget_is_not_oversubscribed_under_concurrency() {
        let budget = DelegationBudget::new(8, 10);
        let root = Authority::root(Level::Autonomous, Arc::clone(&budget));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let a = root.clone();
            handles.push(std::thread::spawn(move || a.delegate().is_ok()));
        }
        let granted = handles
            .into_iter()
            .filter(|h| h.is_finished() || true)
            .map(|h| h.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(granted, 10);
        assert_eq!(budget.spawned(), 10);
    }

    /// The standard budget is what an unconfigured run gets, and the numbers
    /// are the ones the README and the PR quote.
    #[test]
    fn the_standard_budget_is_two_deep_and_sixteen_wide() {
        let budget = DelegationBudget::standard();
        assert_eq!(budget.max_depth(), DEFAULT_MAX_DELEGATION_DEPTH);
        assert_eq!(budget.max_total(), DEFAULT_MAX_SUBAGENTS);
        assert_eq!(budget.max_depth(), 2);
        assert_eq!(budget.max_total(), 16);
    }

    #[test]
    fn every_level_says_out_loud_what_it_does() {
        for level in Level::ALL {
            assert!(!level.headline().is_empty(), "{level} has no headline");
            assert!(!level.as_str().is_empty());
        }
        assert!(Level::names().contains("autonomous"));
    }
}
