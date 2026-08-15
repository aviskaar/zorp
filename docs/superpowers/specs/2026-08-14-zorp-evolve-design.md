# evolve: population search over a question graph

**Date:** 2026-08-14
**Status:** approved, not built

## Purpose

A fifth capability alongside validate, investigate, co-write, and
deliver. Where `investigate` runs one attempt against a pre-registered
metric, `evolve` runs a population of competing decompositions of the
same question, eliminates the weak ones, and breeds the survivors.

The point is not speed. The point is that a defensible answer needs
independent lines of evidence that converge, and a single attempt cannot
produce that no matter how good it is. A population can, but only if
selection is built so the population does not inbreed. Most of this spec
is about that constraint.

The search core is an adaptation of ERBGA, published as Rao, Janikow,
Bhatia, and Climer, "Efficient Reduced-Bias Genetic Algorithm (ERBGA)
for Generic Community Detection Objectives", MWAIS 2018 Proceedings 32,
and in the longer thesis of the same name. zorp's author is that work's
first author, so this is building on prior work of our own, not
vendoring someone else's.

Read `docs/superpowers/specs/2026-08-09-zorp-investigate-design.md`
first. `evolve` reuses its pre-registration discipline and writes into
the same experiment and metric tables.

## The core idea

A question decomposes into sub-questions, and sub-questions bear on each
other. Model that as a **question graph**: vertices are candidate
sub-questions, and an edge (a, b) means evidence for a and evidence for
b should be reasoned about together.

Cutting edges splits the graph into connected components. Each component
is an **independent line of inquiry**. A good cut produces lines that
are internally coherent and mutually independent, which is exactly what
modularity measures. It is also exactly the evidence structure that
makes an answer defensible: independent lines that agree are
corroboration, whereas one line counted twice is not.

So the search is over cuts of a question graph, and ERBGA is a search
over cuts of a graph. The two are the same problem.

## Why ERBGA's representation, specifically

Two properties matter here, and both come from the representation
choice rather than from the algorithm around it.

**No redundancy.** The obvious encoding, a list of sub-questions grouped
into lines, has n! encodings of the same decomposition because the order
is arbitrary. That is the k! blowup ERBGA was built to remove. Encoding
the decomposition as the **set of removed edges** gives each
decomposition exactly one representation.

**The vertex set is invariant.** A chromosome is a bit string over
edges, so every chromosome in every generation on every island asks the
same sub-questions. Only the grouping varies. That is what makes this
affordable, and it is the whole reason the approach works at all:

- Gathering evidence for a sub-question costs a tool-using agent run.
  It is paid **once**, globally, and memoized. Total evidence cost is
  bounded by the number of vertices, not by population times
  generations times islands.
- Evaluating a cut costs nothing but arithmetic over evidence that
  already exists.

The representation chosen to kill redundancy is also the one that makes
memoization maximally effective, because it holds the expensive
dimension fixed and varies only the cheap one. Population size and
generation count stop being budget questions.

**No prior knowledge of the number of lines.** ERBGA needs no cluster
count up front, and neither does this. You do not know in advance how
many independent lines of evidence a question has.

## Where this lives

Two crates, so the search core can be tested without a model in the
loop:

- **`erbga`**, a new workspace member. The pure genetic algorithm over
  an undirected graph, with a pluggable objective. Knows nothing about
  questions, evidence, agents, or zorp. Its correctness is established
  against published community detection benchmarks.
- **`zorp-agent/src/evolve/`**, the integration: question graph
  construction, the memo table, evidence gathering, epoch control,
  checkpoints, and the `zorp-agent evolve` subcommand. Behind the
  existing `research` feature, and additionally requiring the `library`
  feature, since the memo table is LanceDB.

This follows the rule in `CLAUDE.md` that new zorp-specific capabilities
live in new crates or clearly named modules rather than inside inherited
harness code.

