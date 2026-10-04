---
status: accepted
date: 2026-08-19
---

# the discovery layer is called aryabhatta, and erbga is wired into it

**Decision, part one:** the anomaly-driven inquiry subsystem is named
**aryabhatta**, and it is the home for discovery ideas generally. New ideas
about how zorp derives its own questions land there rather than becoming
capabilities of their own. It stays record-and-readers, the way `critique` is
a gate, so it is not a fifth capability. `docs/ARCHITECTURE.md` still says
four is the whole set.

**Decision, part two:** ideas live in aryabhatta at a state, not at a
yes or no. The registry in the spec has four: Proposed, Gated, Building,
Retired. An idea advances on stated evidence, never on enthusiasm, and
nothing is deleted for failing. The point is that everything can belong
while each part stays independently killable, because step 3 of the
implementation order is a stop sign and a monolith cannot be stopped.

**Decision, part three:** `erbga` is integrated as the backend of the
subsystem's community detection service, for graphs too large to solve
exactly. This reverses part of 2026-08-15, which shipped it off any critical
path, and `CLAUDE.md` requires an entry saying so before it is wired in.
This is that entry.

**Why erbga now, and not later:** it has a caller that does not depend on
the anomaly ledger. Bundling confounded conditions reads the `conditions`
table from step 1, sits on the ungated side, and answers a real question:
when two conditions never vary independently, no effect can be attributed
to either alone. That is aliasing in the design of experiments sense, and
it strengthens `invariant_condition`, which is blind to a pair that always
moves together while both vary.

**What the integration had to solve, rather than wave through:**

- **The threshold.** `erbga` takes unweighted edges, so a continuous
  similarity has to be cut somewhere, and an unpriced cut is exactly the
  defect that sank `evolve` (the blob-maximizing score, with edge addition
  free). The service does not price θ, it refuses to pick one: it sweeps a
  range and keeps only communities surviving a contiguous band, recording
  the band. Nobody chooses θ, so nobody can choose it to reach an answer.
- **The scale.** 2026-08-15 observed that at `V = 20` an exact
  clique-partitioning ILP finishes in about 0.2 seconds. That stands, so the
  backend is chosen by graph size: exact below the crossover, `erbga` above.
  In the band where both run, the exact result is a continuous regression
  check on the search against proven optimality, which the four fixed
  benchmark networks cannot give.
- **The stochasticity.** Seed, island count and parameters are recorded
  beside the partition, so a clustering is reproducible like any other
  recorded result.

**What may not be claimed.** Reading the source rather than the module
names: `chromosome.rs`, `rng.rs`, `selection.rs` and three of the five
operators are representation-agnostic and transfer. `RepairTargets` and
`gene_repair` use `graph.degree()` and `graph.incident()` and do not. Both
things that make erbga *erbga* are graph-specific: the reduced-bias encoding
solves a `k!` label-permutation blowup a non-graph genome does not have, and
Gene Repair needs a vertex degree a `condition_key` does not have. The four
benchmarks certify ERBGA on graphs and nothing else, so any future consumer
reusing only the scaffolding is running a new algorithm and needs its own
validation. Calling such a thing validated prior work would be false.

**Rejected:** spawning an agent per population member, each holding a
hypothesis, with the unfit eliminated. That is `evolve` again, and the first
of its three findings applies unchanged: there is no free inner search,
because variation is model-proposed. erbga's own thesis parameters are
250,000 fitness evaluations per island across 25 islands, which is fine for
bit flips and impossible for model calls. A few hundred calls is a
tournament, not evolution. Two further objections also carry over: a
population drawn from one model is one model sampled many times, and
selecting on a metric that is then reported is biased upward twice. The
structured-genome version, where the model proposes the vocabulary once and
the search over combinations runs in code, is recorded as Proposed in the
registry rather than built.

**Also folded into the spec:** the prior art pass
(`docs/superpowers/specs/2026-08-19-anomaly-driven-inquiry-prior-art.md`).
Its findings change what the design may claim, not what it builds. The
architecture is substantially AutoDiscovery (arXiv 2507.00310), which must be
cited and distinguished. The calibration question is answered next door for
the exogenous case, and badly: FermiEval reports 65% coverage at a nominal
99%, so the step 3 stop sign is now more likely to fire than not, which is an
argument for keeping it exactly where it is. What survives is the endogenous
case, where the quantity being measured is manipulable by the thing being
measured.

**Full writeup:**
`docs/superpowers/specs/2026-08-19-anomaly-driven-inquiry-design.md`, kept in
sync with `designs/` in the kb repo. The subsystem as a whole is still
proposed and not approved; these three decisions inside it are taken.
