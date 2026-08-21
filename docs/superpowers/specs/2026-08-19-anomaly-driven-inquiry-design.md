# Aryabhatta: anomaly-driven inquiry and the epistemic record

**Date:** 2026-08-19
**Status:** proposed. The subsystem as a whole is not approved. Three
decisions inside it are taken and recorded in `docs/DECISIONS.md`
(2026-08-19): the name, the admission registry, and the integration of
`erbga` as the search layer's backend.

## Purpose

Every capability zorp has today starts with a question a human supplied.
`validate` scores a question someone typed. `investigate` pursues it.
`co-write` and `deliver` turn the result into an artifact. The question
itself is always an input.

This spec covers the layer before that: what zorp would have to record in
order for a question to be *derived from its own history* rather than
supplied. It does not build an autonomous scientist. It builds the record
such a thing would need, plus the two cheapest readers of that record,
and it makes the measurement that says whether the idea works at all.

The subsystem is called **aryabhatta**. The name is not decoration.
Aryabhata's break with the astronomy around him was that he explained
eclipses as shadow geometry computed from recorded observation, where the
account he displaced reached for a demon swallowing the sun. He replaced
an interpretation with an arithmetic and kept the interpreting for
afterwards. That is the split this subsystem is built on.

Aryabhatta is also the home for discovery ideas that do not exist yet.
New ideas about how zorp derives its own questions land here rather than
becoming capabilities of their own. What varies is an idea's state in the
registry below, not whether it belongs. One name holds the work together
without welding the parts to each other, which matters because step 3 of
the implementation order is a kill switch and a monolith cannot be
killed.

The scope is deliberately narrow. See "Out of scope" for the seven things
this defers.

## The governing principle

**Detection is code. The model only interprets.**

This is not a style preference, it is the property that keeps the
subsystem honest. A model asked to *notice* that something is anomalous
will notice something every time, because fluent speculation is free. A
model asked to *explain a deviation that arithmetic already found* is
constrained by a number it did not choose.

`critique` already works this way by explicit decision: the audit is
code, and the model only inventories claims. Every component here
inherits that split.

## Background: what zorp records today

`zorp-track` has seven tables. The relevant shape is:

- `metrics(experiment_id, metric_key, value_*)` records what came out.
- `experiments(id, track_id, prereg_id, status, timestamps)` records that
  a run happened.
- `preregistrations(metric_name, kill_threshold, threshold_direction, ...)`
  records the commitment, git-pinned and hash-checked.
- `checkpoints(kind, status, prompt_shown, decision_notes)` records where
  a human was asked something.

**Nothing records the conditions a run was performed under.** There is no
table saying an experiment used a particular harness, context length, or
matching rule. Outputs are recorded, inputs are not.

Separately, `SqliteRecorder` already persists the session transcript and
file changes, and the `Renderer` trait already emits a live `tool` and
`verify` stream. That observation stream exists and is durable. It does
not reach `zorp-track`.

`verify.rs` is the closest thing to an expectation engine already
present. It runs commands as a completion gate, holds a `VerifyReport`,
and has a method named `observation()`. The prediction is implicit and
binary: these will pass. The outcome is recorded. The interesting part is
flattened to one bit.

## Design

### 1. Conditions

A new table recording the inputs of an experiment, with the same value
encoding `metrics` already uses so the two are symmetric.

```sql
CREATE TABLE IF NOT EXISTS conditions (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    condition_key TEXT NOT NULL,
    value_type TEXT NOT NULL,
    value_number DOUBLE,
    value_string TEXT,
    value_bool BOOLEAN,
    recorded_at BIGINT NOT NULL,
    seq BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_conditions_experiment_id
    ON conditions(experiment_id);
```

Conditions make three things possible that are impossible today: knowing
what a prediction was a prediction *about*, replaying a run to test
whether a deviation reproduces, and asking which variable has never been
varied.

### 2. Expectations

A quantitative forecast about one metric of one experiment, recorded
before the experiment produces that metric.

