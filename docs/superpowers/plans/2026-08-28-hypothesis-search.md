# Hypothesis Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the hypothesis search from the approved design: a `Problem` trait in erbga, a `hypotheses` module in zorp-track validated on synthetic planted ledgers, and the gate recorded in DECISIONS.md, with nothing reading the real anomaly ledger.

**Architecture:** erbga's generational loop is extracted behind a small `Problem` trait, with the graph path delegating through it unchanged and pinned by a seeded-equivalence test. `zorp-track/src/hypotheses.rs` implements the trait over plain in-memory rows: a genome is a bitmask over (condition atom, direction) pairs, fitness is pure arithmetic, and the parsimony weight lambda is swept the way `families.rs` sweeps theta, with only claim sets stable across a band reported.

**Tech Stack:** Rust workspace. No new dependencies anywhere. erbga stays zero-dependency and must keep knowing nothing about zorp.

**Spec:** `docs/superpowers/specs/2026-08-28-hypothesis-search-design.md`

## Global Constraints

- MSRV is 1.95 (root `Cargo.toml` `rust-version`); no nightly features.
- No new dependencies in any crate. erbga stays zero-dependency.
- erbga's public API (`run_island`, `run_islands`, `best_of`, `Best`, `GaParams`) keeps its exact current signatures.
- Seeded erbga runs must produce identical results before and after the refactor. The RNG draw order may not change.
- No code in this plan reads a database, performs I/O, or calls a model. `hypotheses.rs` is a pure function from rows to a report.
- Repo prose style: no em dashes or en dashes anywhere (docs, comments, commit messages). Use commas, colons, periods, or plain hyphenated compounds.
- Run `cargo fmt --all` before every commit. CI gates on it.
- Every commit message ends with the trailer line: `Claude-Session: https://claude.ai/code/session_01NwHQyG7XFVHHwskEi8VsLw` (blank line before it).
- Work happens on branch `feat/hypothesis-search` in this worktree.

---

### Task 1: The `Problem` trait and generic runner in erbga

**Files:**
- Modify: `erbga/src/ga.rs`
- Modify: `erbga/src/lib.rs:34-39` (re-exports)

**Interfaces:**
- Consumes: existing `Chromosome`, `Rng`, `Graph`, `Objective`, `RepairTargets`, `gene_repair`, and the loop body of `run_island` (`erbga/src/ga.rs:98-173`).
- Produces: `pub trait Problem { fn genome_len(&self) -> usize; fn fitness(&self, chromosome: &Chromosome) -> f64; fn repair(&self, chromosome: &mut Chromosome, rng: &mut Rng) {} }`, `pub fn run_island_on<P: Problem>(problem: &P, params: &GaParams, seed: u64) -> Best`, `pub fn run_islands_on<P: Problem>(problem: &P, params: &GaParams, islands: usize, base_seed: u64) -> Vec<Best>`. All re-exported from `erbga` root. Task 3 calls `run_islands_on` through these exact names.

- [ ] **Step 1: Write the pin test with placeholder constants**

Add to the `tests` module in `erbga/src/ga.rs`:

```rust
    /// Captured from the run before the Problem trait extraction. If any
    /// assertion here changes, the refactor reordered the RNG draws, and
    /// the seeded history the four benchmarks were run under is gone.
    #[test]
    fn seeded_run_is_pinned_across_refactors() {
        let g = two_triangles();
        let best = run_island(&g, &Modularity, &small_params(), 42);
        println!(
            "capture: fitness_bits={:#018x} generation={} ones={}",
            best.fitness.to_bits(),
            best.generation,
            best.chromosome.count_ones()
        );
        assert_eq!(best.fitness.to_bits(), 0);
        assert_eq!(best.generation, usize::MAX);
        assert_eq!(best.chromosome.count_ones(), usize::MAX);
    }
```

- [ ] **Step 2: Run it, capture the real values, make it pass**

Run: `cargo test -p erbga seeded_run_is_pinned -- --nocapture`
Expected: FAIL, with a `capture:` line printing the real values.
Replace the three placeholder constants (`0`, `usize::MAX`, `usize::MAX`) with the printed values. Re-run the same command. Expected: PASS.

- [ ] **Step 3: Commit the pin**

```bash
git add erbga/src/ga.rs
git commit -m "test(erbga): pin one seeded run before the trait extraction"
```

- [ ] **Step 4: Introduce the trait and the generic runner**

In `erbga/src/ga.rs`, add above `run_island`:

```rust
/// A search problem the generational loop can run against.
///
/// The loop needs exactly three things from a problem: how long a genome
/// is, how fit a chromosome is, and how to repair a child after mutation.
/// Everything else in the loop, seeded RNG, tournament selection,
/// elitism, uniform crossover, mutation, never mentions a representation.
///
/// The four published benchmarks certify the graph path and nothing
/// else. A consumer implementing this trait for another representation
/// is running a new, unvalidated algorithm and needs its own validation.
pub trait Problem {
    fn genome_len(&self) -> usize;
    fn fitness(&self, chromosome: &Chromosome) -> f64;
    /// Repair a child after mutation. The default does nothing, which is
    /// correct for representations with no analog of Gene Repair.
    fn repair(&self, _chromosome: &mut Chromosome, _rng: &mut Rng) {}
}
```

Then move the loop body into a generic runner and make the graph entry points delegate. The final shape of the three functions:

```rust
/// Run one island of any `Problem` to completion.
///
/// This is the loop `run_island` always ran; the graph path delegates
/// here through a private `Problem` implementation, so there is one loop
/// and the benchmarks exercise it.
pub fn run_island_on<P: Problem>(problem: &P, params: &GaParams, seed: u64) -> Best {
    assert!(
        params.population_size >= 2,
        "population must hold at least 2"
    );
    let genome_len = problem.genome_len();
    let mut rng = Rng::new(seed);

    let elite_count = ((params.elitism_rate * params.population_size as f64).round() as usize)
        .min(params.population_size);

    let mut population: Vec<Chromosome> = (0..params.population_size)
        .map(|_| Chromosome::random(genome_len, params.initial_one_rate, &mut rng))
        .collect();

    let mut best: Option<Best> = None;

    // Evaluate `generations + 1` times: once per generation plus once on
    // the final population, so the last round of breeding is not thrown
    // away unmeasured.
    for generation in 0..=params.generations {
        let fitness: Vec<f64> = population.iter().map(|c| problem.fitness(c)).collect();

        for (i, &f) in fitness.iter().enumerate() {
            if best.as_ref().is_none_or(|b| f > b.fitness) {
                best = Some(Best {
                    chromosome: population[i].clone(),
                    fitness: f,
                    generation,
                });
            }
        }

        if generation == params.generations {
            break;
        }

        let mut next: Vec<Chromosome> = elite_indices(&fitness, elite_count)
            .into_iter()
            .map(|i| population[i].clone())
            .collect();

        while next.len() < params.population_size {
            let (p, q) = tournament(&fitness, params.tournament_pool, &mut rng);
            let (mut first, mut second) = if rng.unit() < params.crossover_rate {
                let points = crossover_points(genome_len, params.crossover_point_rate, &mut rng);
                uniform_crossover(&population[p], &population[q], &points)
            } else {
                (population[p].clone(), population[q].clone())
            };

            for child in [&mut first, &mut second] {
                mutate(child, params.mutation_rate, &mut rng);
                problem.repair(child, &mut rng);
            }

            next.push(first);
            if next.len() < params.population_size {
                next.push(second);
            }
        }

        population = next;
    }

    best.expect("population is non-empty so a best always exists")
}

/// The graph problem: fitness through the objective on the canonical
/// partition, repair through Gene Repair. Private on purpose; graphs
/// enter through `run_island` and `run_islands` as they always have.
struct GraphProblem<'a, O: Objective> {
    graph: &'a Graph,
    objective: &'a O,
    targets: RepairTargets,
    repair_chance: f64,
}

impl<O: Objective> Problem for GraphProblem<'_, O> {
    fn genome_len(&self) -> usize {
        self.graph.edge_count()
    }

    fn fitness(&self, chromosome: &Chromosome) -> f64 {
        self.objective
            .score(self.graph, &self.graph.partition(chromosome))
    }

    fn repair(&self, chromosome: &mut Chromosome, rng: &mut Rng) {
        gene_repair(chromosome, self.graph, &self.targets, self.repair_chance, rng);
    }
}

fn graph_problem<'a, O: Objective>(
    graph: &'a Graph,
    objective: &'a O,
    params: &GaParams,
) -> GraphProblem<'a, O> {
    let repair_size = (params.repair_rate * graph.edge_count() as f64).round() as usize;
    GraphProblem {
        graph,
        objective,
        targets: RepairTargets::new(graph, repair_size),
        repair_chance: params.repair_chance,
    }
}
```

`run_island` and `run_islands` keep their exact signatures and doc comments and become:

```rust
pub fn run_island<O: Objective>(
    graph: &Graph,
    objective: &O,
    params: &GaParams,
    seed: u64,
) -> Best {
    run_island_on(&graph_problem(graph, objective, params), params, seed)
}
```

```rust
pub fn run_islands<O: Objective>(
    graph: &Graph,
    objective: &O,
    params: &GaParams,
    islands: usize,
    base_seed: u64,
) -> Vec<Best> {
    run_islands_on(&graph_problem(graph, objective, params), params, islands, base_seed)
}
```

And the generic island fan-out:

```rust
/// Run several independent islands of any `Problem`.
pub fn run_islands_on<P: Problem>(
    problem: &P,
    params: &GaParams,
    islands: usize,
    base_seed: u64,
) -> Vec<Best> {
    assert!(islands >= 1, "need at least one island");
    (0..islands)
        .map(|i| run_island_on(problem, params, base_seed.wrapping_add(i as u64 * 0x9E37_79B9)))
        .collect()
}
```

Note the RNG order: `RepairTargets::new` consumes no randomness, so building it inside `graph_problem` before `Rng::new` preserves the draw sequence exactly. Do not move `Chromosome::random` or any operator call relative to each other.

In `erbga/src/lib.rs`, extend the ga re-export line to:

```rust
pub use ga::{best_of, run_island, run_island_on, run_islands, run_islands_on, Best, GaParams, Problem};
```

- [ ] **Step 5: Run the full erbga suite, pin included**

Run: `cargo test -p erbga`
Expected: PASS, every test, and `seeded_run_is_pinned_across_refactors` unchanged from Step 2. If the pin fails, the refactor changed the draw order: fix the refactor, never the pin.

- [ ] **Step 6: Commit**

```bash
git add erbga/src/ga.rs erbga/src/lib.rs
git commit -m "refactor(erbga): the loop runs any Problem, and the graph path is one of them"
```

---

### Task 2: `hypotheses` module: rows, vocabulary, claims

**Files:**
- Create: `zorp-track/src/hypotheses.rs`
- Modify: `zorp-track/src/lib.rs` (add `pub mod hypotheses;` in alphabetical order, after `families`)

