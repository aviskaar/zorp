# evolve: population search over question framings

**Date:** 2026-08-14
**Status:** NOT APPROVED. Under revision after two rounds of adversarial
review. Do not implement from this document.

Read "Where this stands" immediately below before reading anything else.
Several sections of this spec are known to be wrong, and they are left in
place deliberately so the findings against them have something to point
at.

## Where this stands

Two rounds of adversarial review, eight independent reviewers. The
measurement discipline in this document survived both rounds and is
worth keeping. The search layer did not survive either round, and the
second round showed the first rewrite moved the flaw rather than removing
it.

**What survived, confirmed by reviewers on both rounds:**

- Never selecting on the pre-registered metric, because breeding a
  population to maximize a metric and then reporting that metric is
  biased upward twice over, and pre-registration does not cover it.
- The confirmatory stage: `n` independent passes, threshold on the mean
  rather than the best, nulls counted as non-passing rather than exempt.
  Reviewers on both rounds tried to game it and could not, given a fixed
  evidence set.
- Refusing to call framing diversity corroboration, because islands
  share one model and agreement then carries almost no evidential weight.
- Track death on a quorum rather than unanimity.
- Rejecting memo hit rate as a termination signal, since it reduces to
  `1 / (1 + g)` and measures graph growth rather than discovery.

**What did not survive, and why a third patch is not the answer:**

1. **There is no free inner search.** This document prices structural
   operators as "free" meaning free of evidence runs, then the Cost
   section reads that as free outright and concludes a large population
   over many generations "costs nothing beyond CPU", while the epoch loop
   claims "No model calls". Those cannot all hold. Variation is
   model-proposed, so either the search performs no variation and
   converges in one iteration, or it costs `I * P * G * E` model calls
   that appear in no cost model. Without free variation, a genetic
   algorithm over framings has no reason to exist.

2. **The framing score is maximized by a blob.** CPM's objective is
   extensive, not normalized. `H*` is monotone non-decreasing under edge
   addition, and edge reweight is priced free, so the search adds edges
   until the graph is complete, where one undifferentiated block beats a
   real decomposition by more than 6x and scores maximum on the other two
   terms simultaneously. Modularity is normalized and comparable across
   graphs; CPM is not. Swapping one for the other to fix the resolution
   limit inherited a comparability assumption that does not hold.

3. **Two of the three score terms are identically 1.0.** `Grounding` is
   satisfied by construction, since the genome mandates a stated reason
   on every pair and `reuse_floor` makes its second clause unreachable.
   `Evidential independence` is Jaccard over memo rows, but a partition
   is vertex-disjoint and each sub-question owns its row, so components
   can never share one. This is the same defect as the first draft's
   `Coverage` term, reintroduced under two new names, with the same
   stated cause (gathering precedes evolution) preserved unchanged.

4. **Free operators still select which evidence is reported.** The
   confirmatory stage runs over the winning partition's evidence, and
   `drop` is free and raises two of the three terms. So a search that
   never touches the metric still decides which already-paid-for evidence
   reaches the reported number, which is the exact failure this document's
   Purpose section says it exists to prevent.

5. **The resolution-limit argument is quantitatively wrong.** The
   threshold `sqrt(2m)` bounds total degree, not internal edges; the
   internal-edge form is `sqrt(m/2)`. Karate's optimal communities hold
   {6, 7, 21, 23} internal edges, not the "15 to 20" claimed here, and
   the smallest is below the corrected threshold rather than comfortably
   above it. On a question graph the conclusion inverts: the limit binds
   for tree-like lines and not for denser ones. CPM may still be the
   right choice, but the sole stated justification for it is wrong in all
   three of its numbers.

**Recommendation.** The evolutionary search should be dropped or reduced
to something much smaller, and the measurement discipline kept. At the
scale a question graph actually occupies, an exact clique-partitioning
ILP solves the partition to proven optimality in about 0.2 seconds at
`V = 20`, so a genetic algorithm is not needed for the cut, and there is
no cheap search over framings to justify one there either. What is worth
building is the confirmatory measurement machinery on top of ordinary
`investigate` runs.

The full findings from both rounds are recorded at the end of this
document under "Review record".

---

The design below is the second draft, preserved as written. The first
draft searched over cuts of a fixed question graph and selected on the
pre-registered metric; what changed between drafts is recorded under
"What review changed".

## Purpose

A fifth capability alongside validate, investigate, co-write, and
deliver.

