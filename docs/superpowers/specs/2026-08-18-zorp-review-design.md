# zorp review: a standing adversarial review of a paper

2026-08-18. Design note for the fifth capability, `zorp-agent review`,
built alongside validate, investigate, co-write, and deliver.

## What it is

Given a paper, run a hierarchical multi-agent review until findings stop
appearing, then report. The report is written to `review.md` and
`review.json` under the track and the run is recorded as a checkpoint, so
a review is on the record the same way every other capability's output
is.

It reviews a track's `draft.md` by default, and any file with `--paper`.
The second form is what lets it be developed against `docs/paper/zorp-paper.md`.

## Agent topology

Depth 0 is the orchestrator. It is code, not an agent, so it costs
nothing and it is the only thing that decides when to stop.

```
depth 0  orchestrator (code)
           |
           |  per round, per dimension
depth 1    +-- doer      "find problems on this dimension"
           +-- checker   "which of the doer's findings does the paper not
           |              support, and what did the doer miss"
           |
           |  per finding that survives the cheap filters
depth 2    +-- refuter x3, each with a different lens
           |
depth 3+       each refuter may start helpers of its own,
               through spawn_review_agent, to the depth limit
```

A round is: dispatch the doers, dispatch the checkers with the doers'
numbered output, filter, deduplicate, verify what is left. The next round
starts with a list of everything already raised, so the agents are told
not to repeat themselves as well as being prevented from it.

Agents run through the existing `SubagentPool` in
`zorp-agent/src/tools/subagent.rs`. Review agents are therefore visible
to `monitor_subagents` and answer to `cancel_subagent` like any other
subagent. There is no second pool and no second progress mechanism. The
only change to the inherited code was making `ProgressRecorder` public so
a pool slot's progress buffer can be written to from outside the module.

## Three filters, in cost order

A finding is a claim by a reviewer, not a fact about the paper. Three
things happen to it, cheapest first, so the expensive one runs on as
little as possible.

1. **The anchor check.** Every finding must quote at least five words
   verbatim from the paper. The quote is checked against the paper text
   in code, after collapsing whitespace. A finding whose quote is not in
   the paper is dropped for nothing.

   This is the filter that stops padding. Advice a model could give
   without opening the file has nothing to quote. It is also a
   hallucination check: a reviewer quoting text the paper does not
   contain has invented it or is reviewing something else.

2. **The checker.** The doer's findings are handed to a second agent on
   the same dimension, numbered, which returns the indices it judges
   unsupported. Cheap because one agent screens many findings.

3. **Adversarial verification.** Each survivor goes to three agents
   instructed to refute it. See below.

## Why the loop terminates

"Until the reviewer is satisfied" is not a stopping condition. A reviewer
asked "anything else?" always answers, so satisfaction never arrives.

Two bounds replace it, and neither is a prompt.

**Convergence.** The loop stops after `quiet_rounds` consecutive rounds
that produce no finding not already seen. `Convergence::seen` records
every finding ever proposed: the ones the anchor check dropped, the ones
the checker rejected, and the ones verification refuted. Deduplicating
against survivors instead would let a refuted finding come back every
round forever, and the quiet counter would never advance.

Deduplication is a Jaccard score over the token set of a finding's quote
plus its claim, within a single dimension, at a default threshold of 0.6.
Exact matching would never converge because agents rephrase. Findings are
never compared across dimensions: the same sentence can be wrong
statistically and wrong about its citation, and those are two findings.

**The round cap.** `rounds_run` increases by one per round and the loop
stops unconditionally at `max_rounds`. This is what makes termination a
property of the code rather than of the reviewer's behaviour: however the
agents answer, the loop runs at most `max_rounds` times.

**When a bound is hit, the report says so.** `Stop::Converged` is the
only outcome that reports complete coverage. `Stop::RoundCap` and
`Stop::BudgetExhausted` both print "so this review is not exhaustive" and
the report adds "**This review is incomplete.** Treat the absence of a
finding as absence of coverage." A truncated review that does not say it
was truncated reads as a clean bill of health, which is worse than no
review.