```sql
CREATE TABLE IF NOT EXISTS expectations (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    expected_value DOUBLE NOT NULL,
    interval_low DOUBLE NOT NULL,
    interval_high DOUBLE NOT NULL,
    confidence DOUBLE NOT NULL,
    assumptions TEXT,
    recorded_at BIGINT NOT NULL,
    seq BIGINT NOT NULL
);
```

`confidence` is the stated coverage of the interval, for example 0.80.
`assumptions` is a JSON array of free text, recorded but never used as a
detector input.

An expectation is not a pre-registration. A pre-registration is the
scientific commitment for a whole track, git-committed and hash-verified.
An expectation is a per-experiment forecast, and there will be many.

**The integrity rule:** inserting an expectation must be refused if any
metric with that `metric_key` already exists for that `experiment_id`.
This is the entire anti-rationalization guarantee. Without it a
"prediction" is a postdiction and every number downstream is theatre.

This guarantee is procedural, not cryptographic. The DB constraint stops
the ordinary path. It does not stop someone editing the database
directly, the way the prereg file hash does. That is an accepted limit
for v0, on the grounds that the calibration report makes cheating
self-defeating: backdated expectations produce suspiciously perfect
coverage, which is itself visible. Recorded as an open question below.

### 3. Surprise

For an expectation with a recorded outcome:

```
sigma    = (interval_high - interval_low) / (2 * z(confidence))
surprise = |observed - expected_value| / sigma
```

where `z(0.80) = 1.2816` for a central interval.

**Surprise is arithmetic on a sigma the forecaster asserted.** It has no
meaning until the calibration report (§6) says those intervals have real
coverage. The system must therefore present surprise as advisory until
calibration is established, and the spec treats any design that skips
this as broken.

### 4. The re-run gate

An anomaly is not admitted to the ledger on surprise alone. The
experiment is repeated with identical recorded conditions, and the result
classified:

| Outcome | Meaning | Admit |
|---|---|---|
| `reproduced` | repeat also falls outside the expectation interval, on the same side | yes |
| `transient` | repeat falls inside the expectation interval | no, counted |
| `volatile` | repeats fall outside on opposite sides, or their spread exceeds the interval width | no, counted |
| `unverifiable` | conditions could not be replayed | yes, flagged |

Defining `reproduced` against the already-recorded expectation interval
is deliberate. It introduces no new tolerance parameter, and the
forecaster does not get to widen the definition of success after seeing
the result.

This gate does two jobs with one mechanism.

It separates defects and phenomena from noise, which matters because in a
software environment most prediction error is a truncated context, a
changed default, or a mis-parsed result file rather than a discovery.

It also separates reducible uncertainty from irreducible. Reward
prediction error directly and a system reliably finds the things that are
inherently random, which is the noisy TV problem well known from
intrinsic-motivation reinforcement learning. In zorp's environment the
noisy TVs are sampling above temperature zero, flaky tests, network
latency, and search results that differ between calls. A flaky test
generates a clean four-sigma anomaly on demand, forever. `volatile` is
the classification that throws those away.

Rejected anomalies are counted rather than discarded silently, so the
noisy TV rate is itself measurable.

This gate is an empirical separator of aleatoric from epistemic
uncertainty. That is a known mitigation family for the noisy TV rather
than an invention here, and the gate should be presented as a member of
it rather than as ad hoc. Representative work: aleatoric uncertainty
estimation for curiosity (arXiv 2102.04399) and learning-progress
monitoring (arXiv 2509.25438). RND's trick of predicting features of the
current state rather than the next belongs to the same family.

### 5. The anomaly ledger

```sql
CREATE TABLE IF NOT EXISTS anomalies (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    experiment_id TEXT NOT NULL,
    expectation_id TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    expected_value DOUBLE NOT NULL,
    interval_low DOUBLE NOT NULL,
    interval_high DOUBLE NOT NULL,
    observed_value DOUBLE NOT NULL,
    surprise_sigma DOUBLE NOT NULL,
    gate_outcome TEXT NOT NULL,
    status TEXT NOT NULL,
    explanation TEXT,
    created_at BIGINT NOT NULL,
    seq BIGINT NOT NULL
);
```