**Open naming question.** `evolve` is named after its mechanism, while
the other four capabilities are named after their purpose. It is used
throughout this spec as a placeholder and can be renamed before
implementation without changing anything else here.

## The chromosome

Two gene arrays.

**Edge genes.** One bit per edge in the island's question graph, 1 = the
edge is removed. Edges are identified and ordered exactly as in ERBGA:
a unique id per edge, a sorted `EdgeList`, and an inverse mapping back
to endpoints. The connected components induced by the surviving edges
are the lines of inquiry.

**Method genes.** One gene per vertex, naming how that sub-question's
evidence is gathered. Method is a real gene rather than a label because
the memo table is keyed on `(sub_question, method)` pairs, so two
methods on one sub-question are two separate pieces of evidence. This is
what lets the population surface method sensitivity instead of hiding
it.

## Operators

Adapted from ERBGA. Cost is what distinguishes them, and cost is the
only thing that constrains the search.

| Operator | Cost | Behavior |
|---|---|---|
| Uniform crossover | free | A random list of crossover points, single genes exchanged at those points only, applied to both gene arrays |
| Edge mutation | free | Flip a bit. Regroups existing evidence |
| Gene Repair | free | Reattach cut edges around high-degree vertices |
| Elitism | free | The best fraction carries forward unaltered |
| Tournament selection | free | A pool is drawn at random, the best two breed |
| Method mutation | **one evidence run** | Creates a new `(sub_question, method)` memo entry |
| Vertex mutation | **one evidence run** | The variation model proposes a new sub-question, growing the graph |

**Uniform crossover** matters for the reason ERBGA gives: sub-questions
are not linearly ordered, so adjacency in the bit string is an artifact
of how `EdgeList` was sorted, not a real relationship. Single point
crossover would impose structure that does not exist.

**Gene Repair** translates directly and gains a clear meaning. In ERBGA
it addresses dense networks that stay connected despite heavy edge
removal, by reattaching edges incident to high-degree vertices, on the
reasoning that a high-degree vertex has far more intra-cluster than
inter-cluster edges. Here, degree is the number of other sub-questions
that bear on a given one. A hub sub-question is foundational, many
things depend on it, and cutting its edges is usually an artifact of
random cutting rather than a real epistemic boundary. Repairing those
cuts keeps foundational sub-questions attached to what depends on them.

Without it, a densely coupled question, where everything bears on
everything, collapses into one undifferentiated line of inquiry. That is
the failure mode ERBGA reports on its denser datasets, and it is
precisely the kind of question zorp exists to handle.

## Two-tier fitness

An evidence-free fitness evaluation is still an LLM call if the metric
is a judgment. At ERBGA-scale parameters that is millions of calls, so
fitness splits in two.

**Inner tier, cheap and deterministic.** A structural proxy computed in
Rust, drives the full genetic search at ERBGA-scale parameters. Because
the proxy is pre-registered, it is a named and versioned function rather
than an informal notion. v1 ships exactly one, `modularity-coverage-v1`,
a weighted sum of three terms over the cut:

- **Modularity** of the question graph under the cut, the same measure
  ERBGA optimizes. High when lines are internally dense and mutually
  sparse.
- **Coverage**, the fraction of vertices in the cut that have a memo
  entry for their method gene. Penalizes cuts leaning on evidence
  nobody has gathered.
- **Balance**, penalizing cuts that strand most vertices in one giant
  line, which is the collapse mode Gene Repair exists to fight.

Its weights are part of the named version, so changing them produces a
different proxy name and a different pre-registration rather than a
silent retune. **The proxy can never kill anything.** It ranks
candidates, nothing more.

**Outer tier, expensive.** At each epoch boundary the pre-registered
metric is evaluated on elites and island bests only, by synthesizing the
already-gathered evidence for that cut. This is the only fitness that
decides whether anything lives or dies, and each evaluation is written
as an ordinary row in the existing `experiments` and `metrics` tables.