## The budget model

Depth and fan-out multiply. At a fan-out of three, depth ten is 59,049
agents at the deepest level and 88,572 across all ten levels. At a
fan-out of two it is 1,024 and 2,046. At a fan-out of one it is ten.

So depth is not a safety bound. It bounds the length of one chain of
enquiry, which is a real thing to bound (a refuter that needs to check a
cited work, which needs a second source checked, and so on), and it
bounds nothing about spend.

**The total agent budget is the bound that binds.** One `Budget` per
review, charged before any agent starts, with the depth check and the
count check in a single critical section so two agents recursing at once
cannot both read the count below the cap and both pass it. Everything
goes through it: the orchestrator's dispatches through
`BudgetedDispatcher`, and an agent's own `spawn_review_agent` calls
directly. An agent cannot know what its siblings have spent, so asking
each agent to be careful bounds nothing.

Exceeding the budget degrades honestly. A refused dispatch is recorded
with what it was for, the report lists those under "What this review did
not cover", the loop stops, and `coverage_is_complete` is false. Nothing
is silently skipped.

### The numbers

| Bound | Default | Flag |
|---|---|---|
| Rounds | 4 | `--rounds` |
| Consecutive quiet rounds to stop | 2 | `--quiet-rounds` |
| Recursion depth | 3 | `--max-depth` (hard ceiling 10) |
| Total agents | 150 | `--max-agents` |
| Refuters per finding | 3 | `--refuters` |
| Dimensions | the 10 in `core` | `--dimensions` |

Fixed cost, independent of what is found: `2 x dimensions x rounds`. For
the default core set that is `2 x 10 x 4 = 80` agents, a little over half
the budget. Verification costs `3` per fresh finding.

A realistic run on the core set: round 1 produces around 8 findings that
clear the anchor check, so 20 + 24 = 44; round 2 produces 3, so 20 + 9 =
29; rounds 3 and 4 are quiet at 20 each. Total around 113 of 150, ending
on the convergence criterion rather than a bound.

Worst case is 150, by construction. That is what a central budget buys.
Without it, the same configuration with 22 dimensions, four rounds, ten
findings per agent per round and one helper per refuter would be roughly
`176 + 1,760 x 3 + 5,280 = 16,576` agents.

**Depth 10 is honestly unreachable in width.** Under the default budget a
chain of ten costs ten agents and is fine. A tree of fan-out two to depth
ten costs 2,046 and is refused at agent 151. The setting is real as a
ceiling on chain length and would be dishonest presented as anything
else, which is why it is documented here as one.

**`--dimensions all` needs a bigger budget.** 22 dimensions at four
rounds is a fixed cost of 176, already past the default 150. Pass
`--max-agents 400` or the review runs out inside round one and says so.

## Adversarial verification

Each surviving finding goes to `refuters_per_finding` agents whose
instruction is to refute it. Not to assess it, not to improve it: an
agent asked whether a claim is reasonable agrees far more often than one
asked how it is wrong.

Three rules stop this being a rubber stamp.

- **Distinct lenses.** Five are defined (`text`, `elsewhere`,
  `standard`, `consequence`, `source`) and each verifier gets a different
  one before any is reused. Repeating one lens three times catches noise.
  Different lenses catch different failure modes, and a finding usually
  has more than one way of being wrong.
- **Uncertain counts as refuted.** A vote that is not clearly "upheld" is
  `Uncertain`, including a reply that did not parse at all, and
  `Uncertain` is counted and counted against the finding. A verifier that
  malfunctions cannot wave a finding through.
- **Strict majority.** A finding survives only if upheld votes are more
  than half the votes cast. Two of three survives; one of two does not;
  an even split does not.

Zero votes is neither. It means no verifier reached the finding, which
happens when the budget ran out, and the verdict is `Unverified`. The
finding is reported and labelled, because dropping it would hide a gap
and calling it upheld would invent support.

## Dimensions