`status` is one of `unexplained`, `explained`, `superseded`. Rows are
never deleted. Status changes are explicit acts, never a side effect of
retrieval or of time passing.

`explanation` is model-authored text. It is stored and displayed. It is
never read by any detector, for the reason in §7.

### 6. The calibration report

A pure read over expectations that have outcomes:

- empirical coverage per stated confidence band
- the calibration curve, stated confidence against observed coverage
- mean interval width, so a forecaster cannot buy coverage with
  uselessly wide intervals
- n

This report is the deliverable that decides whether the rest of the
architecture is worth building. If 80% intervals contain the truth 45% of
the time, surprise is noise and every component above it is generating
confident fiction. That result is worth knowing early and is worth
publishing.

Threshold for trusting surprise: coverage within tolerance of the stated
band, over n of at least 50. The specific tolerance is deliberately left
to the implementation plan, since it should be set from the first
observed curve rather than guessed here.

**This is the endogenous case, and the adjacent evidence predicts it
fails.** The general question is not open. FermiEval (arXiv 2510.26995)
reports LLM nominal 99% intervals covering the true answer 65% of the
time, with a "perception tunnel" account: models sample from a truncated
region of their own inferred distribution and neglect the tails.
QuantSightBench (arXiv 2604.15859) measures empirical coverage against
stated confidence directly, which is precisely this quantity. Both cover
static or external questions, and QuantSightBench states outright that
its tasks are not self-conducted experiments. What is untouched is the
case where the agent predicts the outcome of an experiment it chose and
is about to run.

That is not merely a domain transfer. In the endogenous case the quantity
being measured is manipulable by the thing being measured, which is open
question 5 below. It is the sharpest part of this design.

It also means the step 3 stop sign is more likely to fire than not. 65%
coverage at a nominal 99% is a strong prior on the answer. Firing early
is cheap, which is exactly why the stop sits at step 3 rather than after
the ledger is built.

### 7. Boredom detectors

Pure SQL over the track database. Each emits a finding containing the
invariant, the supporting count, and the query that produced it.

Each detector takes a minimum support `n` below which it stays silent,
since "this varied in none of two runs" is not evidence of anything.
Starting values, to be revised once real distributions exist: 8 for
`checkpoint_habituation`, which accrues fastest, and 12 for the three
that count experiments or preregistrations.

Three work against data zorp records today:

**`checkpoint_habituation`** groups `checkpoints` by `kind` and finds
kinds where every resolution was identical over at least n occurrences.
The finding reads: this checkpoint has been presented 31 times and
resolved the same way 31 times, so it is a formality rather than a
decision. This is the detector that accrues fastest, since checkpoints
recur many times per investigation.

**`metric_monoculture`** finds tracks whose experiments record only one
distinct `metric_key` across at least n experiments. Every run measured
one thing, and nothing has asked what that measure hides.

**`threshold_direction_monoculture`** finds projects where every
`preregistrations.threshold_direction` is identical. Every hypothesis
registered can be falsified by a number moving one way, and none by a
distribution changing shape.

One requires §1:

**`invariant_condition`** finds `condition_key` values with exactly one
distinct value across at least n experiments.

The model's only role is to turn a finding into a candidate question. It
receives the invariant and the counts, and it may not add invariants of
its own.

**Model-authored text is never a detector input.** Candidate questions
and anomaly explanations live in columns no detector reads. Without this
rule the agent's own speculation becomes tomorrow's observation, and the
system develops the ability to be surprised by itself.

### 8. Where findings go

A candidate question is handed to `validate`, which already scores
redundancy and feasibility and already exists. Boredom is a question
generator, and zorp already has the question judge. This is the missing
input to a built capability, not a new pipeline.

Nothing in this subsystem acts autonomously on a finding. It proposes.

### 9. The search layer

Two readers here need to partition a graph: the confounded condition
bundles below, and the anomaly families that group a ledger into related
deviations. Rather than two implementations, community detection enters
the subsystem once, as one interface with two backends chosen by graph
size.