**Interfaces:**
- Consumes: `crate::families::Direction` (`zorp-track/src/families.rs:43`, derives `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord`), `erbga::Chromosome`.
- Produces: `pub struct ExperimentRow { pub conditions: Vec<(String, String)>, pub outcome: Option<Direction> }`; `pub struct Claim { pub key: String, pub value: String, pub direction: Direction }`; `pub struct Vocabulary` with `pub fn from_rows(&[ExperimentRow]) -> Vocabulary`, `pub fn len(&self) -> usize`, `pub fn is_empty(&self) -> bool`, `pub fn genome_len(&self) -> usize`, `pub fn decode(&self, &Chromosome) -> Vec<Claim>`. Tasks 3 and 4 use these exact names.

- [ ] **Step 1: Write the failing tests**

Create `zorp-track/src/hypotheses.rs` with the module header and a tests module only:

```rust
//! Hypothesis search over condition atoms, the structured-genome version
//! the 2026-08-19 design left Proposed and the 2026-08-28 design admits
//! to Gated.
//!
//! A hypothesis is a set of claims, each "this condition atom is
//! implicated in admitted anomalies of this direction". The genome is a
//! bitmask over (atom, direction) pairs, fitness is arithmetic against
//! rows the caller supplies, and the parsimony weight lambda is swept
//! the way `families` sweeps theta: only claim sets stable across a
//! contiguous band are reported, with the band recorded.
//!
//! **What is claimed, and what is not.** The search runs on erbga's
//! generic loop through the `Problem` trait. The four erbga benchmarks
//! certify the graph path and nothing here: this module's validation is
//! its planted-structure tests, which certify it on synthetic ledgers
//! only. Running it against the real anomaly ledger is behind the
//! admission gate in `docs/DECISIONS.md` (2026-08-28): at least 12
//! admitted anomalies spanning at least 3 distinct condition keys, and a
//! person's recorded decision. No function in this module reads a
//! database, and no adapter to the real ledger exists.
//!
//! **Integrity.** The input type holds condition atoms and a direction
//! and nothing else. There is no field a model-authored sentence could
//! travel in, so integrity rule 5 holds by construction rather than by
//! filtering.

use crate::families::Direction;
use erbga::Chromosome;

#[cfg(test)]
mod tests {
    use super::*;

    fn row(conditions: &[(&str, &str)], outcome: Option<Direction>) -> ExperimentRow {
        ExperimentRow {
            conditions: conditions
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            outcome,
        }
    }

    /// Exhaustive struct literal on purpose: adding any field to
    /// `ExperimentRow`, in particular a text field, breaks this compile
    /// until someone edits it here, in front of the integrity note.
    #[test]
    fn the_input_type_carries_atoms_and_a_direction_and_nothing_else() {
        let _ = ExperimentRow {
            conditions: vec![("k".to_string(), "v".to_string())],
            outcome: Some(Direction::Above),
        };
    }

    #[test]
    fn vocabulary_is_sorted_deduped_atoms() {
        let rows = vec![
            row(&[("b", "2"), ("a", "1")], None),
            row(&[("a", "1"), ("a", "2")], Some(Direction::Below)),
        ];
        let v = Vocabulary::from_rows(&rows);
        assert_eq!(v.len(), 3);
        assert_eq!(v.genome_len(), 6);
        assert!(!v.is_empty());
    }

    #[test]
    fn empty_rows_give_an_empty_vocabulary() {
        let v = Vocabulary::from_rows(&[]);
        assert!(v.is_empty());
        assert_eq!(v.genome_len(), 0);
    }

    #[test]
    fn decode_names_the_set_bits_as_sorted_claims() {
        let rows = vec![row(&[("a", "1"), ("b", "2")], None)];
        let v = Vocabulary::from_rows(&rows);
        // Atoms sort to [(a,1), (b,2)]. Bit layout is atom_index * 2,
        // Below first. Set (a,1,Above) and (b,2,Below).
        let mut c = Chromosome::zeros(v.genome_len());
        c.set(1, true);
        c.set(2, true);
        let claims = v.decode(&c);
        assert_eq!(
            claims,
            vec![
                Claim {
                    key: "a".to_string(),
                    value: "1".to_string(),
                    direction: Direction::Above,
                },
                Claim {
                    key: "b".to_string(),
                    value: "2".to_string(),
                    direction: Direction::Below,
                },
            ]
        );
    }
}
```

- [ ] **Step 2: Register the module and verify the tests fail to compile**

Add `pub mod hypotheses;` to `zorp-track/src/lib.rs` after `pub mod families;`.

Run: `cargo test -p zorp-track hypotheses`
Expected: COMPILE FAILURE, `ExperimentRow` and `Vocabulary` not found.

- [ ] **Step 3: Implement the types**

Add between the imports and the tests module:

```rust
/// One experiment, reduced to what a hypothesis may be scored against.
///
/// Condition atoms as recorded, and whether the experiment produced an
/// admitted anomaly. Held deliberately narrow: see the module header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperimentRow {
    /// Condition key and value pairs.
    pub conditions: Vec<(String, String)>,
    /// `Some` if the experiment produced an admitted anomaly, with the
    /// deviation's direction. `None` for an unremarkable run.
    pub outcome: Option<Direction>,
}

/// One claim: this condition atom is implicated in admitted anomalies
/// of this direction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Claim {
    pub key: String,
    pub value: String,
    pub direction: Direction,
}

/// The distinct condition atoms across a set of rows, sorted, which is
/// what makes genome bit positions stable and a decode deterministic.
///
/// Atoms, not bare keys: a real ledger records the same keys on every
/// row with differing values, so the key set separates nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vocabulary {
    atoms: Vec<(String, String)>,
}

impl Vocabulary {
    pub fn from_rows(rows: &[ExperimentRow]) -> Self {
        let mut atoms: Vec<(String, String)> = rows
            .iter()
            .flat_map(|r| r.conditions.iter().cloned())
            .collect();
        atoms.sort();
        atoms.dedup();
        Vocabulary { atoms }
    }

    pub fn len(&self) -> usize {
        self.atoms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }

    /// Two bits per atom, one per direction.
    pub fn genome_len(&self) -> usize {
        self.atoms.len() * 2
    }

    /// The bit claiming `direction` for the atom at `index`.
    fn bit(index: usize, direction: Direction) -> usize {
        index * 2
            + match direction {
                Direction::Below => 0,
                Direction::Above => 1,
            }
    }

    /// The set bits, as sorted claims.
    pub fn decode(&self, chromosome: &Chromosome) -> Vec<Claim> {
        let mut claims = Vec::new();
        for (index, (key, value)) in self.atoms.iter().enumerate() {
            for direction in [Direction::Below, Direction::Above] {
                if chromosome.get(Self::bit(index, direction)) {
                    claims.push(Claim {
                        key: key.clone(),
                        value: value.clone(),
                        direction,
                    });
                }
            }
        }
        claims.sort();
        claims
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p zorp-track hypotheses`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/hypotheses.rs zorp-track/src/lib.rs
git commit -m "feat(track): hypothesis rows, claims, and an atom vocabulary"
```

---

### Task 3: Fitness, and the `Problem` implementation over rows

**Files:**
- Modify: `zorp-track/src/hypotheses.rs`

**Interfaces:**
- Consumes: Task 2's `ExperimentRow`, `Vocabulary`, `Claim`; Task 1's `erbga::Problem`.
- Produces: `pub(crate) fn score(rows: &[ExperimentRow], vocabulary: &Vocabulary, chromosome: &Chromosome, lambda: f64) -> f64`; `struct LedgerFit<'a> { rows: &'a [ExperimentRow], vocabulary: &'a Vocabulary, lambda: f64 }` implementing `erbga::Problem`. Task 4 constructs `LedgerFit` by these field names.

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `hypotheses.rs`:

```rust
    /// Encode `claims` for `vocabulary`, panicking on an atom the
    /// vocabulary does not hold. Test-side inverse of `decode`.
    fn encode(v: &Vocabulary, claims: &[(&str, &str, Direction)]) -> Chromosome {
        let mut c = Chromosome::zeros(v.genome_len());
        for (key, value, direction) in claims {
            let index = v
                .atoms
                .iter()
                .position(|(k, val)| k == key && val == value)
                .expect("atom in vocabulary");
            c.set(Vocabulary::bit(index, *direction), true);
        }
        c
    }

    #[test]
    fn a_correct_claim_is_rewarded_and_parsimony_is_charged() {
        let rows = vec![
            row(&[("harness", "b")], Some(Direction::Above)),
            row(&[("harness", "a")], None),
        ];
        let v = Vocabulary::from_rows(&rows);
        let h = encode(&v, &[("harness", "b", Direction::Above)]);
        // One correct cover, no wrong flags, one set bit:
        // (1 - 0 - lambda * 1) / 2 rows.
        let got = score(&rows, &v, &h, 0.1);
        assert!((got - (1.0 - 0.1) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn flagging_an_unremarkable_row_costs() {
        let rows = vec![
            row(&[("harness", "b")], Some(Direction::Above)),
            row(&[("harness", "b")], None),
        ];
        let v = Vocabulary::from_rows(&rows);
        let h = encode(&v, &[("harness", "b", Direction::Above)]);
        // One correct, one wrong flag: (1 - 1 - 0.1) / 2.
        let got = score(&rows, &v, &h, 0.1);
        assert!((got - (-0.1) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_claim_in_the_wrong_direction_is_a_wrong_flag() {
        let rows = vec![row(&[("harness", "b")], Some(Direction::Above))];
        let v = Vocabulary::from_rows(&rows);
        let h = encode(&v, &[("harness", "b", Direction::Below)]);
        // Flagged, not correct: (0 - 1 - 0.1) / 1.
        let got = score(&rows, &v, &h, 0.1);
        assert!((got - (-1.1)).abs() < 1e-12);
    }

    #[test]
    fn the_empty_hypothesis_scores_zero() {
        let rows = vec![row(&[("harness", "b")], Some(Direction::Above))];
        let v = Vocabulary::from_rows(&rows);
        let h = Chromosome::zeros(v.genome_len());
        assert_eq!(score(&rows, &v, &h, 0.1), 0.0);
    }

    #[test]
    fn no_rows_score_zero_rather_than_dividing_by_nothing() {
        let v = Vocabulary::from_rows(&[]);
        let h = Chromosome::zeros(0);
        assert_eq!(score(&[], &v, &h, 0.1), 0.0);
    }
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p zorp-track hypotheses`
Expected: COMPILE FAILURE, `score` not found.

- [ ] **Step 3: Implement `score` and `LedgerFit`**

