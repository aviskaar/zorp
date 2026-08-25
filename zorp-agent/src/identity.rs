//! What zorp says it is, in one place.
//!
//! This constant exists because it did not. The CLI opened with "a helpful
//! coding assistant" (inherited from quecto, never revisited) and the web UI
//! opened with "a careful assistant", which said so little that a local model
//! filled the gap itself and introduced zorp to a user as their "coding
//! buddy". Two surfaces, two identities, neither of them the product's.
//!
//! zorp is a research agent. The README, zorp.dev, and the four research
//! capabilities all say so. The prompt the model actually reads has to say so
//! too, and it has to say it once so the next surface cannot drift again.
//!
//! Overriding this is still supported and still cheap: `ZORP_SYSTEM` in the
//! environment, or `system_prompt` in a flavor.

/// The default system prompt for every zorp surface that does not set its own.
pub const DEFAULT_SYSTEM_PROMPT: &str = "\
You are zorp, a research agent. You turn hard questions into defensible \
answers. A confident answer is not the same as a defensible one, and the \
difference is the evidence behind it.

Ground what you claim in something you actually checked: a file you read, a \
command you ran, a source you retrieved. Say where it came from, so the \
person reading can check it themselves. When you cannot ground a claim, say \
so plainly instead of writing around the gap.

Report what pointed the other way, not only what supported your answer, and \
say what would change it.

Use tools when they help. The question does not have to be about code. A \
technical decision, a market question, a due diligence pass and an academic \
hypothesis are the same shape of problem here.";

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression itself. A model told a user it was their "coding buddy"
    /// because nothing in its prompt said otherwise.
    #[test]
    fn zorp_introduces_itself_as_a_research_agent() {
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("research agent"),
            "the default prompt never says what zorp is: {DEFAULT_SYSTEM_PROMPT}"
        );
    }

    #[test]
    fn zorp_is_not_described_as_a_coding_assistant() {
        let lowered = DEFAULT_SYSTEM_PROMPT.to_lowercase();
        for wrong in ["coding assistant", "coding agent", "coding buddy"] {
            assert!(
                !lowered.contains(wrong),
                "the default prompt still positions zorp as {wrong}"
            );
        }
    }

    /// Positioning without the discipline behind it is just a noun. The
    /// prompt has to ask for the grounding, or renaming the role changes
    /// nothing about what the model does.
    #[test]
    fn the_prompt_asks_for_evidence_rather_than_only_claiming_to_be_evidence_based() {
        let lowered = DEFAULT_SYSTEM_PROMPT.to_lowercase();
        assert!(
            lowered.contains("ground"),
            "the prompt names the role but never asks the model to ground anything"
        );
        assert!(
            lowered.contains("cannot ground"),
            "the prompt never tells the model what to do when it has no evidence"
        );
    }

    /// The finding block only works if the model knows the shape, so the
    /// syntax lives in the prompt rather than in documentation nobody feeds
    /// it.
    #[test]
    fn the_prompt_teaches_the_finding_block() {
        for part in ["```finding", "claim:", "because:", "source:"] {
            assert!(
                DEFAULT_SYSTEM_PROMPT.contains(part),
                "the prompt never shows the model `{part}`, so the block cannot be written"
            );
        }
    }

    /// The failure mode the whole marker is designed against. A model asked
    /// whether something was novel says yes, so the prompt has to say plainly
    /// that most answers get no block at all.
    #[test]
    fn the_prompt_rations_findings_rather_than_inviting_them() {
        let lowered = DEFAULT_SYSTEM_PROMPT.to_lowercase();
        assert!(
            lowered.contains("at most one"),
            "the prompt sets no budget, so a long run will emit a block per paragraph"
        );
        assert!(
            lowered.contains("most answers"),
            "the prompt never says the normal case is no finding at all"
        );
    }

    /// A citation the run never touched is the cheapest way to manufacture a
    /// marker, and it is also the one thing the browser checks. The prompt
    /// says so, so a dropped marker is not a mystery.
    #[test]
    fn the_prompt_says_sources_have_to_be_things_the_run_used() {
        let lowered = DEFAULT_SYSTEM_PROMPT.to_lowercase();
        assert!(
            lowered.contains("actually used"),
            "nothing tells the model its sources are checked against the run"
        );
        assert!(
            lowered.contains("dropped"),
            "the prompt never says what happens when a source does not check out"
        );
    }

    /// zorp is not a research agent in the narrow academic sense, and the
    /// prompt should not let a model assume it is. See the domains section on
    /// zorp.dev: the loop does not know what domain it is in.
    #[test]
    fn the_prompt_does_not_narrow_zorp_to_code_or_to_academia() {
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("does not have to be about code"),
            "nothing tells the model the question can be non-technical"
        );
    }
}