`investigate` runs one attempt at answering a question. `evolve` does
something different and narrower: it searches for a **good way to
decompose** the question into independent lines of inquiry, then
measures the answer once, carefully, on the decomposition it found.

The distinction is the whole design. `evolve` does not search for an
answer.

## What evolve does not do, and why

**It never selects on the pre-registered metric.**

If you breed a population to maximize a metric and then report that
metric, the number is biased upward by construction. Every decomposition
that surfaces inconvenient evidence scores worse, dies, and does not
breed, so within a few generations the population consists precisely of
the framings that avoid the problem. The disconfirming evidence remains
in the record, paid for, and reaches nothing.

Pre-registration does not protect against this. Pre-registration stops
you from moving the test after seeing the data. It does nothing about
selecting, from many observations, the one that best clears a fixed
test. Those are different failure modes, and only the first is covered
by committing a threshold in advance.

There is a second, independent bias in the same direction. Any metric
produced by a model synthesizing evidence is a noisy estimator. The
maximum of `N` draws from a noisy estimator exceeds the truth by roughly
`sigma * sqrt(2 * ln N)`. A search that reports its best evaluation is
reporting that maximum.

So the pre-registered metric appears **nowhere** in selection, at any
tier, in any generation. It is measured once per island, after the
search has finished, under the confirmatory protocol below.

## The core idea

A question decomposes into sub-questions, and sub-questions bear on each
other. Model that as a **question graph**: vertices are sub-questions,
and a weighted edge (a, b) records how strongly evidence for a and
evidence for b must be reasoned about together.

Partitioning that graph yields **independent lines of inquiry**. Lines
that are internally coherent and mutually separable are what a
defensible answer needs, because independent lines that agree carry
weight and one line counted twice does not.

The critical design question is where to spend compute. The partition of
a 20-vertex graph is a small problem with good deterministic solvers.
The **graph itself** is where all the uncertainty lives: it is invented
by a model, it is never observed, and a wrong edge silently manufactures
or destroys an entire line of inquiry.

So the population searches over **framings**, and the partition of any
given framing is solved directly rather than evolved.

## Where this lives

Three crates, so each layer can be tested without the one above it:

- **`erbga`**, a new workspace member. A question-agnostic genetic
  algorithm over graphs with a pluggable objective, implementing the
  representation and operators of Rao, Janikow, Bhatia, and Climer,
  "Efficient Reduced-Bias Genetic Algorithm (ERBGA) for Generic
  Community Detection Objectives", MWAIS 2018. Used here as the
  partition solver for framings too large to solve directly, and
  validated independently against that paper's benchmarks. zorp's author
  is that work's first author.
- **`partition`**, a small module inside `erbga`. The deterministic
  partition solver used at question-graph scale, plus the objective
  functions. Separate from the GA because at realistic sizes the GA is
  not the right tool.
- **`zorp-agent/src/evolve/`**, the integration: framing construction
  and mutation, the memo table, evidence gathering, epoch control, the
  confirmatory stage, checkpoints, and the `zorp-agent evolve`
  subcommand.

**Open naming question.** `evolve` is named after its mechanism, while
the other four capabilities are named after their purpose. It is a
placeholder and can be renamed without changing anything else here.

## The framing genome

An individual is a **framing**, not a bit string.

- **Sub-questions.** A set of natural-language questions, each with a
  gathering **method** drawn from a fixed, closed enum declared in the
  pre-registration. Method is part of the genome because the memo table
  is keyed on `(sub_question, method)`, so two methods on one
  sub-question are two separate pieces of evidence.
- **Relations.** A weighted "bears on" score in `[0, 1]` for each
  ordered pair the framing model considered, plus a stated reason. An
  edge exists where the score meets the pre-registered threshold `tau`.
  The **full weighted matrix is recorded**, not just the thresholded
  graph, so `tau` is auditable after the fact and a different threshold
  can be replayed against the same framing.
- **Cross-cutting premises.** Sub-questions the framing model marks as
  bearing on everything, for example "is the underlying data reliable".
  These are **excluded from the partition** and attached to every
  component as a shared premise. A universally relevant sub-question is
  not a line of inquiry, it is a precondition of all of them, and left
  in the graph it is a maximum-degree vertex that prevents the graph
  from separating at all.

## Operators

Variation is model-proposed and structural. Selection and scoring are
deterministic code.