The surrogate risk is real and is accepted deliberately: the proxy may
rank cuts differently than the true metric would. It is bounded by the
proxy having no authority. A cut the proxy loves still has to survive
the pre-registered metric to matter.

## Selection

Tournament selection as in ERBGA, over fitness penalized by similarity
to survivors already selected in that generation. Each additional
survivor must be both good and different.

Convergence is the goal in optimization and a failure mode here. Eight
survivors that agree because they share a decomposition and a source
are not eight confirmations, but they look like eight confirmations,
which is worse than one honest attempt.

The diversity coefficient is pre-registered, so it cannot be tuned after
someone sees which survivors it produced. At zero it degrades to plain
tournament selection, so ERBGA's original behavior stays reachable and
stays on the record.

## Islands

Each island evolves independently and **builds its own question graph**
from a separate framing pass.

In ERBGA, islands guard against an unlucky initial population. Here they
do that and something more important. The question graph is constructed
by a model, so the search is only as good as the graph, and a single
graph is a single point of failure that ERBGA does not have, since ERBGA
is handed a real network. Per-island graphs turn framing variance into
diversity instead.

The payoff is the strongest evidence structure this system can produce:
independently framed investigations, run independently, converging on
the same answer.

The cost is reduced memo sharing, since islands share evidence only
where their framings propose the same sub-question. Semantic lookup
recovers most of it. Two islands asking "p99 latency of the Kafka read
path" and "how slow is the tail on current Kafka reads" resolve to the
same memo entry despite the different wording.

## The memo table

The dynamic programming layer. Sub-questions are subproblems, evidence
is the memoized result, and the population collectively explores the
space while paying for each subproblem once.

- **Keyed** on `(sub_question, method)`.
- **Stored** in LanceDB, which is why this capability needs the
  `library` feature that `research` deliberately leaves off.
- **Looked up semantically**, with a similarity floor deciding hit
  versus miss, because a natural-language key would almost never match
  exactly.
- **Read and written only by the controller.** Workers never touch it.

A hit is not free of risk. A similarity floor set too low reuses
evidence that does not actually answer the sub-question being asked, and
the failure is silent. The floor is therefore recorded with every hit,
along with the similarity score and the originating sub-question, so any
reuse can be audited after the fact.

Entries carry the epoch they were gathered in. Staleness rules are not
specified here beyond that; the recorded epoch is enough to add them
later without a migration.

## Controller and workers

**The controller is code, not an agent.** Every decision that ends
something runs deterministically in Rust: memo lookup, proxy
computation, the similarity penalty, tournament selection, threshold
enforcement, and termination. If you cannot replay why a candidate died,
the defensibility claim is theatre.

The model is used at exactly three points, all of them generative rather
than judgmental: constructing an island's question graph, proposing a
new sub-question during vertex mutation, and synthesizing evidence at
the outer fitness tier. Every output of those three is recorded as data.

**Workers gather evidence and nothing else.** They are subagents from
the existing `SubagentPool` in `zorp-agent/src/tools/subagent.rs`, which
already provides per-worker progress, status, and a `CancelToken`, and
`running_count` for capping concurrency.

Workers get no store handle. DuckDB takes an exclusive file lock, which
`zorp-track` already knows about and tests for in
`zorp-track/src/project.rs`, so a single writer is a hard constraint
rather than a stylistic preference. The controller resolves memo hits on
a worker's behalf and injects them into its prompt, so a worker receives
a sub-question, a method, and the relevant known evidence, and returns a
structured result. This keeps workers stateless, parallel, and killable.

## The epoch loop

An **epoch** is one round of expensive work. The cheap genetic search
runs to convergence inside it, unattended.

1. **Frame.** Each island builds its question graph, on the first epoch
   only.
2. **Gather.** Every vertex without a memo entry for its method gets a
   worker run. This is the expensive step.