`erbga` is that interface's backend for large graphs. It is a
zero-dependency implementation of Rao, Janikow, Bhatia and Climer (MWAIS
2018), already in this repo and validated against that work's four
benchmarks. Wiring it in reverses part of the 2026-08-15 decision, which
shipped it off any critical path, so it carries its own decision entry.

**Two callers, on opposite sides of the gate.**

| Caller | Vertices | An edge means | Lane | Lands at |
|---|---|---|---|---|
| Confounded conditions | `condition_key`s | never varied independently across at least n experiments | ungated | step 1 |
| Anomaly families | ledger rows | shared metric, same deviation sign, overlapping conditions | gated | step 5 |

The first caller is why this is not premature. Bundling confounded
conditions reads the `conditions` table from section 1. It never touches
the anomaly ledger and never sits behind the calibration gate. It is
aliasing in the design of experiments sense: when two conditions never
move independently, no effect can be attributed to either one alone. It
also strengthens `invariant_condition`, which sees only single keys with
one distinct value and is blind to a pair that always moves together
while both vary.

**θ is swept, never chosen.** `erbga` takes unweighted edges, so a
continuous similarity has to become a binary one. The service does not
price the threshold, it refuses to pick one. The partition is computed
across a range of θ, and only communities surviving a contiguous band are
kept. A family visible only at θ = 0.43 is an artifact of the cutoff. One
stable from 0.3 to 0.7 is structure. The swept range and the surviving
band go into the record. Nobody chooses θ, so nobody can choose it to
reach a wanted answer. Without this the design repeats the defect that
sank `evolve`, where the score was maximized by an undifferentiated blob
because edge addition was priced free.

**The backend is chosen by |V|, and the overlap is free testing.** Below
the crossover an exact solver returns a proven-optimal partition, which
is the honest answer to the 2026-08-15 finding that at `V = 20` a
clique-partitioning ILP finishes in about 0.2 seconds. Above it, `erbga`
searches. In the band where both can run, the exact result is a
continuous regression check on the genetic algorithm against proven
optimality. The benchmark suite cannot provide that, because it holds
four fixed networks.

**The seed is part of the record.** `erbga` is stochastic and seeded,
with an in-crate RNG chosen so sequences stay stable across platforms and
releases. The seed, island count and parameter set are recorded next to
the partition, so a clustering is reproducible like every other recorded
result here.

**What it may not do is unchanged.** The similarity graph and the
co-variation graph are built from code-visible columns only:
`metric_key`, deviation sign, `gate_outcome`, conditions, timing. Never
`anomalies.explanation`. Integrity rule 5 binds the search layer exactly
as it binds the detectors.

#### What transfers from erbga, and what does not

Read against the source, not the module names:

| Module | Graph coupling | Transfers |
|---|---|---|
| `chromosome.rs` | none, never imports `Graph` | yes |
| `rng.rs` | none | yes |
| `selection.rs` | none, its only import is `crate::rng::Rng` | yes |
| `crossover_points`, `uniform_crossover`, `mutate` | none, take a length and chromosomes | yes |
| `RepairTargets`, `gene_repair` | `graph.degree()`, `graph.incident()` | no |
| `graph.rs`, `objective.rs` | they are the representation | no |
| `ga.rs` | three call sites: genome length, fitness, repair | needs a trait |

Both of the things that make erbga *erbga* are graph-specific. The
reduced-bias encoding solves the `k!` label-permutation blowup by
encoding removed edges instead of vertex labels, and a non-graph genome
has no permutation problem to solve. Gene Repair restores edges around
high-degree vertices because cuts inside dense neighborhoods fail to
disconnect anything, and a `condition_key` has no degree.

This matters for what may be claimed. The four benchmarks certify ERBGA
on graphs and nothing else. Any future consumer that reuses only the
representation-agnostic scaffolding is running a new algorithm and needs
its own validation. Describing such a thing as validated prior work would
be false.