| Operator | Cost | Behavior |
|---|---|---|
| Edge reweight | free | Adjust a "bears on" score. Changes the graph without new evidence |
| Edge add or drop | free | Only meaningful via reweight across `tau`; recorded as its own operator for auditability |
| Sub-question split | **one evidence run** | One sub-question becomes two more specific ones |
| Sub-question merge | free | Two become one. Evidence for both is retained and both memo rows attach to the merged vertex |
| Sub-question add | **one evidence run** | A genuinely new question |
| Sub-question drop | free | Removes a vertex. Its memo row is retained, not deleted |
| Method change | **one evidence run** | New `(sub_question, method)` memo entry |
| Recombination | free, plus evidence for any imported sub-question not already gathered | Merge two parents' sub-question sets and average their relation matrices, with conflicts resolved toward the higher-scoring parent |

Only four operators cost anything, and all four are the same thing:
asking a question nobody has asked yet. That is the budget, and it is
capped per epoch (see Cost).

Note what is absent. There is no Gene Repair, no crossover over bit
strings, and no mutation over cut bits. Those belong to the cut search,
which no longer exists at this layer.

## Scoring a framing

Deterministic, in Rust, with no model in the loop.

**Step 1: partition the framing.** Solve for the best partition under
the pre-registered objective. Below a stated vertex count, verify the
solver by exhaustive enumeration over connected partitions; above it,
run the deterministic solver from multiple fixed starts; above the size
where that is affordable, fall back to `erbga`.

**Step 2: score the framing** on three terms, each of which measures
something the design actually needs:

- **Separation**, the objective value of the best partition. This says
  the framing carves into coherent, separable lines.
- **Evidential independence**, the complement of source overlap between
  components. Each component reports the set of memo row ids its
  sub-questions rest on, and overlap is Jaccard over those sets. This is
  the term that means what "independent lines" is supposed to mean.
  Structural separation does not imply evidential independence, and
  without this term nothing in the system measures the difference.
- **Grounding**, the fraction of sub-questions whose relation scores
  carry a stated reason and whose evidence was gathered rather than
  reused below a stated similarity. Penalizes framings that assert
  structure the model did not justify.

**A framing whose component source-overlap exceeds a pre-registered
ceiling is invalid, not merely low-scoring.** Components resting on the
same evidence are not independent lines regardless of how cleanly the
graph separates, and a design that sells corroboration must refuse to
call them independent.

### The objective, and why not plain modularity

The partition objective is the **Constant Potts Model** with a
pre-registered resolution `gamma`.

Modularity is the obvious choice and it is wrong here, for a reason that
is specific to scale rather than to the analogy. Modularity cannot
resolve communities holding fewer than about `sqrt(2m)` internal edges.
On Karate, `m = 78`, the threshold is about 12.5, and the optimal
communities carry 15 to 20 internal edges, so the limit does not bind.
On a realistic question graph of 20 sub-questions and 40 relations, the
threshold is about 8.9, while a line of inquiry of four sub-questions
holds at most six internal edges. Every line of inquiry a question
decomposition would want to find sits below the limit. Modularity would
systematically merge exactly the distinctions this capability exists to
draw, and it would impose a size-dependent cap on the number of lines,
which is precisely the freedom the representation is supposed to buy.

CPM has no resolution limit. `gamma` sets the density scale at which a
group counts as a line, so it is derived from the target line size and
pre-registered, because it directly controls how many lines you get.

`erbga`'s pluggable-objective design is what makes this substitution
possible, and it is the contribution of that work most load-bearing
here. Modularity remains available in `erbga` for reproducing the
paper's benchmarks.

## The confirmatory stage

After the search terminates, and only then:

1. Take each island's highest-scoring valid framing and its partition.
2. Run **`n` independent synthesis passes** over that partition's
   evidence, `n` pre-registered, each producing a `metric_value` under
   the pre-registered metric.
3. Report the **mean and spread** for that island. The threshold is
   applied to the mean, never to the best pass.
4. Record every pass individually, with its full prompt, model id,
   temperature, seed, and raw completion.

A synthesis pass may return `{"metric_value": null, "reason": "..."}`
when the evidence does not support a single number. This is a recorded
result, not a parse failure, and it counts as **non-passing**. Evidence
that genuinely conflicts is more likely to produce hedged output than
evidence that agrees, so treating unquantifiable results as exempt would
systematically excuse the runs most likely to be refutations.

The output of `evolve` is a distribution and its dissent. It is not a
number.