```rust
/// How well a hypothesis fits the rows, in arithmetic a reader can
/// recompute by hand.
///
/// A claim (atom, direction) flags every row carrying that atom, and
/// predicts its deviation runs that way. A row is covered correctly when
/// it produced an admitted anomaly and some claim on one of its atoms
/// matches the direction. A flagged row that is not correctly covered is
/// a wrong flag, whether it was unremarkable or deviated the other way.
///
/// score = (correct - wrong - lambda * set bits) / rows
///
/// Normalizing by row count keeps lambda's meaning stable across ledger
/// sizes. The exact form is certified by the planted-structure tests,
/// not the other way around.
pub(crate) fn score(
    rows: &[ExperimentRow],
    vocabulary: &Vocabulary,
    chromosome: &Chromosome,
    lambda: f64,
) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut correct = 0i64;
    let mut wrong = 0i64;
    for r in rows {
        let mut claims_below = false;
        let mut claims_above = false;
        for (index, atom) in vocabulary.atoms.iter().enumerate() {
            if r.conditions.iter().any(|c| c == atom) {
                claims_below |= chromosome.get(Vocabulary::bit(index, Direction::Below));
                claims_above |= chromosome.get(Vocabulary::bit(index, Direction::Above));
            }
        }
        let flagged = claims_below || claims_above;
        let covered = match r.outcome {
            Some(Direction::Below) => claims_below,
            Some(Direction::Above) => claims_above,
            None => false,
        };
        if covered {
            correct += 1;
        } else if flagged {
            wrong += 1;
        }
    }
    let bits = chromosome.count_ones() as f64;
    ((correct - wrong) as f64 - lambda * bits) / rows.len() as f64
}

/// The rows as a search problem. Repair stays the trait's no-op: a
/// condition atom has no degree, so Gene Repair has no meaning here.
struct LedgerFit<'a> {
    rows: &'a [ExperimentRow],
    vocabulary: &'a Vocabulary,
    lambda: f64,
}

impl erbga::Problem for LedgerFit<'_> {
    fn genome_len(&self) -> usize {
        self.vocabulary.genome_len()
    }

    fn fitness(&self, chromosome: &Chromosome) -> f64 {
        score(self.rows, self.vocabulary, chromosome, self.lambda)
    }
}
```

Note: `LedgerFit` is unused until Task 4; if the compiler warns, add `#[allow(dead_code)]` and remove it in Task 4.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p zorp-track hypotheses`
Expected: PASS, 9 tests.

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/hypotheses.rs
git commit -m "feat(track): hypothesis fitness is arithmetic a reader can recompute"
```

---

### Task 4: The lambda sweep and the report

**Files:**
- Modify: `zorp-track/src/hypotheses.rs`

**Interfaces:**
- Consumes: Task 3's `LedgerFit` and `score`; Task 1's `erbga::{run_islands_on, best_of, GaParams}`.
- Produces: `pub struct LambdaSweep { pub low: f64, pub high: f64, pub steps: usize }` (Default 0.02 to 0.50 in 7 steps); `pub struct LambdaBand { pub low: f64, pub high: f64, pub steps: usize }`; `pub struct StableHypothesis { pub claims: Vec<Claim>, pub band: LambdaBand, pub fitness: f64 }`; `pub struct HypothesisReport { pub hypotheses: Vec<StableHypothesis>, pub swept: LambdaSweep, pub min_band: usize, pub rows_considered: usize, pub vocabulary_len: usize, pub discarded_as_unstable: usize, pub seed: u64, pub islands: usize, pub params: erbga::GaParams }`; `pub fn search(rows: &[ExperimentRow], seed: u64) -> HypothesisReport`; `pub fn search_with(rows: &[ExperimentRow], swept: LambdaSweep, min_band: usize, params: &erbga::GaParams, islands: usize, seed: u64) -> HypothesisReport`. Task 5's tests call both by these exact signatures.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn the_default_sweep_is_recorded_in_the_report() {
        let rows = vec![
            row(&[("harness", "b")], Some(Direction::Above)),
            row(&[("harness", "a")], None),
        ];
        let report = search(&rows, 7);
        assert_eq!(report.swept, LambdaSweep::default());
        assert_eq!(report.min_band, 2);
        assert_eq!(report.rows_considered, 2);
        assert_eq!(report.vocabulary_len, 2);
        assert_eq!(report.seed, 7);
        assert_eq!(report.islands, 4);
    }

    #[test]
    fn empty_rows_report_nothing_and_say_why() {
        let report = search(&[], 7);
        assert!(report.hypotheses.is_empty());
        assert_eq!(report.rows_considered, 0);
        assert_eq!(report.vocabulary_len, 0);
    }

    #[test]
    fn sweep_values_are_evenly_spaced_and_inclusive() {
        let swept = LambdaSweep {
            low: 0.1,
            high: 0.5,
            steps: 5,
        };
        let values = swept.values();
        assert_eq!(values.len(), 5);
        assert!((values[0] - 0.1).abs() < 1e-12);
        assert!((values[4] - 0.5).abs() < 1e-12);
        assert!((values[1] - 0.2).abs() < 1e-12);
    }
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p zorp-track hypotheses`
Expected: COMPILE FAILURE, `search`, `LambdaSweep` not found.

- [ ] **Step 3: Implement the sweep**

```rust
/// The lambda range the sweep covered, and how finely.
///
/// The same shape and reasoning as `families::ThetaSweep`: recorded next
/// to the result, because a band means nothing without the range it was
/// measured over. Nobody picks lambda, so nobody can pick it to get the
/// answer they wanted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LambdaSweep {
    pub low: f64,
    pub high: f64,
    pub steps: usize,
}

impl Default for LambdaSweep {
    /// 0.02 to 0.50 in seven steps.
    ///
    /// Deliberately not tuned, for the reason `ThetaSweep::default`
    /// gives: tuning against zorp's own ledgers would be a measurement,
    /// and there is no ledger to measure against yet. The low end avoids
    /// 0.0, where parsimony charges nothing and the full cover set wins
    /// by construction.
    fn default() -> Self {
        LambdaSweep {
            low: 0.02,
            high: 0.50,
            steps: 7,
        }
    }
}

impl LambdaSweep {
    /// The lambda values, lowest first.
    fn values(&self) -> Vec<f64> {
        if self.steps <= 1 {
            return vec![self.low];
        }
        let span = self.high - self.low;
        (0..self.steps)
            .map(|i| self.low + span * (i as f64) / ((self.steps - 1) as f64))
            .collect()
    }
}

/// The contiguous run of lambda values a claim set held together across.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LambdaBand {
    pub low: f64,
    pub high: f64,
    /// How many swept values the run covers, compared against
    /// `min_band`, and reported so a reader can see how close a kept
    /// set came to being dropped.
    pub steps: usize,
}