One thing does transfer with its reasoning intact. `crossover_points`
documents why crossover is uniform and scattered rather than contiguous:
whether edge 7 sits beside edge 8 in the bit string is an artifact of the
sort, so a contiguous segment would impose a linkage structure the
problem does not have. That argument holds unchanged for any genome whose
genes are unordered, which is all of them here.

## What this does not rank

Most never-varied variables are invariant for excellent reasons.
`temperature` is fixed at zero because it should be. A raw invariant list
will be mostly boring, and useful ranking probably weights *cheap to
vary* against *plausibly consequential*.

v0 does not attempt this. Detectors emit findings with evidence counts,
and filtering is left to `validate` and to a human. Automatic ranking is
deferred rather than half-built, because a bad ranker is worse than none:
it hides the interesting item under a confident ordering.

## Integrity properties

These are the testable guarantees, and they are the parts worth mutation
testing rather than merely covering.

1. An expectation cannot be written once an outcome exists for that
   experiment and metric.
2. The anomaly ledger is append-only. No path deletes a row.
3. Nothing in this subsystem writes to `preregistrations`. The same
   guarantee `critique` holds.
4. Detectors perform reads only.
5. No detector reads a column containing model-authored text.
6. The search layer never reports a partition at a single θ. A partition
   is reported only with the band of θ over which it survived.
7. Nothing in the search layer reads a column containing model-authored
   text. Rule 5 states this for detectors; the search layer is bound by
   it too.
8. A recorded partition carries the seed, island count and parameters
   that produced it, so it can be reproduced.

## Testing

TDD throughout, per repo convention. Specifically:

- Integrity rule 1 gets a test that inserts a metric, then attempts the
  expectation, and asserts refusal. Mutate the guard to confirm the test
  fails when the guard is removed. A green test that passes with the
  check deleted is worth nothing here.
- Calibration math is tested against samples drawn from a known
  distribution, where correct coverage is known in advance.
- The re-run gate is tested with a deterministic fake producing each of
  the four classifications.
- Detectors run against fixture databases with the invariant present and
  absent.
- The search layer is checked against the exact backend across the size
  band where both run. A search result that beats proven optimality is a
  bug in the objective rather than a win, and is asserted one-sided the
  way the karate club optimum already is in `erbga`.
- The θ sweep is tested on a fixture whose structure appears at exactly
  one cutoff, asserting it is discarded, and on one stable across a band,
  asserting it survives.
- Integrity rule 7 gets a test that a query built by the search layer
  refuses to name a model-authored column.

## The registry

Aryabhatta is the home for discovery ideas, so it needs a way to hold an
idea without building it. An idea moves to the next state on evidence,
never on enthusiasm, which is the discipline the subsystem already
applies to its own anomalies.

| State | Meaning | What moves it forward | Currently |
|---|---|---|---|
| Proposed | Recorded here. No gate defined, nothing built. | Someone states the number or result that would justify it. | Hypothesis search |
| Gated | The gate is known and the evidence has not arrived. | The number arrives. | Steps 6 and 7 |
| Building | Admitted. TDD, shippable alone, leaves the tree working. | Its own tests and the next step's gate. | Steps 1 to 5 |
| Retired | Measured and failed. Kept, with its result attached. | Nothing. It stays as a record of what was believed. | None yet |

The states are about the build, not about belonging. Everything in the
table is inside aryabhatta. Keeping them separately killable is what lets
step 3 delete the measurement chain without taking the detector lane and
the search layer with it.

### Hypothesis search, and why it is only Proposed

A population where each member holds a hypothesis and the unfit are
eliminated is the shape of `evolve`, which two rounds of adversarial
review found unsound. The finding that applies here is the first of the
three: there is no free inner search, because variation is
model-proposed. A genetic algorithm assumes variation is nearly free.
erbga's own thesis parameters are a population of 250 over 1000
generations, which is 250,000 fitness evaluations per island, times 25
islands. That is affordable when a mutation is a bit flip. Make each
member a spawned agent and every mutation, crossover and evaluation
becomes a model call. A few hundred calls is a tournament rather than
evolution, and a genetic algorithm run for three generations is worse
than generating a dozen candidates once and choosing carefully.