## Islands and framing diversity

Islands evolve independently, each starting from its own framing pass.

**This is framing diversity. It is not corroboration, and the spec says
so wherever the result is reported.** All islands share one model, one
prompt scaffold, one tool set, and one memo table. Agreement between
them carries almost no evidential weight against a bias the model
itself holds, because such a bias produces agreement whether the claim
is true or false. Islands vary the stage least likely to carry the
model's bias, while evidence interpretation and synthesis, where bias
actually lives, is fully shared.

Calling that corroboration would manufacture exactly the false
confidence this product exists to prevent. Every checkpoint, artifact,
and generated summary states the shared generative source explicitly.

Real independence requires diversifying the generator, which is
achievable because `ZORP_BASE_URL` is already per-config, so islands
could run different models or providers. That is out of scope for v1 and
is the single most valuable extension.

## The memo table and evidence provenance

Sub-questions are subproblems and evidence is the memoized result, so
the population pays for each distinct question once.

- **Keyed** on `(sub_question, method)`.
- **Looked up semantically**, since a natural-language key would rarely
  match exactly.
- **Two separate thresholds.** A high `reuse_floor` decides whether an
  existing entry actually answers the question being asked. A separate
  `novelty_floor` decides whether a proposed sub-question counts as new.
  These jobs pull in opposite directions and a single threshold cannot
  serve both.
- **Read and written only by the controller.** Workers never touch it.
- **Every hit records** the floor, the similarity score, the originating
  sub-question, and the epoch, so reuse is auditable.

**Memo reuse and evidential independence are the same dial.** Every
cross-island hit is a unit of shared evidence, which is cost saved and
independence lost. The design resolves this rather than claiming both:

- Within an island, reuse is unrestricted.
- **Across islands, reuse is forbidden for evidence feeding the
  confirmatory stage.** Islands pay separately for the evidence their
  reported result rests on. This is why island count is small, 3 to 5,
  rather than the 5 to 25 of the source work.
- Across islands, reuse is permitted for evidence feeding search-time
  scoring only, where it affects ranking but not any reported claim.

Every evidence record carries the set of islands that consumed it, so
convergence backed by shared sources is distinguishable from
convergence backed by separate ones.

## Cost

Stated honestly, since the first draft's bound was wrong.

```
evidence_runs = I * M * V_0 * (1 + g)^E
```

for `I` islands, `M` methods in the closed enum, `V_0` initial
sub-questions per island, per-epoch fractional growth `g`, and `E`
epochs. Cross-island dedup does not reduce it for confirmatory evidence,
by the rule above.

What genuinely drops out is **population size and generation count**.
Framing scoring is deterministic arithmetic over evidence that already
exists, so a large population searched for many generations costs
nothing beyond CPU. That asymmetry is the reason this approach is
affordable at all, and it is the one part of the first draft's cost
argument that survived review.

What does not drop out is islands, methods, and epochs. To keep the
product bounded:

- `M` is a **closed enum** fixed in the pre-registration.
- New `(sub_question, method)` pairs per epoch are **capped** by a
  pre-registered constant.
- `I` is small and pre-registered.

Confirmatory cost is `I * n` synthesis passes, once per run.

## Controller and workers

**The controller is code.** Framing scoring, partition solving, source
overlap, selection, the confirmatory comparison, and termination are all
deterministic Rust. The model is used for framing construction, for the
mutation and recombination operators, and for confirmatory synthesis.

Confirmatory synthesis is a model output that determines the reported
result, so it is not deterministic and the spec does not pretend
otherwise. Replayability is bought instead by recording the full prompt,
model id, temperature, seed, and raw completion for every pass, and by
requiring `n > 1` passes with reported spread.

**Every random draw in the system takes a recorded seed**, at the run
level and the island level, without which none of the above is
replayable.

**Workers gather evidence and nothing else.** They receive a
sub-question, a method, and the memo hits the controller resolved on
their behalf, and return a structured result. They hold no store handle.

The existing `SubagentPool` is **not** sufficient, and the first draft
was wrong to treat it as the execution substrate. What exists at
`zorp-agent/src/tools/subagent.rs` is a handle registry plus a
`Tool`-shaped spawner: `SubagentPool` never spawns, `running_count` only
counts, and `SpawnSubagent::run` **returns an error** at
`MAX_CONCURRENT_SUBAGENTS = 8` rather than queueing. What must be built
is listed under Prerequisites.

