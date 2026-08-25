//! Adversarial verification of a finding.
//!
//! A finding is a claim by a reviewer, not a fact about the paper. Every
//! finding that survives the cheap filters is handed to several agents
//! whose instruction is to refute it, not to assess it. Two rules keep
//! that from becoming a rubber stamp: an agent that cannot decide counts
//! as a refutation, and a finding survives only on a strict majority of
//! the votes actually cast.

use serde::{Deserialize, Serialize};

/// One way a finding can be wrong.
///
/// Repeating one lens catches noise. Different lenses catch different
/// failure modes, and a finding usually has more than one way of being
/// wrong, so verifiers are assigned distinct lenses before any lens is
/// reused.
pub struct Lens {
    pub key: &'static str,
    pub brief: &'static str,
}

pub const LENSES: &[Lens] = &[
    Lens {
        key: "text",
        brief: "Re-read the quoted span and the paragraph around it. Does the paper \
                actually say what the finding says it says? Quote the surrounding text.",
    },
    Lens {
        key: "elsewhere",
        brief: "Search the rest of the paper. Does it already address this somewhere \
                the finding did not look: an appendix, a footnote, a later section?",
    },
    Lens {
        key: "standard",
        brief: "Judge the finding against what work of this kind is actually held to, \
                not against an ideal. Would a reviewer at a real venue raise this, or \
                is it a preference stated as a defect?",
    },
    Lens {
        key: "consequence",
        brief: "Grant the finding for the sake of argument. Does any claim the paper \
                makes change? A true observation that changes no conclusion is not a \
                finding worth a reader's time.",
    },
    Lens {
        key: "source",
        brief: "Go to the primary material the finding depends on, whether that is a \
                cited work, the evidence record, or a figure, and check it directly \
                rather than trusting the finding's account of it.",
    },
];

/// How one verifier voted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vote {
    /// Tried to refute it and could not.
    Upheld,
    /// Refuted it.
    Refuted,
    /// Could not tell. Counts against the finding: see [`tally`].
    Uncertain,
}

impl Vote {
    /// Parse a verifier's answer. Anything unrecognised, including a
    /// reply that did not parse at all, is [`Vote::Uncertain`]. A
    /// verifier that malfunctions must not be able to wave a finding
    /// through.
    pub fn parse(text: &str) -> Vote {
        let lowered = text.to_ascii_lowercase();
        // "refuted" is checked first because an agent writing "not
        // upheld, refuted" contains both words, and the safe reading of
        // an ambiguous answer is the one that does not admit a finding.
        if lowered.contains("refuted") {
            Vote::Refuted
        } else if lowered.contains("upheld") {
            Vote::Upheld
        } else {
            Vote::Uncertain
        }
    }
}

/// What verification concluded about a finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// A strict majority of verifiers tried to refute it and failed.
    Upheld,
    /// It did not carry a majority.
    Refuted,
    /// Nobody got to check it. Reported, never treated as either
    /// outcome: an unverified finding is not a confirmed one and its
    /// absence from the report would hide a gap in coverage.
    Unverified(String),
}

impl Verdict {
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Upheld => "upheld",
            Verdict::Refuted => "refuted",
            Verdict::Unverified(_) => "unverified",
        }
    }
}

/// A finding's verification record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Verification {
    pub verdict: Verdict,
    /// Which lens cast which vote, in the order they were assigned.
    pub votes: Vec<(String, Vote)>,
}

/// Decide a finding's fate from the votes cast.
///
/// [`Vote::Uncertain`] is counted, and counted against the finding: it is
/// a vote that was cast and did not support it. Discarding uncertain
/// votes instead would let a single confident verifier carry a finding
/// that everyone else found doubtful.
pub fn tally(votes: &[Vote]) -> Verdict {
    if votes.is_empty() {
        return Verdict::Unverified("no verifier reached this finding".to_string());
    }
    let upheld = votes.iter().filter(|v| **v == Vote::Upheld).count();
    if upheld * 2 > votes.len() {
        Verdict::Upheld
    } else {
        Verdict::Refuted
    }
}

/// Assign `count` verifiers distinct lenses, reusing one only once every
/// lens has been used.
pub fn lenses_for(count: usize) -> Vec<&'static Lens> {
    (0..count).map(|i| &LENSES[i % LENSES.len()]).collect()
}