Two further objections, both already recorded as surviving principles
from that review. A population drawn from one model is one model sampled
many times rather than many independent hypotheses, so selection
optimizes for what that model finds fluent. And selecting on a metric
that is then reported is biased upward twice, because pre-registration
does not cover choosing the observation that best clears a fixed test.

The version that survives takes the model out of the inner loop. A
hypothesis becomes a structured object, which `condition_key`s are
implicated in which anomalies and in which direction, so variation is
cheap again and fitness is code: does this structure predict the
deviations already in the ledger? The model proposes the vocabulary once,
at the start. The search over combinations runs in Rust. That answers the
affordability finding directly and keeps the model on the interpreting
side of the quarantine.

It stays Proposed because it needs the anomaly ledger to exist first, and
because it needs its own validation for the reason in section 9.

## Out of scope

Deferred deliberately, each large enough for its own spec:

1. The hypothesis factory and competing explanation agents in their
   agent-population form. The structured-genome version is Proposed in
   the registry above, which carries the reasoning for the split.
2. Automated design of falsifying experiments.
3. The belief graph and persistent world model with per-edge confidence.
4. Information gain as a reward signal.
5. Automatic ranking of invariants.
6. Any autonomous action taken in response to a finding.
7. A `boredom` or `observe` CLI capability. The four capabilities are the
   whole set, and the one attempt at a fifth was not approved: see
   `docs/DECISIONS.md`, 2026-08-14 and 2026-08-15. This is record plus
   readers, the way `critique` is a gate rather than a fifth capability.

## Prior art

Verified in a dedicated pass,
`2026-08-19-anomaly-driven-inquiry-prior-art.md`, which checked every
citation and went looking for work this design did not cite. Read that
document for the detail. The summary:

**The nine original attributions are correct.** Schmidhuber on
compression progress, Pathak et al. 2017 on the Intrinsic Curiosity
Module, Burda et al. 2018 on Random Network Distillation (arXiv
1810.12894), Lindley 1956 on the measure of information provided by an
experiment, Chamberlin 1890 on multiple working hypotheses (in *Science*;
the widely circulated PDF is the 1897 reprint, so cite the year you
mean), Platt 1964 on strong inference, King et al. on the Robot Scientist
(*Nature* 2004, and Adam 2009), Langley's BACON on law rediscovery, and
Google's AI Co-Scientist and Sakana's AI Scientist as examples of the
current wave. `reference/` holds AI-Scientist-v2 locally, read only, per
the 2026-08-08 decision.

**The architecture is already published.** AutoDiscovery (arXiv
2507.00310), "Open-ended Scientific Discovery via Bayesian Surprise", is
this design's core loop, built. It selects which hypotheses to test with
no human-supplied research question, elicits the model's prior beliefs,
gathers results, elicits posteriors, and uses the shift between them as
surprisal, which rewards a Monte Carlo tree search over nested
hypotheses. Two thirds of its discoveries were rated surprising by domain
experts. That is observe, predict, compare, be surprised, pursue: this
design's diagram, minus the record.

What AutoDiscovery does not have, checked against the paper page, is
exactly what this design adds:

| Missing there | Supplied here |
|---|---|
| no pre-registration, so a belief can be stated after the result is known | section 2 and its integrity rule |
| no calibration measurement, so surprisal is never validated | section 6 |
| no re-run gate, so a phenomenon is not separated from a defect | section 4 |

**Two more that need citing rather than fixing.** "Agentic AI Scientists
Are Not Built For Autonomous Scientific Discovery" (arXiv 2605.08956) is
the critique this design answers. It asks for a public preregistration
repository for AI-generated hypotheses and for persistent world models
holding mutable epistemic state, two things being built here. That is
good positioning and bad novelty: the idea is in print, the
implementation is not. "Preregistration for Experiments with AI Agents"
(arXiv 2606.11217) is preregistration *of experiments on* agents, a
different object, but its integrity mechanism is a human attestation that
data collection has not begun, where section 2 refuses the insert in code
when an outcome already exists. Same idea enforced rather than asserted,
and worth drawing out explicitly.