## The epoch loop

An **epoch** is one round of expensive work. The framing search runs to
convergence inside it, unattended and free.

1. **Frame.** On the first epoch, each island builds its own question
   graph, emitting per-pair "bears on" scores with reasons and marking
   cross-cutting premises.
2. **Gather.** Every `(sub_question, method)` pair without a memo entry
   gets a worker run, subject to the per-epoch cap. This is the only
   expensive step.
3. **Search.** Each island evolves its framing population against the
   deterministic score. No model calls, no evidence gathering, no
   pre-registered metric.
4. **Checkpoint.** One prompt per epoch.
5. **Grow.** Mutation and recombination propose the next epoch's
   framings, including the sub-question splits, additions, and method
   changes that make the next epoch cost anything.

After the final epoch, the confirmatory stage runs once.

## Termination

**Primary: marginal improvement per evidence run.** The run stops when
additional evidence stops improving the best framing score by more than
a pre-registered amount per evidence run. This measures what the search
is actually for.

**Gate: a run may only terminate on convergence if the population has
not collapsed.** A collapsed population also stops improving, and the
two are indistinguishable from the improvement signal alone. A run that
converges with diversity below the floor terminates as **Inconclusive**,
which is a result, not a pass.

**Backstops:** a pre-registered maximum epoch count and a pre-registered
maximum total evidence runs.

Memo hit rate is **recorded and reported but not used to terminate.**
It reduces algebraically to `1 / (1 + g)` where `g` is graph growth, so
under constant absolute growth it crosses any ceiling within a fixed
number of epochs regardless of what the search finds, and under
proportional growth it is constant forever and fires at epoch one or
never. It measures the denominator, not discovery.

## Pre-registration

The rule from the first draft, "pre-register anything that could bias
which answer wins, merely record anything that only affects how hard you
searched," does not survive. When the reported result depends on a
search, search effort **is** answer selection. Island count changes the
quorum. Mutation rates determine which sub-questions are ever asked.
The stopping rule is optional stopping.

**So everything that is an input to the run is pre-registered.** There
is no recorded-only tier. Over-registering costs nothing, because a
value can always be widened in the next run, on the record.

Written into `prereg.md` and covered by the existing SHA-256 hash and
git commit:

- Metric name, threshold, and direction, as today.
- The partition objective and its resolution `gamma`.
- The relation threshold `tau`.
- The closed method enum.
- The framing score's three term weights.
- The source-overlap validity ceiling.
- `reuse_floor` and `novelty_floor`.
- Confirmatory `n`.
- The breach quorum.
- Island count, population size, generation count, mutation and
  recombination rates, per-epoch evidence cap, termination parameters,
  diversity floor.
- All RNG seeds.

## Elimination and track death

Individuals are not eliminated on the pre-registered metric, because the
metric is never evaluated during the search. Framings die by scoring
poorly on the deterministic framing score, or by being invalid on the
source-overlap ceiling. Both reasons are recorded.

**The track dies** when the fraction of islands whose confirmatory
**mean** breaches the threshold exceeds a pre-registered
`breach_quorum`. Unanimity is not acceptable: with 24 of 25 islands
refuting and one passing, a unanimity rule keeps the track Active and
propagates only the winner, which is exactly the failure the 2026-08-14
kill-threshold decision was written to eliminate.

Every island's confirmatory mean is recorded and surfaced, whether or
not it breaches, so a one-of-five survival is visible rather than hidden
behind a maximum.

The `AutoApprove` exemption attaches to track death, matching the
existing decision.

## Checkpoints

One checkpoint per epoch, showing each island's best framing, its
partition, its component source-overlap, the best score and its trend,
new sub-questions asked this epoch, memo hit rate, cumulative evidence
runs, and cost.

Cross-island divergence is computed **structurally**, over recorded
numbers, not by a model judging agreement. During the search it is the
distance between island partitions; at the confirmatory stage it is the
spread of island means. This keeps the controller free of a judgmental
model call.

Interrupts fire on population collapse, on the per-epoch evidence cap
being hit, and on the total evidence backstop approaching.

## What co-write gets

The first draft claimed "nothing in co-write changes." That is true and
useless: `co_write/mod.rs` emits one flat line per metric with an opaque
experiment id, and the schema has no column that can express which
island, epoch, or framing a row came from. Given hundreds of rows it
would receive undifferentiated scalar soup and could not distinguish
five islands agreeing from one island measured five times.