3. **Evolve.** The full genetic search runs per island against the
   structural proxy: initialize, then repeat tournament selection,
   uniform crossover, mutation, Gene Repair, and elitism for the
   configured number of generations. No model calls, no evidence
   gathering.
4. **Score.** The pre-registered metric is evaluated on elites and
   island bests. Each evaluation is recorded as an experiment with a
   metric.
5. **Checkpoint.** One prompt per epoch.
6. **Grow.** Vertex and method mutation propose new sub-questions and
   new methods for the next epoch, which is the only thing that makes
   the next epoch cost anything.

## Termination

**Evidence exhaustion.** The run stops when the memo hit rate crosses a
pre-registered saturation ceiling, meaning the population keeps
proposing sub-questions that are already answered and has stopped
discovering anything. This measures the thing that actually matters,
new evidence, rather than a proxy for it, and it correlates with cost
without imposing the hard budget cap that a prior decision rejected.

**Backstop:** the best pre-registered metric across islands failing to
improve for a configured number of epochs.

## Pre-registration

`evolve` extends `investigate`'s pre-registration. The rule for what
belongs there:

**Pre-register anything that could bias which answer wins. Merely record
anything that only affects how hard you searched.**

Pre-registered, written into `prereg.md` and covered by the existing
SHA-256 hash and git commit:

- Metric name, threshold, and threshold direction, as today.
- The diversity coefficient.
- The saturation ceiling.
- Which structural proxy is used, by name and version. Swapping proxies
  after seeing results is p-hacking, so it is committed up front.
- The memo similarity floor. This one belongs here rather than with the
  recorded parameters, because a floor set low enough reuses evidence
  that does not answer the sub-question being asked, and that changes
  which answer wins rather than how hard the search worked.

Recorded in the run record but not pre-registered: population size,
generation count, island count, mutation rates, elitism rate, Gene
Repair parameters, and the stagnation backstop length. These change how
thoroughly the space is searched, not which answer is favored.

## Elimination and track death

The existing rule is that a threshold breach kills the track and is
exempt from `AutoApprove`. Under a population that rule has to split,
because individuals breaching a threshold is elimination working
correctly, not a failure signal. Left unsplit, a single run would stop
for a human dozens of times and then kill the thing it was
investigating.

- **An individual breaching dies.** Recorded with its reason. No human
  involved. This is the elimination step.
- **The track dies** when every island's best breaches at an epoch
  boundary. The `AutoApprove` exemption attaches here and only here,
  which preserves the intent of the original decision rather than
  firing it thousands of times.

## Checkpoints

One checkpoint per epoch, showing island bests, where islands agree and
disagree, best-so-far against the pre-registered threshold, memo hit
rate, vertex count growth, and cost so far.

Cross-island disagreement is presented as a finding rather than as
noise. Two independently framed investigations reaching different
answers is information about the question, and it is exactly what
`co-write` is specced to surface.

Interrupts fire before an epoch boundary on:

- Cross-island divergence past a floor.
- A configured fraction of budget consumed.
- Population collapse, meaning diversity below a floor on every island.

## What co-write gets

Nothing in `co-write` changes. Each outer-tier evaluation is an ordinary
experiment with an ordinary metric, so `co-write` reads the output as it
reads `investigate`'s.

What it gains is the thing its own spec asks for and has never had:
recorded conflicting evidence. "Evidence that conflicts with the draft's
conclusion must be surfaced, not silently dropped" has been difficult to
honor when a track holds a handful of attempts that never disagreed
because they were never independent. A population of independently
framed islands produces disagreement structure as a normal output.

## Error handling

- Track status is already `Killed`: refuse before building anything, as
  `investigate` does.
- No prereg exists and the required flags were not given, or a prereg
  exists and the flags contradict it: same two errors `investigate`
  already has, extended to cover the new pre-registered fields.
- Question graph construction returns an unusable graph, meaning fewer
  than two vertices or no edges: that island fails and is recorded as
  failed. The run continues if any island survives, and fails if none
  do.