**The boredom detectors have a lineage.** Habituation-based novelty
detection in robotics, for example "Novelty Detection on a Mobile Robot
Using Habituation" (arXiv cs/0006007), habituates to sensory input.
Section 7 habituates to the research process itself, asking what a line
of investigation has stopped varying. Same mechanism, different object.
Not a collision, but not unprecedented either.

### What this design can actually claim

Not that the architecture is novel, because it is substantially
AutoDiscovery. Not that the section 6 measurement is unanswered, because
it is answered next door for the exogenous case, with a result that
predicts failure.

What survives is narrower and more interesting than what was first
claimed:

> Prediction-error curiosity has been moved from RL agents to LLM agents
> doing open-ended discovery, and the move is being made without the
> guardrails the RL literature spent a decade building. AutoDiscovery
> uses elicited belief shift as a reward with no calibration check and no
> reproducibility gate. Nobody has measured whether an LLM's
> pre-registered interval about an experiment it is about to run is
> calibrated well enough to carry that weight, and the closest evidence,
> 65% coverage at a nominal 99%, says probably not. The endogenous case
> is also the action-dependent noisy TV, where the agent can move the
> signal it is rewarded on.

That is a claim about whether the current wave has a measurement problem,
supported by an artifact that measures it. The negative result stays
publishable, and it now has a named target to be negative about.

The same discipline applies inside the repo. Section 9 records what may
and may not be claimed about `erbga`: the four benchmarks certify ERBGA
on graphs, and any consumer reusing only its representation-agnostic
scaffolding is running a new algorithm that needs its own validation.

## Open questions

1. Should expectations be git-pinned like preregistrations, or is the DB
   constraint plus a visible calibration curve sufficient? v0 assumes the
   latter.
2. What tolerance counts as calibrated? Set from the first observed
   curve, not guessed now.
3. The re-run gate costs a second execution of every anomalous
   experiment. Is that affordable at the point where experiments are
   expensive, and should the gate be sampled rather than exhaustive?
4. Cold start. Every boredom detector needs history, and zorp is
   pre-alpha with no user holding 43 experiments. Checkpoint habituation
   is the partial answer since it accrues within a single investigation,
   but the others need a corpus that does not exist yet.
5. **The action-dependent noisy TV.** The agent chooses its own actions
   and writes its own expectations, so it can move its surprise rate in
   either direction at will. This has a name and a literature in
   intrinsic-motivation RL: the agent handed a remote control to the
   noise source. The mitigation family is separating reducible from
   irreducible uncertainty, which is what the section 4 re-run gate
   already does empirically. Calibration tracking makes the gaming
   visible without preventing it. Still open, but no longer unnamed, and
   it is what makes the endogenous calibration question different in kind
   from a static benchmark rather than merely different in domain.
6. Where the |V| crossover between the exact backend and `erbga` sits. It
   should be measured on real condition graphs, not guessed here.
7. Whether the confounded condition relation is crisp or graded. If pairs
   either never vary independently or always do, the bundles are
   connected components, union-find answers them exactly, and the search
   layer earns its place only on the anomaly side. This is a cheap query
   once step 1 has data, and it should be run before the `erbga` backend
   is built.

## Implementation order

Each step is independently shippable and each leaves the tree working.

1. `conditions` table and its recording API.
2. `expectations` table with the integrity rule and its mutation test.
3. Surprise arithmetic and the calibration report. **The report is the
   decision point.** If calibration is hopeless, stop here, write it up,
   and do not build the rest.
4. Checkpoint habituation detector. Independent of 1 through 3 and
   works on data that exists today, so it can be built in parallel.
5. The search layer with its exact backend, and the confounded condition
   caller. Depends on step 1 only and sits on the ungated side, so it can
   be built alongside 2 and 3. The `erbga` backend follows once a real
   graph is large enough to need it, which is a measurement rather than a
   guess.
6. The re-run gate and the anomaly ledger.
7. Remaining detectors, anomaly families through the search layer, and
   the handoff to `validate`.