Worse, co-write's grounding guarantee was designed for a handful of
unselected attempts. Under a search it would silently become "only the
winners reach the model."

So `evolve` requires changes to storage and to co-write, listed under
Prerequisites. What co-write must receive is: every island's
confirmatory distribution, the framing and partition each rests on,
component source-overlap, and the explicitly labelled dissenting and
indeterminate results. Conflicting evidence it is required to surface
has to be handed to it as data, not left to be inferred from a list of
floats.

## Prerequisites

These are not integration work. They are missing substrate, and the
first draft assumed all four existed.

1. **A LanceDB read path.** `zorp-track`'s `Library` exposes exactly
   `open`, `table_names`, and `insert_source`, with private fields and
   no query API of any kind. The memo table needs a new table, a keyed
   insert carrying method, epoch, score, and origin, and a nearest
   neighbour query returning distances. This is new `zorp-track` public
   API. Note that embedding goes through `zorp-agent::embed_texts`,
   a blocking HTTP call, so lookups must be resolved once per epoch into
   a dense in-memory structure rather than queried inside any loop.
2. **Schema columns.** `experiments` carries no island, epoch, or role.
   Add them, with the `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` idiom
   already used in `schema.rs`, plus a record for confirmatory passes
   that stores the substantive claim text and not only a float.
3. **Worker dispatch.** A Rust-callable entry point that does not go
   through the `Tool` trait, a bounded queue that waits rather than
   erroring at the cap, a configurable cap, and a structured worker
   result contract with a parser.
4. **Cost accounting.** The spec's checkpoint shows cost and its
   backstop counts evidence runs. Neither exists anywhere in the
   codebase today.

A fifth item is a decision rather than code: `write_prereg` returns
`AlreadyRegistered` if any prereg row exists, and the row id is
hardcoded to one per track, so `evolve` cannot currently pre-register on
a track that has already run `investigate`. Either `evolve` requires a
fresh track, or prereg gains a version marker and a superseding row.
Adding fields to `prereg.md` does not break verification of existing
files, since hashing is over raw bytes and the parser tolerates unknown
and missing lines.

## Error handling

- Track already `Killed`: refuse before building anything.
- A framing with fewer than a pre-registered minimum number of
  sub-questions, or no relations above `tau`: that island fails and is
  recorded as failed. The run continues if any island survives.
- A framing whose component source-overlap exceeds the ceiling: invalid,
  recorded with its overlap, not scored.
- A worker returns an unparseable result: no memo entry for that pair,
  recorded as a gather failure. Repeated gather failure on one
  sub-question is surfaced at the checkpoint as a finding, since a
  question whose evidence resists gathering is informative, not
  defective.
- A confirmatory pass fails to parse: recorded as non-passing, counted
  in the quorum.
- Every island fails: a distinct error, not a silent empty result.

## Testing

**The partition solver, without a model.** Below the stated vertex
count, exhaustive enumeration over connected partitions establishes the
true optimum, and the solver must find it. This is a real correctness
proof at question-graph scale and needs no benchmark data at all.

**`erbga`, against all four of the source's benchmarks**, including the
two it failed. Expected values from thesis Table 3:

| Network | E/V | BKR | ERBGA |
|---|---|---|---|
| Karate | 2.3 | 0.420 | 0.420 |
| Dolphin | 2.6 | 0.529 | 0.445 |
| Political Books | 4.2 | 0.527 | 0.256 |
| Football | 5.3 | 0.605 | 0.073 |

Gating only on the two that succeeded would be selecting the benchmark
after seeing which ones the method passes, which is the same move the
pre-registration section forbids. Reproducing 0.073 on Football is a
**stronger** correctness signal than reproducing 0.420 on Karate,
because far fewer wrong implementations produce it.

The gate must specify island count, generation count, time budget, RNG
seed, and a tolerance, because the source's results are stochastic:
4 of 25 islands reached 0.420 on Karate and the rest landed near 0.397,
so a five-island run of a correct implementation fails roughly 42% of
the time. The source's numbers come from runs capped at 48 hours, which
is not a CI budget, so the gate reproduces the protocol at reduced scale
with a stated tolerance and the full-scale run stays manual.

Three paper-versus-thesis discrepancies must be resolved before these
are treated as targets:

| Parameter | Paper | Thesis |
|---|---|---|
| Random Population Rate | 0.25 | 0.85 |
| Tournament pool size | 3 | 7 |
| Dolphin result | 0.465 | 0.445 |