/// A claim set that survived a lambda band.
#[derive(Debug, Clone, PartialEq)]
pub struct StableHypothesis {
    /// Sorted claims.
    pub claims: Vec<Claim>,
    /// The lambda band the set survived.
    pub band: LambdaBand,
    /// Fitness at the low end of the band.
    pub fitness: f64,
}

/// Stable hypotheses, and everything needed to know what they are worth.
#[derive(Debug, Clone)]
pub struct HypothesisReport {
    pub hypotheses: Vec<StableHypothesis>,
    /// The range swept, so the bands above can be read.
    pub swept: LambdaSweep,
    /// The band length a set had to survive to be kept.
    pub min_band: usize,
    /// How many rows went in. A reader needs this to tell "no
    /// hypotheses" from "nothing to look at".
    pub rows_considered: usize,
    /// How many distinct condition atoms the rows held.
    pub vocabulary_len: usize,
    /// Non-empty claim sets that appeared at some lambda but never
    /// across a long enough band. Counted rather than dropped silently.
    pub discarded_as_unstable: usize,
    /// The search is stochastic and seeded; without these the result is
    /// not reproducible, and reproducible is the whole point.
    pub seed: u64,
    pub islands: usize,
    pub params: erbga::GaParams,
}

/// Search parameters for the hypothesis problem.
///
/// Smaller than the graph thesis values because the genome is small,
/// and starting near-empty because the parsimony term means a good
/// hypothesis is sparse. The repair fields are carried but unused: this
/// problem's repair is the trait's no-op.
fn hypothesis_params() -> erbga::GaParams {
    erbga::GaParams {
        population_size: 60,
        generations: 200,
        initial_one_rate: 0.05,
        ..erbga::GaParams::thesis()
    }
}

/// Sweep lambda and report the claim sets that survive a band.
pub fn search(rows: &[ExperimentRow], seed: u64) -> HypothesisReport {
    search_with(rows, LambdaSweep::default(), 2, &hypothesis_params(), 4, seed)
}

/// The sweep with everything explicit, for tests and callers that need
/// smaller runs.
pub fn search_with(
    rows: &[ExperimentRow],
    swept: LambdaSweep,
    min_band: usize,
    params: &erbga::GaParams,
    islands: usize,
    seed: u64,
) -> HypothesisReport {
    let vocabulary = Vocabulary::from_rows(rows);
    let empty = |discarded| HypothesisReport {
        hypotheses: Vec::new(),
        swept,
        min_band,
        rows_considered: rows.len(),
        vocabulary_len: vocabulary.len(),
        discarded_as_unstable: discarded,
        seed,
        islands,
        params: params.clone(),
    };
    if rows.is_empty() || vocabulary.is_empty() {
        return empty(0);
    }

    let values = swept.values();
    let mut per_lambda: Vec<(f64, Vec<Claim>, f64)> = Vec::with_capacity(values.len());
    for &lambda in &values {
        let fit = LedgerFit {
            rows,
            vocabulary: &vocabulary,
            lambda,
        };
        let results = erbga::run_islands_on(&fit, params, islands, seed);
        let best = erbga::best_of(&results);
        per_lambda.push((lambda, vocabulary.decode(&best.chromosome), best.fitness));
    }

    // Group contiguous runs of identical claim sets.
    let mut hypotheses = Vec::new();
    let mut discarded = 0usize;
    let mut start = 0usize;
    while start < per_lambda.len() {
        let mut end = start;
        while end + 1 < per_lambda.len() && per_lambda[end + 1].1 == per_lambda[start].1 {
            end += 1;
        }
        let steps = end - start + 1;
        let claims = &per_lambda[start].1;
        if !claims.is_empty() {
            if steps >= min_band {
                hypotheses.push(StableHypothesis {
                    claims: claims.clone(),
                    band: LambdaBand {
                        low: per_lambda[start].0,
                        high: per_lambda[end].0,
                        steps,
                    },
                    fitness: per_lambda[start].2,
                });
            } else {
                discarded += 1;
            }
        }
        start = end + 1;
    }

    let mut report = empty(discarded);
    report.hypotheses = hypotheses;
    report
}
```

Remove any `#[allow(dead_code)]` added in Task 3.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p zorp-track hypotheses`
Expected: PASS, 12 tests.

- [ ] **Step 5: Commit**

```bash
git add zorp-track/src/hypotheses.rs
git commit -m "feat(track): lambda is swept, never chosen, and the band is the report"
```

---

### Task 5: The planted-structure validation gate

**Files:**
- Modify: `zorp-track/src/hypotheses.rs` (tests module only)

**Interfaces:**
- Consumes: Task 4's `search_with`, `LambdaSweep`, and Task 2's helpers; `erbga::Rng` for the deterministic shuffle.
- Produces: the validation gate from the spec, as passing tests. Nothing downstream consumes these.

- [ ] **Step 1: Write the planted fixtures and the crisp recovery test**

