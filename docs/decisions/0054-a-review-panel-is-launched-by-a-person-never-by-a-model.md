---
status: accepted
date: 2026-08-20
---

# a review panel is launched by a person, never by a model

**Decision:** the review panel is a button in the browser and a function in
`zorp-agent`. There is no model-callable tool to spawn a reviewer, and none is
planned.

**Why:** an agent that can spawn agents can spawn agents that spawn agents, and
nothing in the loop bounds that. A human-launched panel has a natural bound:
one click, one panel, a fixed number of reviewers, each of which runs with no
panel of its own. `zorp-agent/src/agent.rs` already carried a test asserting a
filtered agent has no `spawn_subagent`, `monitor_subagents`, `cancel_subagent`
or `invoke_subagent`. That test still passes and this work did not add any of
them.

**A reviewer gets strictly less than the panel that launched it.** Read-only
tools by allow-list, named one at a time rather than derived by excluding the
dangerous ones, because an allow-list stays correct when a new tool is added
and a deny-list silently grants it. No `write_file`, no `apply_patch`, no
`run_command`. An opinion that can edit the thing it is reviewing is not a
review. This is the rule `zorp-skill` already follows for skill bodies.

**The panel is not `critique`, and the two must not merge.** Critique is a gate:
it audits a draft against a track's own evidence record, the audit is
arithmetic, and it refuses if the record moved underneath it. The panel is a
reader: it produces opinions, changes nothing, and can be wrong. Conflating
them would let an opinion block a deliverable the evidence record was happy
with.

**Agreement is computed in code and never negotiated inside the panel.**
Reviewers do not see each other's findings, and the prompt says so. Reviewers
that read each other converge, and a panel that converges is one reviewer with
extra cost. Corroboration counts *lenses* rather than findings, so one reviewer
listing the same objection three times cannot corroborate itself. Locus
matching normalizes case and surrounding whitespace and nothing else: a fuzzier
match would merge two different objections that happen to be worded alike, and
inflating a corroboration count is the one error the function must not make.

**A failed reviewer is a first class part of the report.** A panel of five where
two fell over is not a panel of three. Without `lenses_requested` next to
`verdicts`, "every reviewer agreed" can mean "the one that ran agreed", and the
browser leads its summary with the shortfall for the same reason.