- A worker returns an unparseable result: that `(sub_question, method)`
  gets no memo entry, and chromosomes depending on it score as having
  missing evidence rather than failing the run.
- A worker is cancelled or times out: same as unparseable. The
  `CancelToken` path already exists.
- The outer metric evaluation fails to parse: that experiment is marked
  `Failed`, matching `investigate`, and the candidate does not count
  toward track death, since a failed measurement is not a breach.
- Every island fails: a distinct error, not a silent empty result.

## Testing

**The search core, without a model.** `erbga` is validated against the
published benchmarks before it is ever pointed at a question. Targets
are ERBGA's own reported results rather than the literature best:
modularity of about 0.420 on Zachary's Karate Club and about 0.465 on
the Dolphin social network. If those do not reproduce, the search engine
is broken, and that is established with zero model calls.

The paper and the thesis disagree on two parameter values, and the
benchmark test depends on resolving them:

| Parameter | Paper | Thesis |
|---|---|---|
| Random Population Rate | 0.25 | 0.85 |
| Tournament pool size | 3 | 7 |

Both need confirming against the original implementation before the
benchmark numbers can be treated as a regression target.

**Operators.** Property tests: uniform crossover preserves gene array
length, edge mutation flips exactly the bits it claims to, Gene Repair
only ever reattaches edges and never cuts them, elitism never lowers the
best proxy score between generations, and a decomposition round-trips
through the `EdgeList` mapping unchanged.

**Representation.** The redundancy claim is testable directly:
generating distinct cuts must never produce two chromosomes that induce
the same partition of vertices.

**Memo table.** Unit tests over hit and miss around the similarity
floor, and a test that a hit records its score and originating
sub-question so reuse is auditable.

**Integration.** A stub model, following `validate_integration.rs`, over
a small fixed question graph with a scripted worker result per
sub-question. Covers a full epoch, elimination of an individual on a
breach, track survival when only some islands breach, track death when
all island bests breach, and the `AutoApprove` exemption applying only
to track death.

## Implementation order

This is larger than the other four capability specs and should not be
planned as one undifferentiated block. The dependency order is also the
risk order, so each stage de-risks the next:

1. **`erbga` crate, no zorp integration.** Representation, operators,
   islands, tournament, elitism, Gene Repair, pluggable objective.
   Done when the Karate Club and Dolphin benchmarks reproduce.
2. **Question graph and memo table.** Graph construction, semantic
   lookup, and the audit record for hits. Testable with a stub model.
3. **Epoch loop and the outer tier.** Worker dispatch, metric
   evaluation, storage into the existing experiment tables.
4. **Checkpoints, interrupts, and elimination semantics.** Last,
   because it is the layer that most needs the ones below it to be
   trustworthy first.

Stage 1 is the one that can fail on its own terms and is worth
completing before committing to the rest.

## Out of scope

- **Cross-island migration.** ERBGA's own future work names it, and it
  is harder here: chromosomes are bit strings over an island's own edge
  set, so under per-island graphs they are not portable between islands
  at all. It needs its own design.
- **Evolving the proxy.** The proxy is pre-registered specifically so it
  cannot move during a run.
- **Prioritized or biased initialization.** ERBGA's future work suggests
  biasing the initial population to improve early breakup. Worth doing,
  not v1.
- **Roulette selection alongside tournament**, also from ERBGA's future
  work, and noted there as possibly reducing diversity, which is the
  thing this design most needs to protect.
- **Replacing `investigate`.** `evolve` is a fifth capability, not a
  replacement. One attempt against one metric remains the right tool
  when the question is not worth a population.
- **ERBGA's memory work.** The 3D bit array and the 85% memory
  reduction solve a problem this system does not have. The bottleneck
  here is model calls, not RAM, and a straightforward representation is
  preferred over a packed one until profiling says otherwise.
