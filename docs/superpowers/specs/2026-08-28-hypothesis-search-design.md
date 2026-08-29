# Hypothesis search: gate, substrate, validation

Date: 2026-08-28. Status: approved design.

This design moves hypothesis search, the one unbuilt entry in the
aryabhatta registry, from Proposed to Gated. It was Proposed in
`2026-08-19-anomaly-driven-inquiry-design.md` for three stated reasons:
no gate was defined, the anomaly ledger did not exist, and the algorithm
needed its own validation. This design states the gate, supplies the
validation on synthetic data, and builds the algorithm and its
substrate. It deliberately does not wire anything to the real ledger.
That remains locked until the admission gate's number arrives.

## What ships

Three deliverables, each shippable alone and each leaving the tree
working:

1. A representation-agnostic `Problem` trait in `erbga`, with the graph
   path proven equivalent to the current code.
2. A hypothesis search module in `zorp-track`, validated against
   synthetic ledgers with planted structure.
3. The gate, stated in `docs/DECISIONS.md` and in this spec, moving the
   registry entry to Gated.

## What stays out

No adapter reads the real anomaly ledger. No wiring into `inquiry` or
`validate`. No CLI command, no web route, no model call anywhere in the
new code. The registry's rule is that an idea is admitted by evidence,
and the evidence has not arrived: the real ledger is empty. Building the
reader before the record has rows would be building a consumer for data
that does not exist, which is the mistake the registry exists to stop.

## The gate

Two parts. The first is satisfied inside this change. The second is not,
and that is the point.

**Validation gate, satisfied here.** The algorithm must exactly recover
planted implicated sets on crisp synthetic ledgers, and must beat a
permutation null on noisy ones, across multiple seeds. These are tests
in the tree. If they cannot be made to pass, the module does not merge
and the registry entry stays Proposed with the failure recorded.

**Admission gate, not satisfied here.** Hypothesis search may run
against the real anomaly ledger only once the ledger holds at least 12
admitted anomalies spanning at least 3 distinct condition keys. The
number 12 matches the minimum support the boredom detectors already use
for every detector except habituation, and for the same reason: a
structure fitted to fewer rows is arithmetic about those rows, not
evidence of anything. Crossing this gate is a decision a person makes,
recorded in `docs/DECISIONS.md`, and it is when the real-ledger adapter
gets built.

## The substrate: a `Problem` trait in erbga

`erbga::ga::run_island` touches the graph at exactly three points:
genome length (`graph.edge_count()`), fitness
(`objective.score(graph, &graph.partition(c))`), and repair
(`RepairTargets::new` plus `gene_repair`). Everything else in the loop,
chromosome, seeded RNG, tournament selection, elitism, uniform
crossover, mutation, never mentions a graph.

The change introduces a trait covering those three points:

```rust
pub trait Problem {
    fn genome_len(&self) -> usize;
    fn fitness(&self, chromosome: &Chromosome) -> f64;
    fn repair(&self, chromosome: &mut Chromosome, rng: &mut Rng);
}
```

A generic runner `run_island_on<P: Problem>` holds the current loop
body. The existing `run_island` and `run_islands` keep their signatures
and delegate through a private graph-flavored `Problem` implementation
built from `Graph`, `Objective`, `RepairTargets`, and the repair
parameters. Public API is unchanged.

**The equivalence guarantee.** Seeded runs must produce identical
`Best` values, chromosome and fitness both, before and after the
refactor. The in-crate xorshift64* RNG makes this checkable: the
generic runner must consume randomness in the same order the current
loop does. The existing reproducibility and benchmark tests pin most of
this; one new test pins recorded seeded outcomes on the two-triangle
graph so a later reordering of RNG draws fails loudly.

**The naming trap, honored.** The four benchmarks certify ERBGA on
graphs and nothing else. The trait's documentation says so, and nothing
in the new code calls the generic core ERBGA. The reduced-bias
removed-edge encoding and Gene Repair stay on the graph path, because
they are what make erbga erbga and neither has a meaning for a
hypothesis bitmask.

