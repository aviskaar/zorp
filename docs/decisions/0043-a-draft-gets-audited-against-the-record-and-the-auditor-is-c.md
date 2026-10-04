---
status: accepted
date: 2026-08-18
---

# a draft gets audited against the record, and the auditor is code

**Decision:** `zorp-agent critique "<question>"` audits a track's
`draft.md` against that track's evidence record and revises what the
record does not support. It is not a fifth capability. It has no scope of
its own, gathers nothing, and produces no evidence: it reads the record
and edits the artifact `co-write` produced. Four capabilities is still
the whole set. See
[`superpowers/specs/2026-08-18-zorp-self-critique-design.md`](superpowers/specs/2026-08-18-zorp-self-critique-design.md).

**Why the model does not judge:** asking a model whether its own draft is
good produces a confident answer about a confident answer. The model's
only job here is extraction: inventory the draft's claims and name, from
a fixed list, the one piece of evidence each rests on. Code decides
whether that evidence exists. Alongside it, a purely deterministic pass
flags every figure in the draft that the record cannot account for at any
rounding, with no model involved at all, so a critic that reports nothing
cannot declare a draft clean.

**Why it terminates:** two bounds, neither of them the model's opinion. A
configured round bound (`--critique-rounds`, `ZORP_CRITIQUE_ROUNDS`,
default 2, where 0 means audit only), and a strict-improvement rule: a
revision is kept only if it leaves strictly fewer findings than the draft
it replaced, and the first one that does not ends the pass. Findings are
a non-negative integer that must strictly decrease, so the loop cannot
run longer than the initial finding count. That rule is also what makes
"the draft is fine" reachable: a clean draft costs one model call and
nothing is rewritten.

**What is recorded:** a `critiques` table in `zorp-track`, one row per
audited draft (round, draft hash, findings as JSON, whether it was
carried forward), plus `draft.pre-critique.md` for the diff and
`critique.md` for reading. Findings are deliberately **not** recorded as
metrics: they would then be evidence `co-write` reads, and a later draft
could cite its own critique as a finding about the question.

**What it rules out moving:** the Kill Threshold and everything else
pre-registered. The pass snapshots the record before it runs and
re-checks it after every model turn, and any movement aborts with the
draft untouched. This is enforced at runtime rather than assumed from the
call graph, and it is tested against a write-capable agent that really
does rewrite `prereg.md` mid-run.

**What it cannot catch:** that cited evidence actually implies the
sentence citing it. The pass verifies the evidence exists, not that it
supports the claim. Derived figures also read as invented. Both are in
the spec's limits section.