```rust
    /// Small parameters so the whole suite stays fast. The genome here
    /// is a few dozen bits; the thesis values are sized for graphs with
    /// thousands of edges.
    fn planted_params() -> erbga::GaParams {
        erbga::GaParams {
            population_size: 40,
            generations: 120,
            initial_one_rate: 0.05,
            ..erbga::GaParams::thesis()
        }
    }

    fn planted_search(rows: &[ExperimentRow], seed: u64) -> HypothesisReport {
        search_with(rows, LambdaSweep::default(), 2, &planted_params(), 2, seed)
    }

    /// Twelve anomalous rows, all carrying harness=b and deviating
    /// above, and twelve unremarkable rows carrying harness=a, with the
    /// other atoms cycling identically through both halves. The planted
    /// truth is the single claim (harness=b, Above); every other atom
    /// appears on both sides, so claiming it costs.
    fn crisp_rows() -> Vec<ExperimentRow> {
        let contexts = ["short", "long", "wide"];
        let matchers = ["exact", "fuzzy"];
        let mut rows = Vec::new();
        for i in 0..12 {
            for (harness, outcome) in
                [("b", Some(Direction::Above)), ("a", None)]
            {
                rows.push(row(
                    &[
                        ("harness", harness),
                        ("context", contexts[i % 3]),
                        ("matcher", matchers[i % 2]),
                    ],
                    outcome,
                ));
            }
        }
        rows
    }

    fn planted_claim() -> Claim {
        Claim {
            key: "harness".to_string(),
            value: "b".to_string(),
            direction: Direction::Above,
        }
    }

    /// The validation gate, part one: exact recovery on crisp plants,
    /// across seeds.
    #[test]
    fn recovers_a_crisp_plant_exactly_across_seeds() {
        let rows = crisp_rows();
        for seed in [1, 2, 3] {
            let report = planted_search(&rows, seed);
            assert_eq!(
                report.hypotheses.len(),
                1,
                "seed {seed}: expected one stable set, got {:?}",
                report.hypotheses
            );
            assert_eq!(report.hypotheses[0].claims, vec![planted_claim()]);
        }
    }

    /// The same plant under a wider vocabulary: extra condition keys
    /// whose values cycle identically through both halves, so every new
    /// atom is uninformative and the genome is several times larger.
    #[test]
    fn recovers_a_crisp_plant_under_a_wider_vocabulary() {
        let extras = [
            ("runner", ["r1", "r2", "r3", "r4"].as_slice()),
            ("region", ["us", "eu", "ap"].as_slice()),
            ("tier", ["hot", "cold"].as_slice()),
        ];
        // Cycle by pair index, not row index: rows alternate anomalous
        // and normal, so cycling by row index would hand even-cycle
        // atoms to one side only and plant a confound by accident.
        let rows: Vec<ExperimentRow> = crisp_rows()
            .into_iter()
            .enumerate()
            .map(|(i, mut r)| {
                let pair = i / 2;
                for (key, values) in extras {
                    r.conditions
                        .push((key.to_string(), values[pair % values.len()].to_string()));
                }
                r
            })
            .collect();
        for seed in [1, 2] {
            let report = planted_search(&rows, seed);
            assert_eq!(
                report.hypotheses.len(),
                1,
                "seed {seed}: expected one stable set, got {:?}",
                report.hypotheses
            );
            assert_eq!(report.hypotheses[0].claims, vec![planted_claim()]);
        }
    }
```

- [ ] **Step 2: Run it**

Run: `cargo test -p zorp-track recovers_a_crisp_plant`
Expected: PASS. If it fails, the algorithm from Tasks 3 and 4 does not clear its own gate: debug the search, never loosen the assertion. The planted optimum is provably the fitness maximum at every swept lambda (score (12 - lambda) / 24 with every alternative strictly lower), so a failure is a search bug or a genome-size parameter problem, not an impossible test.

- [ ] **Step 3: Write the noisy plant and permutation null test**

```rust
    /// The crisp rows with two anomaly labels flipped off and two
    /// unremarkable rows flipped on, deterministically.
    fn noisy_rows() -> Vec<ExperimentRow> {
        let mut rows = crisp_rows();
        rows[0].outcome = None; // was harness=b, Above
        rows[6].outcome = None; // was harness=b, Above
        rows[1].outcome = Some(Direction::Above); // was harness=a, unremarkable
        rows[9].outcome = Some(Direction::Above); // was harness=a, unremarkable
        rows
    }

    /// The validation gate, part two: on a noisy plant the fitted
    /// structure must beat a permutation null. Outcomes are shuffled
    /// across rows, keeping the multiset, and the best fitness on real
    /// data must exceed every shuffled best across ten shuffles.
    #[test]
    fn beats_the_permutation_null_on_a_noisy_plant() {
        let rows = noisy_rows();
        let real = planted_search(&rows, 5);
        let real_best = real
            .hypotheses
            .iter()
            .map(|h| h.fitness)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(real_best.is_finite(), "the noisy plant produced no stable set");

        let mut rng = erbga::Rng::new(11);
        for shuffle in 0..10 {
            let mut outcomes: Vec<Option<Direction>> =
                rows.iter().map(|r| r.outcome).collect();
            // Fisher-Yates with the in-crate RNG, so the null is seeded.
            for i in (1..outcomes.len()).rev() {
                let j = rng.below((i + 1) as u64) as usize;
                outcomes.swap(i, j);
            }
            let shuffled: Vec<ExperimentRow> = rows
                .iter()
                .zip(outcomes)
                .map(|(r, outcome)| ExperimentRow {
                    conditions: r.conditions.clone(),
                    outcome,
                })
                .collect();
            let null = planted_search(&shuffled, 5);
            let null_best = null
                .hypotheses
                .iter()
                .map(|h| h.fitness)
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                real_best > null_best,
                "shuffle {shuffle}: null fitness {null_best} reached real {real_best}"
            );
        }
    }
```

Note: a shuffle with no stable set gives `null_best` of negative infinity, which the real best beats, and that is the correct reading: the null found nothing.

- [ ] **Step 4: Write the no-plant and determinism tests**