The thesis text supports the high population rate ("tweaking the Random
Population Rate to be closer to 1 resulted in the improvement in the
quality of the initial set of chromosomes"), which suggests 0.25 is a
paper typo. Resolve by running both and reporting both.

**Representation, without going through an objective.** ERBGA removes
the `k!` label-permutation redundancy of label-based encodings, which is
true and worth testing directly. It does **not** give each partition a
unique encoding: on a triangle, removing nothing and removing one edge
both leave a single component, and in general the collapse factor is at
least `2^(|E| - |V| + c)`. Test the true claim, that distinct partitions
never share an encoding, and separately measure and report the
genotype-to-phenotype collapse ratio per graph.

**Framing operators.** Property tests: merge followed by split
round-trips the sub-question set, recombination never invents a
sub-question absent from both parents, dropping a vertex never deletes
its memo row, and cross-cutting premises never appear in any component.

**Source overlap.** Unit tests that two components resting on the same
memo row are detected, and that a framing exceeding the ceiling is
rejected rather than scored.

**Confirmatory stage.** Stub-model integration tests covering: `n`
passes recorded individually, threshold applied to the mean and not the
best, a `null` metric counted as non-passing, quorum reached and not
reached, and `AutoApprove` bypassed only on track death.

## Implementation order

Each stage is independently useful and de-risks the next.

1. **`erbga` and the partition solver, no zorp integration.** Done when
   the exhaustive-enumeration check passes at question scale and all
   four benchmark values reproduce within tolerance.
2. **Prerequisites.** The LanceDB read path, schema columns, worker
   dispatch, cost accounting. Boring, and nothing above works without
   it.
3. **Framing search.** Genome, operators, scoring, source overlap.
   Testable with a stub model.
4. **Epoch loop and gathering.**
5. **Confirmatory stage, quorum, checkpoints.** Last, because it is the
   layer that most needs the ones below it to be trustworthy.

Stage 1 can fail on its own terms and is worth completing before
committing to the rest.

## Out of scope

- **Per-island model diversity.** The only change that would let a
  corroboration claim survive, and the most valuable extension. Out of
  v1 because it needs per-island provider configuration and cost
  accounting.
- **Cross-island migration.** Framings are portable in principle, unlike
  the first draft's bit strings, but migration reintroduces exactly the
  shared-source problem the island reuse rule exists to prevent.
- **Evolving the objective, `gamma`, or `tau`.** All pre-registered
  precisely so they cannot move during a run.
- **Replacing `investigate`.** One attempt against one metric remains
  right when a question is not worth a search.
- **ERBGA's 3D cuboid memory layout.** Deferred. Bit packing itself is
  not: `Chromosome` is opaque with `get`, `set`, `flip`, and
  `count_ones`, so operators never see the backing store and the cuboid
  stays a one-file change. Packing is justified by memory traffic in the
  GA's hot loop, not by the thesis's 85% figure, which measures a
  different comparison (one bit per edge against one integer per vertex)
  and is banked by adopting the edge representation at all.

## What review changed

Four adversarial reviews attacked the first draft on cost, epistemics,
fidelity to the source work, and fit with the codebase. The findings
that forced the rewrite:

- **Selecting on the pre-registered metric turned the threshold into a
  breeding objective.** Fixed by removing the metric from selection
  entirely and adding the confirmatory stage.
- **The compute was pointed at the wrong half.** The first draft spent
  roughly 1.25M evaluations per island perfecting the cut of a graph
  produced by one model call and never revisited, with no operator able
  to change a relation between existing sub-questions. Fixed by
  inverting: evolve framings, solve cuts.
- **Modularity cannot resolve communities at question-graph scale.**
  The resolution limit does not bind on Karate and binds on every
  realistic question graph. Fixed by using CPM with pre-registered
  `gamma`.
- **The corroboration claim was false.** Islands share one model, and
  the first draft terminated on memo saturation, which is the moment
  inter-island independence is lowest. Fixed by renaming to framing
  diversity, forbidding cross-island reuse for confirmatory evidence,
  measuring source overlap, and changing the termination signal.
- **Track death by unanimity voided the guarantee it was meant to
  preserve.** Fixed with a pre-registered quorum.
- **`Coverage` in the first draft's proxy was identically 1.0**
  throughout the search, since gathering precedes evolution and
  crossover cannot invent an ungathered pair. Removed.
- **The vertex set was not invariant** across islands or epochs, so the
  cost bound was wrong and elites had no defined migration across a
  chromosome length change. Dissolved by the inversion.
- **Gene Repair's own results refute the dense-graph claim.** ERBGA
  accuracy degrades monotonically with density across all four
  benchmarks with Gene Repair enabled. The operator is gone from this
  layer, and no dense-question claim is made.
- **Four pieces of assumed substrate do not exist.** Now Prerequisites.

## Review record

Eight adversarial reviewers across two rounds, each given one lens and
told to refute rather than review. Findings not already covered above,
kept because they will apply to whatever replaces this design.

### Confirmed against the codebase

- `zorp-track`'s `Library` exposes exactly `open`, `table_names`, and
  `insert_source`, with private fields. There is **no read path of any
  kind**, so the memo table's storage layer is unbuilt, not merely
  unintegrated.
- `experiments` carries no island, epoch, or role column, and
  `co_write/mod.rs` emits one flat line per metric with an opaque id. So
  "nothing in co-write changes" is true and useless: it would receive
  undifferentiated scalars and could not tell five islands agreeing from
  one island measured five times.
- `SubagentPool` never spawns. `running_count` only counts, and
  `SpawnSubagent::run` **returns an error** at
  `MAX_CONCURRENT_SUBAGENTS = 8` rather than queueing. Worker dispatch
  must be built, not reused.
- `TrackStatus::from_str` has a catch-all `_ => Active`, so the
  `Inconclusive` status this spec invents would round-trip as **Active**.
  `ExperimentStatus::from_str` has the identical `_ => Planned` bug. For
  a product whose pitch is that a non-result must not look like a live
  result, that default is the worst possible one, and it is worth fixing
  regardless of what happens to this design.
- `write_prereg` returns `AlreadyRegistered` if any prereg row exists and
  the row id is hardcoded to one per track, so `evolve` could not
  pre-register on a track that already ran `investigate`. Adding fields
  to `prereg.md` does not break verification of existing files (hashing
  is over raw bytes, the parser tolerates unknown lines), but there is no
  writer that can emit them.
- Neither `temperature` nor `seed` exists anywhere in the model layer, so
  the per-pass replayability this spec requires is unimplementable as
  written. Anthropic has no seed parameter at all, so replay is
  provider-conditional and must not be claimed uniformly.
- `MetricValue` has no null variant, and both readers end in a catch-all
  that would error on one, so a recorded non-result becomes a read
  failure.
- No cost accounting exists in any form, so two of three interrupt
  triggers and one checkpoint field are unimplementable.

### Methodological findings that outlive this design

- **The winner's curse is attenuated, not eliminated, by removing the
  metric from selection.** Selecting `argmax S` over `N` framings and
  then measuring `M` imports bias proportional to `corr(S, M)`. But a
  framing score uncorrelated with answer quality is a useless search
  objective, so the design cannot claim both that the search improves the
  answer and that the reported number is unbiased.
- **The confirmatory spread measures synthesis jitter only.** The `n`
  passes share evidence, partition, prompt, and model, so evidence bias
  and model bias are constant across them. Larger `n` tightens the
  interval around the wrong centre. It should be named "synthesis spread"
  and stated as a lower bound on total uncertainty.
- **A per-epoch checkpoint is a human optional-stopping channel** that no
  pre-registered parameter can cover. Either the checkpoint is
  informational with no reject path, or a rejected run must be marked
  Abandoned and disclosed in every later artifact on that track.
- **Cross-cutting premises are a common cause.** Lines that share a
  premise are jointly wrong if it is wrong, which is the exact failure
  corroboration is meant to rule out. Any design that removes the
  coupling vertices and then measures independence on the residue is
  overclaiming.
- **Source overlap must be measured over source identifiers**, not memo
  rows. Two components citing the same three papers have row-overlap zero
  and source-overlap one. The worker result contract needs a citation
  field for this to be computable at all.

### Empirical results from Stage 1

Building `erbga` produced two facts relevant to any successor design.

Its accuracy against best-known degrades with graph density: 100% on
Karate, 99% on Dolphin, 95% on Political Books, 63% on Football. A dense
question graph, where everything bears on everything, is the hard case
for this family of method.

An exact clique-partitioning ILP solves modularity to proven optimality
in about 0.2 seconds at `V = 20` and about 5 seconds at `V = 50`. At the
scale a question graph occupies, the partition is not a search problem.