Alternatives considered and rejected: re-implementing a small GA in
`zorp-track` duplicates tested code and loses the operator choices'
recorded reasoning; a new shared crate is ceremony for a single
consumer. `zorp-track` already depends on `erbga`, so the trait rides
the wired direction.

## The module: `zorp-track/src/hypotheses.rs`

A sibling of `partition.rs` and `families.rs` in the search layer.

**Input.** A plain struct of code-visible fields only, built by the
caller, never by a database read in this module:

- experiment rows: condition key and value pairs, plus whether the
  experiment produced an admitted anomaly and, if so, the deviation
  sign.

The type has no free-text field at all, so integrity rule 5 holds by
construction: there is nothing in the input a model could have written.
The vocabulary is the set of distinct condition keys present in the
input. No model proposes anything, which is narrower than the 2026-08-19
sketch ("the model proposes the vocabulary once") and deliberately so.
Condition keys are already code-visible columns; a model adds nothing
but risk here.

**Genome.** A bitmask over the vocabulary of (condition key, deviation
sign) pairs. A set bit claims that key is implicated in anomalies of
that sign. Genome length is twice the vocabulary size. There is no
label-permutation problem and no vertex degree, which is exactly why the
graph-specific parts of erbga do not transfer.

**Fitness, all code.** A hypothesis H earns reward for each admitted
anomaly it covers, meaning some claimed (key, sign) pair matches a
condition key present on that row and the row's deviation sign. It pays
for each non-anomalous experiment it would have flagged by the same
rule, and it pays a parsimony cost of lambda per set bit. Fitness is
coverage minus false coverage minus lambda times the bit count,
normalized by row count so lambda has a stable meaning across ledger
sizes. The exact functional form may be tuned during implementation;
the acceptance criterion is fixed and is the planted-structure test
suite, not the formula.

**Lambda is swept, never chosen.** The same discipline `partition.rs`
applies to theta. The search runs across a coarse lambda range, and
only implicated sets stable across a contiguous lambda band are
reported, with the band recorded next to the result. Nobody picks
lambda, so nobody can pick it to get the answer they wanted.

**Determinism.** The search is seeded through the same in-crate RNG,
and seed, island count, parameter set, and lambda range are part of the
result, the way `partition.rs` records its runs.

**Output.** Reported implicated sets with their lambda bands, fitness,
and run parameters. Nothing consumes the output. It is for a person to
read, and the future handoff into `validate` is a separate decision
behind the admission gate.

## Validation

Synthetic ledgers with planted structure, generated in tests with known
ground truth:

1. **Crisp plants.** Anomalies constructed so one known (key, sign) set
   explains all of them and no non-anomalous row matches. The search
   must recover the planted set exactly, across several seeds and
   several vocabulary sizes.
2. **Noisy plants.** Label noise added at a stated rate. The recovered
   set must beat a permutation null: shuffle the anomaly labels, rerun,
   and the planted-data fitness must exceed the shuffled-data fitness
   distribution across seeds.
3. **No plant.** A ledger with no structure must report no stable band,
   not a confident set. This is the analog of a band too thin to judge
   being its own no-go.

These tests are the validation gate. They certify the new algorithm on
synthetic ledgers, and only that. Running on the real ledger is a
separate claim behind the admission gate, and the docs say so in the
module header.

## Integrity

- The input type has no text field; a test asserts the input builder
  names no model-authored column, mirroring the ledger reader's
  existing test.
- No function in the module performs I/O or takes a database handle.
- Nothing writes to any store. The module is a pure function from rows
  to a report.

## Registry and records

- `docs/DECISIONS.md` gets one entry: the gate as stated above, the
  registry move from Proposed to Gated, and the reasoning for keeping
  the real-ledger adapter unbuilt.
- The 2026-08-19 spec stays as written, per the log's convention that
  old entries are superseded, not rewritten.

## Testing

- `cargo test -p erbga` including the new equivalence pin.
- `cargo test -p zorp-track` including the planted-structure suite.
- `cargo build --workspace` and `cargo test --workspace` before the PR.
- `cargo fmt --all` before every commit.