```rust
    /// The validation gate, part three: rows with outcomes assigned
    /// independently of conditions must report no stable set. The
    /// analog of a band too thin to judge being its own no-go.
    #[test]
    fn reports_nothing_when_nothing_was_planted() {
        let contexts = ["short", "long", "wide"];
        let matchers = ["exact", "fuzzy"];
        let harnesses = ["a", "b"];
        let mut rng = erbga::Rng::new(23);
        let rows: Vec<ExperimentRow> = (0..40)
            .map(|i| {
                let outcome = match rng.below(4) {
                    0 => Some(Direction::Above),
                    1 => Some(Direction::Below),
                    _ => None,
                };
                row(
                    &[
                        ("harness", harnesses[i % 2]),
                        ("context", contexts[i % 3]),
                        ("matcher", matchers[i % 2]),
                    ],
                    outcome,
                )
            })
            .collect();
        let report = planted_search(&rows, 3);
        assert!(
            report.hypotheses.is_empty(),
            "unplanted rows produced {:?}",
            report.hypotheses
        );
    }

    #[test]
    fn the_same_seed_gives_the_same_report() {
        let rows = crisp_rows();
        let a = planted_search(&rows, 9);
        let b = planted_search(&rows, 9);
        assert_eq!(a.hypotheses, b.hypotheses);
        assert_eq!(a.discarded_as_unstable, b.discarded_as_unstable);
    }
```

If `reports_nothing_when_nothing_was_planted` fails on its fixed seed, first inspect the reported set: if the seeded outcome assignment happened to correlate with an atom, change the fixture seed 23 once, note it in the test, and commit the choice. If no nearby seed passes, the sweep is not filtering unstable sets and that is a real bug in Task 4.

- [ ] **Step 5: Run the whole module's suite**

Run: `cargo test -p zorp-track hypotheses`
Expected: PASS, 17 tests.

- [ ] **Step 6: Commit**

```bash
git add zorp-track/src/hypotheses.rs
git commit -m "test(track): the planted-structure suite is the validation gate, and it passes"
```

---

### Task 6: The decision entry, the full suite, and the PR

**Files:**
- Modify: `docs/DECISIONS.md` (new entry at the top of the entries, above the 2026-08-24 compose entry at line 15)

**Interfaces:**
- Consumes: everything above, finished and committed.
- Produces: the PR.

- [ ] **Step 1: Add the decision entry**

Insert above the `## 2026-08-24: one compose stack, extended, with an Ollama sidecar` heading:

```markdown
## 2026-08-28: hypothesis search moves to Gated, and the real ledger stays out of reach

**Decision:** the structured-genome hypothesis search that the
2026-08-19 design left Proposed is built, in
`zorp-track/src/hypotheses.rs`, on a new representation-agnostic
`Problem` trait in `erbga`. It is validated against synthetic ledgers
with planted structure and against nothing else. No code reads the real
anomaly ledger into it; the adapter does not exist. The admission gate:
hypothesis search may run on real data only once the ledger holds at
least 12 admitted anomalies spanning at least 3 distinct condition
keys, and crossing that gate is a person's decision, recorded here when
it happens. In the registry of the 2026-08-19 spec, the entry moves
from Proposed to Gated; that spec stays as written.

**Why:** the registry admits an idea on evidence, and the evidence for
running on real data has not arrived, because the ledger is empty. What
could be admitted now is the algorithm itself, so that is what was
built and gated. Three choices carry the weight. The vocabulary is
condition atoms, key and value pairs, not bare keys, because a real
ledger records the same keys on every row with differing values, so the
key set separates nothing. The parsimony weight lambda is swept and
never chosen, the same discipline `families` applies to theta, and only
claim sets stable across a band are reported. And the erbga refactor is
pinned: a seeded run is asserted bit-identical across the trait
extraction, because the four benchmarks certify the graph path under
its recorded seeds and a reordered RNG draw would quietly retire that
certification. The trait's own documentation says the benchmarks
certify nothing about any other representation; hypothesis search's
validation is its planted-structure tests, which certify it on
synthetic ledgers only. 12 matches the minimum support every
non-habituation boredom detector already uses, for the same reason: a
structure fitted to fewer rows is arithmetic about those rows.
```

- [ ] **Step 2: Format and run the full gauntlet**

```bash
cargo fmt --all
cargo build --workspace
cargo test --workspace
cargo test -p erbga -p zorp-track
```
Expected: everything passes and `git status` shows only `docs/DECISIONS.md` modified (fmt should change nothing if earlier commits were formatted).

- [ ] **Step 3: Commit the entry**

```bash
git add docs/DECISIONS.md
git commit -m "docs: record the gate, and why the real ledger stays out of reach"
```

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin feat/hypothesis-search
gh pr create --base main --title "feat(track): hypothesis search, gated, on an erbga substrate" --body "$(cat <<'EOF'
Builds the structured-genome hypothesis search the 2026-08-19 design left Proposed, and moves it to Gated.

- erbga gains a representation-agnostic `Problem` trait; the graph path delegates through it unchanged, pinned by a seeded-equivalence test so the benchmark certification survives the refactor.
- `zorp-track/src/hypotheses.rs`: a hypothesis is a bitmask over (condition atom, direction) pairs, fitness is arithmetic against caller-supplied rows, and lambda is swept the way `families` sweeps theta, with only claim sets stable across a band reported.
- Validation gate satisfied in-tree: exact recovery of crisp plants across seeds, a permutation null beaten on noisy plants, and silence on unplanted rows.
- Admission gate stated in DECISIONS.md: no code touches the real anomaly ledger until it holds at least 12 admitted anomalies over at least 3 distinct condition keys, by a person's recorded decision.

Spec: docs/superpowers/specs/2026-08-28-hypothesis-search-design.md
Plan: docs/superpowers/plans/2026-08-28-hypothesis-search.md

https://claude.ai/code/session_01NwHQyG7XFVHHwskEi8VsLw
EOF
)"
```