The dimension set is a table in `dimension.rs`. The orchestrator iterates
it and branches on no dimension by name, so adding one is a row and
nothing else. Each row declares what it needs (`Needs::EvidenceRecord`,
`Needs::VenueList`) and a dimension whose inputs are absent is dropped up
front and named in the report, never run against nothing.

Categories are `Technical`, `Communication`, `Distribution`, `Executive`.
The report groups by category, prints Technical first, and prints a
caveat under every other group. A paper can be highly shareable and
wrong. A finding about reach must never be read as a finding about
correctness.

### The default set is smaller than the full set

Ten dimensions are on by default: citation integrity, claim to evidence
traceability, statistical validity, reproducibility, technical
correctness, benchmarking validity, data correctness, threats to
validity, novelty and prior art, figures and tables.

Twelve more exist and are opt-in: content quality, readability,
completeness, related work coverage, architecture validation, venue fit,
problem validation, business case and return, virality and reach, and
three executive readers (CEO, CTO, CFO).

This is a judgement, and it is the part of this design I am least
confident in, so here it is stated plainly rather than buried.

- `content-quality` and `readability` overlap heavily and both tend
  toward writing advice that would apply to anything. The anchor rule
  forces them to quote a sentence, which helps, but a quoted sentence
  plus "this is hard to read" is still close to worthless.
- `exec-ceo` is the weakest dimension in the table. Strategic fit and
  market timing have no ground truth in the paper, so the anchor rule
  can force a quote and cannot force the reasoning about it to be
  anchored in anything. `exec-cfo` is the strongest of the four
  executive ones, because "trace what this number is computed from" is a
  concrete instruction with a checkable answer.
- `virality-reach` is half checkable ("is the contribution in the first
  paragraph", "state the single quotable claim") and half taste.
- `architecture-validation` is near-vacuous for an empirical paper and
  real for a systems one.
- `completeness` overlaps `reproducibility` and `threats-to-validity`.

I could not measure any of this, because no test here calls a model and I
did not run one. So rather than assert it, the capability ships the
instrument: the report's per-dimension table records proposed,
unanchored, repeat, checker-dropped, refuted, and surviving counts. A
dimension that proposes steadily across several real papers and never
survives verification is padding the report, and after a few runs that
will be visible in the table rather than a matter of opinion. Until then
the default set is the conservative choice.

## What this cannot catch

- **Whether a citation exists.** Reviewer subagents are built from
  `AgentConfig`, which carries the model, prompt, step limit, repo root,
  cancel token, and approval mode, and no tool registry. They get
  `register_builtins()`, which is local file access. MCP tools attached
  to the parent agent do not reach them. So citation integrity is checked
  against what is on disk and not against the published record, and the
  report says so unconditionally under "What this review did not cover".
  Threading a tool registry through `AgentConfig` is the fix and it is a
  change to inherited harness code that this work did not make.
- **A wrong finding all three refuters share a blind spot about.**
  Diversity of lenses reduces this and does not remove it. Every verifier
  is the same model.
- **Anything the anchor rule excludes.** A real problem that is about
  what the paper does not say, and so has nothing to quote, cannot be
  reported. This is a deliberate trade: the filter that removes padding
  also removes findings about absence. `threats-to-validity` and
  `completeness` are written to quote the surrounding text as a way
  around it, and that is a workaround, not a solution.
- **Whether the paper is any good.** The report is a list of specific
  defects. It has no opinion on the whole.

## Relationship to the self-critique work in `deliver`

A single-pass self-critique of a draft against the evidence record is
being built inside `deliver` at the same time as this. The two overlap on
exactly one dimension: `claim-evidence-traceability` is the same check,
run once, without verification and without a convergence loop.

They should not both exist for long. The right end state is that
`deliver`'s self-critique calls `review` with
`--dimensions claim-evidence-traceability --rounds 1 --refuters 0`,
rather than two implementations of "check the draft against the record"
drifting apart. Doing that now would couple two branches under review at
once and make both harder to judge, so it is written down here instead of
done.