/// The instruction a verifier gets. It never asks whether the finding is
/// correct: the question is always how it is wrong, because an agent
/// asked to evaluate a claim agrees with it far more often than one asked
/// to break it.
pub fn refuter_prompt(
    lens: &Lens,
    dimension: &str,
    claim: &str,
    anchor: &str,
    evidence: &str,
) -> String {
    format!(
        "Another reviewer has raised the finding below against this paper. Your job is \
         to REFUTE it. Do not assess whether it is reasonable, and do not improve it: \
         find the reason it is wrong.\n\n\
         Take this angle specifically: {}\n\n\
         Dimension: {dimension}\n\
         Finding: {claim}\n\
         Quoted from the paper: \"{anchor}\"\n\
         Stated reason: {evidence}\n\n\
         Answer with a single fenced JSON block: \
         {{\"vote\": \"refuted\" | \"upheld\" | \"uncertain\", \"reason\": \"<one or two sentences>\"}}\n\
         Vote \"refuted\" if you found the reason it is wrong. Vote \"upheld\" only if \
         you genuinely tried and could not. Vote \"uncertain\" if you could not \
         establish either, which counts against the finding.",
        lens.brief
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strict_majority_upholds() {
        assert_eq!(
            tally(&[Vote::Upheld, Vote::Upheld, Vote::Refuted]),
            Verdict::Upheld
        );
    }

    #[test]
    fn a_bare_minority_does_not_uphold() {
        assert_eq!(
            tally(&[Vote::Upheld, Vote::Refuted, Vote::Refuted]),
            Verdict::Refuted
        );
    }

    /// The rule that stops a two-verifier split reading as support.
    /// Half is not a majority.
    #[test]
    fn an_even_split_does_not_uphold() {
        assert_eq!(tally(&[Vote::Upheld, Vote::Refuted]), Verdict::Refuted);
    }

    /// The default-to-refuted rule. An undecided verifier is not a
    /// neutral one: it did not support the finding.
    #[test]
    fn uncertainty_counts_against_the_finding() {
        assert_eq!(
            tally(&[Vote::Upheld, Vote::Uncertain, Vote::Uncertain]),
            Verdict::Refuted
        );
        assert_eq!(
            tally(&[Vote::Upheld, Vote::Upheld, Vote::Uncertain]),
            Verdict::Upheld
        );
    }

    #[test]
    fn all_uncertain_refutes() {
        assert_eq!(
            tally(&[Vote::Uncertain, Vote::Uncertain, Vote::Uncertain]),
            Verdict::Refuted
        );
    }

    /// No votes is not a refutation. It means coverage was cut short and
    /// the report has to say so rather than presenting the finding as
    /// checked or dropping it silently.
    #[test]
    fn no_votes_is_unverified_not_refuted() {
        assert!(matches!(tally(&[]), Verdict::Unverified(_)));
    }

    #[test]
    fn a_single_verifier_can_uphold() {
        assert_eq!(tally(&[Vote::Upheld]), Verdict::Upheld);
        assert_eq!(tally(&[Vote::Uncertain]), Verdict::Refuted);
    }

    #[test]
    fn verifiers_get_distinct_lenses_before_any_is_reused() {
        let three = lenses_for(3);
        let keys: Vec<&str> = three.iter().map(|l| l.key).collect();
        assert_eq!(keys.len(), 3);
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "three verifiers, three different lenses");
    }

    #[test]
    fn lenses_only_repeat_once_every_one_is_used() {
        let many = lenses_for(LENSES.len() + 1);
        assert_eq!(many[LENSES.len()].key, many[0].key);
    }

    #[test]
    fn an_unparseable_vote_is_uncertain() {
        assert_eq!(Vote::parse("I could not access the paper"), Vote::Uncertain);
        assert_eq!(Vote::parse(""), Vote::Uncertain);
    }

    #[test]
    fn an_ambiguous_vote_reads_as_refuted() {
        assert_eq!(Vote::parse("not upheld, refuted"), Vote::Refuted);
    }

    #[test]
    fn a_clear_vote_parses() {
        assert_eq!(Vote::parse("{\"vote\": \"upheld\"}"), Vote::Upheld);
        assert_eq!(Vote::parse("{\"vote\": \"refuted\"}"), Vote::Refuted);
    }

    /// The prompt must never ask an agent to judge the finding. Asking
    /// "is this right?" gets agreement; asking "how is this wrong?" gets
    /// work.
    #[test]
    fn the_refuter_prompt_asks_for_refutation_not_assessment() {
        let p = refuter_prompt(
            &LENSES[0],
            "statistical-validity",
            "no error bars",
            "we observe 14%",
            "one run",
        );
        assert!(p.contains("REFUTE"));
        assert!(p.contains("counts against the finding"));
    }
}
