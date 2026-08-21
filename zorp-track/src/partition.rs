//! aryabhatta step 5: the search layer and its confounded-condition caller.
//!
//! The design has two readers that need to partition a graph: these
//! confounded condition bundles, and the anomaly families that come with
//! the ledger later. So community detection enters the subsystem once,
//! as one interface with two backends chosen by graph size, rather than
//! twice. Only the first caller is built here. It reads the `conditions`
//! table, sits on the ungated side, and needs no anomaly ledger.
//!
//! **What a bundle claims.** Two condition keys are on an edge when,
//! across the experiments recording both, they never varied
//! independently: no run pair changed one and held the other. A bundle
//! is a connected group of those, and it says no observed effect can be
//! attributed to any single member alone. That is aliasing in the design
//! of experiments sense. It is also what `invariant_condition` cannot
//! see, because that detector looks at one key at a time and a pair that
//! always moves together has two keys that both vary.
//!
//! A bundle is a transitive claim. If `a` is locked to `b` and `b` to
//! `c`, all three land in one bundle even where `a` and `c` were never
//! recorded together. That follows from the relation when the support
//! sets line up, and it is an inference through `b` when they do not.
//!
//! **The measurement is the point.** Open question 7 asks whether this
//! relation is crisp or graded, and says to answer it before leaning on
//! a search. So [`CrispnessReport`] comes back with every result, as a
//! value and not as a log line. If pairs only ever sit at the ends, the
//! bundles are connected components, union-find is exact, and the search
//! earns nothing here. If pairs sit in the middle, where the line falls
//! is a real trade-off.
//!
//! **What is claimed of erbga, and what is not.** The search backend is
//! `erbga`, an implementation of published prior work validated against
//! that work's four benchmark networks. Those benchmarks certify ERBGA
//! on graphs. This caller hands it a graph, so using it is legitimate.
//! They certify nothing about condition keys, and a bundle this module
//! reports is not a validated result. Nothing here should be described
//! that way.
//!
//! **Integrity rules.** Rule 7: only code-visible columns are read, and
//! [`CONDITIONS_QUERY`] is the whole of this module's SQL. Rule 8: a
//! partition from the search backend comes back with the seed, island
//! count and parameters that produced it. Rule 6, which forbids
//! reporting a partition at a single θ, does not bite this caller: its
//! edge is a count of zero independent variations, which is where the
//! definition of independence puts it and not a cutoff anyone chose. If
//! the measurement comes back graded, a tolerance appears, and then rule
//! 6 applies and a sweep is owed. That is not built here.
//!
//! Everything in this module reads. Nothing in it writes.
//!
//! **Cost.** Every pair of keys is weighed, and each pair walks every
//! pair of experiments recording both, so this is quadratic in keys and
//! quadratic in experiments. Both are small at zorp's scale, and the
//! quadratic term is arithmetic on values already in memory.

use crate::track::Store;
use crate::TrackError;
use std::collections::BTreeMap;

/// The columns this module is allowed to name.
///
/// Integrity rule 7: nothing in the search layer reads a column holding
/// model-authored text. The value columns are compared for equality and
/// never read as language, which is why `value_string` is here at all.
const CONDITIONS_QUERY: &str = "SELECT experiment_id, condition_key, value_type, \
     value_number, value_string, value_bool \
     FROM conditions ORDER BY experiment_id, seq";

/// Where the exact backend stops and `erbga` starts, counted in
/// vertices, which here is condition keys.
///
/// Two things set it. The exact backend below is connected components by
/// union-find, which is near linear, and `docs/DECISIONS.md` 2026-08-15
/// records that even the much harder exact formulation, a
/// clique-partitioning ILP, solves this at `V = 20` in about 0.2
/// seconds. So 64 is chosen well inside the range where the exact answer
/// is instant, erring toward exactness rather than toward search.
///
/// It is a guess, and the design says so. Open question 6 is where the
/// crossover actually sits, and it says the number should be measured on
/// real condition graphs rather than picked here. Nothing has measured
/// it yet, so treat this as a placeholder with a reason and not as a
/// finding.
///
/// One honest caveat on the shape of the rule. For a relation that turns
/// out crisp, components are the exact answer at every size, not only
/// below the crossover, so the crossover only starts to bite once the
/// crispness measurement comes back graded and the components collapse
/// into one blob. Choosing by `|V|` is what the design asks for, and it
/// is right whenever the graph is dense enough for that to happen.
pub const EXACT_MAX_VERTICES: usize = 64;

/// The default seed for the search backend.
///
/// A fixed number rather than a clock or an RNG, so an unparameterized
/// call is reproducible by default. It is recorded next to the partition
/// either way.
pub const DEFAULT_SEED: u64 = 0x7A0B_9E11_4C3D_2F55;

/// Which backend to run.
#[derive(Clone, Debug, Default)]
pub enum Backend {
    /// Pick by `|V|` against [`EXACT_MAX_VERTICES`].
    #[default]
    Auto,
    /// Connected components. Proven optimal for this relation, instant.
    Exact,
    /// `erbga` searches for a modularity maximum.
    Search(SearchSettings),
}

/// Everything the search backend needs, and everything a reader needs to
/// run it again and get the same answer.
#[derive(Clone, Debug)]
pub struct SearchSettings {
    pub seed: u64,
    pub islands: usize,
    pub params: erbga::GaParams,
}

impl Default for SearchSettings {
    /// The published protocol: 25 islands at the thesis parameters.
    ///
    /// Not tuned here. Tuning it against zorp's own graphs would be a
    /// measurement, and there is no corpus to measure against yet.
    fn default() -> Self {
        SearchSettings {
            seed: DEFAULT_SEED,
            islands: 25,
            params: erbga::GaParams::thesis(),
        }
    }
}

/// Which backend actually ran, and on what terms.
#[derive(Clone, Debug)]
pub enum BackendRecord {
    Exact,
    Search(SearchRecord),
}

/// Integrity rule 8: a partition carries what produced it.
///
/// `erbga` is stochastic and seeded, so without the seed, the island
/// count and the parameters, a recorded clustering is not a result
/// anyone can check.
#[derive(Clone, Debug)]
pub struct SearchRecord {
    pub seed: u64,
    pub islands: usize,
    pub params: erbga::GaParams,
    /// The modularity of the partition that came back. Reported so a
    /// later run can be compared to this one, and so a result that beats
    /// a known optimum is visible as the bug it would be.
    pub modularity: f64,
}

/// One recorded condition value, reduced to something comparable.
///
/// Only equality is ever asked of it. `f64` is held as its bit pattern
/// so the enum can be `Eq`, with the two values that have more than one
/// encoding folded first: `-0.0` compares equal to `0.0` and every NaN
/// is one NaN. Without that, two runs recording the same number could
/// look like a change.
#[derive(Clone, PartialEq, Eq, Debug)]
enum ValueKey {
    Number(u64),
    Text(String),
    Bool(bool),
    Missing,
}

impl ValueKey {
    fn number(n: f64) -> Self {
        let folded = if n == 0.0 {
            0.0
        } else if n.is_nan() {
            f64::NAN
        } else {
            n
        };
        ValueKey::Number(folded.to_bits())
    }
}

/// Whether the co-variation relation is a clean split or a spectrum.
///
/// This is open question 7 of the design, and it decides whether the
/// search layer earns its place on this caller at all. Crisp means every
/// measured pair sits at one end: it never varied independently, or it
/// never varied together. Then the bundles are connected components,
/// union-find answers them exactly, and a genetic algorithm has nothing
/// to win. Graded means at least one pair is in the middle, say
/// independent twice in forty, and then where the line falls is a real
/// trade-off rather than a lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Crisp,
    Graded,
}

/// The answer to open question 7, measured rather than assumed.
///
/// A first class result, not a log line. The counts are what a reader
/// needs to tell "crisp" from "nothing was measured", which look the
/// same if only the verdict is reported.
#[derive(Clone, Debug)]
pub struct CrispnessReport {
    pub verdict: Relation,
    /// Pairs that cleared `min_support` and were seen to vary at all.
    pub pairs_measured: usize,
    /// Pairs that varied together and never independently. These are
    /// the edges of the co-variation graph.
    pub locked: usize,
    /// Pairs that varied independently and never together.
    pub free: usize,
    /// Pairs that did both. Carried whole rather than counted, because
    /// if the relation is graded these are the evidence for where a
    /// threshold would have to go.
    pub graded: Vec<PairEvidence>,
    /// Pairs that cleared `min_support` but never varied at all. Held
    /// apart from the verdict: two conditions that never move carry no
    /// evidence about whether they move together, and they are already
    /// the `invariant_condition` detector's business.
    pub unvaried: usize,
}

impl CrispnessReport {
    /// True when nothing sits in the middle.
    pub fn is_crisp(&self) -> bool {
        matches!(self.verdict, Relation::Crisp)
    }
}

/// Bundles of mutually confounded condition keys, with the measurement
/// that says whether the relation they came from is worth searching.
#[derive(Clone, Debug)]
pub struct ConfoundedConditions {
    /// Each bundle is sorted, and the bundles are sorted among
    /// themselves. Singletons are dropped: one key alone is not a
    /// confound, it is a key.
    pub bundles: Vec<Vec<String>>,
    /// Whether the relation these bundles came out of is crisp.
    pub crispness: CrispnessReport,
    /// The support floor the caller asked for, carried so a result can
    /// be read without the call that produced it.
    pub min_support: usize,
    /// Which backend produced the bundles, and its seed and parameters
    /// if it was the stochastic one.
    pub backend: BackendRecord,
}

impl Store {
    /// Condition keys that never varied independently of each other.
    ///
    /// A bundle means no observed effect can be attributed to any single
    /// member alone. That is aliasing in the design of experiments
    /// sense. `min_support` is the number of experiments a pair has to
    /// have been recorded together in before the pair is judged at all.
    ///
    /// Reads only.
    pub fn confounded_conditions(
        &self,
        min_support: usize,
    ) -> Result<ConfoundedConditions, TrackError> {
        self.confounded_conditions_with(min_support, Backend::Auto)
    }

    /// [`Store::confounded_conditions`] with the backend named.
    ///
    /// `Backend::Auto` is what ordinary callers want. Naming a backend
    /// is for the band where both can run, where the exact answer is a
    /// standing regression check on the search.
    ///
    /// Reads only.
    pub fn confounded_conditions_with(
        &self,
        min_support: usize,
        backend: Backend,
    ) -> Result<ConfoundedConditions, TrackError> {
        let observed = self.read_conditions()?;
        let keys = distinct_keys(&observed);
        let (edges, crispness) = measure(&observed, &keys, min_support);
        let (groups, record) = partition_graph(keys.len(), &edges, backend);
        let bundles = groups
            .into_iter()
            .filter(|group| group.len() > 1)
            .map(|group| group.into_iter().map(|v| keys[v].clone()).collect())
            .collect();
        Ok(ConfoundedConditions {
            bundles,
            crispness,
            min_support,
            backend: record,
        })
    }

    /// Every condition row, folded to one value per experiment and key.
    ///
    /// A key recorded twice for one experiment keeps the later row: `seq`
    /// is insertion order, so the last write is the condition the run
    /// actually ran under. `BTreeMap` throughout because the output of
    /// this module has to be identical between runs, and hash iteration
    /// order is not.
    fn read_conditions(&self) -> Result<BTreeMap<String, BTreeMap<String, ValueKey>>, TrackError> {
        let mut stmt = self.conn.prepare(CONDITIONS_QUERY)?;
        let rows = stmt.query_map([], |r| {
            let experiment_id: String = r.get(0)?;
            let key: String = r.get(1)?;
            let value_type: String = r.get(2)?;
            let value = match value_type.as_str() {
                "number" => r.get::<_, Option<f64>>(3)?.map(ValueKey::number),
                "bool" => r.get::<_, Option<bool>>(5)?.map(ValueKey::Bool),
                _ => r.get::<_, Option<String>>(4)?.map(ValueKey::Text),
            };
            Ok((experiment_id, key, value.unwrap_or(ValueKey::Missing)))
        })?;
        let mut out: BTreeMap<String, BTreeMap<String, ValueKey>> = BTreeMap::new();
        for row in rows {
            let (experiment_id, key, value) = row?;
            out.entry(experiment_id).or_default().insert(key, value);
        }
        Ok(out)
    }
}

/// The vertex set: every condition key ever recorded, sorted.
///
/// Sorted rather than first-seen, so a key's vertex number depends on
/// the key alone. Both backends index into this, and the erbga one is
/// seeded, so a shifting vertex numbering would make a recorded seed
/// meaningless.
fn distinct_keys(observed: &BTreeMap<String, BTreeMap<String, ValueKey>>) -> Vec<String> {
    let mut keys: Vec<String> = observed
        .values()
        .flat_map(|row| row.keys().cloned())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// Walk every pair once, and get both things the caller needs out of it.
///
/// The edges of the co-variation graph and the crispness of the relation
/// are the same walk seen two ways, so they are counted together. An
/// edge is a pair that varied together and never apart. A pair that
/// varied both ways is not an edge, and it is also the reason a search
/// could be worth running.
///
/// Edges come out in ascending vertex order, which is what makes the
/// graph handed to either backend identical between runs.
fn measure(
    observed: &BTreeMap<String, BTreeMap<String, ValueKey>>,
    keys: &[String],
    min_support: usize,
) -> (Vec<(usize, usize)>, CrispnessReport) {
    let mut edges = Vec::new();
    let (mut locked, mut free, mut unvaried) = (0, 0, 0);
    let mut graded: Vec<PairEvidence> = Vec::new();

    for a in 0..keys.len() {
        for b in (a + 1)..keys.len() {
            let evidence = weigh_pair(observed, &keys[a], &keys[b]);
            if evidence.support < min_support {
                continue;
            }
            match (evidence.moved_together, evidence.moved_independently) {
                (0, 0) => unvaried += 1,
                (_, 0) => {
                    locked += 1;
                    edges.push((a, b));
                }
                (0, _) => free += 1,
                _ => graded.push(evidence),
            }
        }
    }

    let verdict = if graded.is_empty() {
        Relation::Crisp
    } else {
        Relation::Graded
    };
    let report = CrispnessReport {
        verdict,
        pairs_measured: locked + free + graded.len(),
        locked,
        free,
        graded,
        unvaried,
    };
    (edges, report)
}

/// How often two keys moved together and how often one moved alone.
///
/// Both counts are over unordered pairs of experiments, because "changed
/// value" is a statement about two runs and not about one. Comparing
/// consecutive experiments instead would make the answer depend on the
/// order experiments happen to have been created in.
fn weigh_pair(
    observed: &BTreeMap<String, BTreeMap<String, ValueKey>>,
    left: &str,
    right: &str,
) -> PairEvidence {
    let seen: Vec<(&ValueKey, &ValueKey)> = observed
        .values()
        .filter_map(|row| Some((row.get(left)?, row.get(right)?)))
        .collect();

    let mut moved_together = 0;
    let mut moved_independently = 0;
    for i in 0..seen.len() {
        for j in (i + 1)..seen.len() {
            let left_moved = seen[i].0 != seen[j].0;
            let right_moved = seen[i].1 != seen[j].1;
            match (left_moved, right_moved) {
                (true, true) => moved_together += 1,
                (true, false) | (false, true) => moved_independently += 1,
                (false, false) => {}
            }
        }
    }

    PairEvidence {
        left: left.to_string(),
        right: right.to_string(),
        support: seen.len(),
        moved_together,
        moved_independently,
    }
}

/// What one pair of keys was observed to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairEvidence {
    /// The lower key of the pair, by the sorted vertex order.
    pub left: String,
    /// The higher key of the pair.
    pub right: String,
    /// Experiments where both keys were recorded.
    pub support: usize,
    /// Experiment pairs where both keys changed value.
    pub moved_together: usize,
    /// Experiment pairs where exactly one of them changed value.
    pub moved_independently: usize,
}

/// The one interface, with its two backends behind it.
///
/// Groups come back sorted inside and out, whichever backend ran, so a
/// caller cannot tell them apart by shape alone. That is what makes the
/// two comparable in the band where both can run, and comparing them is
/// the point: the exact result is a continuous regression check on the
/// search against proven optimality, which four fixed benchmark networks
/// cannot give.
fn partition_graph(
    n_vertices: usize,
    edges: &[(usize, usize)],
    backend: Backend,
) -> (Vec<Vec<usize>>, BackendRecord) {
    let chosen = match backend {
        Backend::Auto if n_vertices > EXACT_MAX_VERTICES => {
            Backend::Search(SearchSettings::default())
        }
        Backend::Auto => Backend::Exact,
        named => named,
    };
    match chosen {
        Backend::Exact => (components(n_vertices, edges), BackendRecord::Exact),
        Backend::Search(settings) => search(n_vertices, edges, settings),
        Backend::Auto => unreachable!("Auto was resolved above"),
    }
}

/// The `erbga` backend.
///
/// This is the first thing in zorp to call `erbga`, permitted by
/// `docs/DECISIONS.md` 2026-08-19. It is used on a graph, which is the
/// only thing the source work's four benchmarks certify it on, and the
/// bundles it returns are not thereby validated: the benchmarks say the
/// search finds good partitions of graphs, not that a partition of this
/// graph is a true account of what confounds what.
///
/// erbga encodes a clustering as a set of removed edges, so whatever it
/// returns is a refinement of the connected components. It can split a
/// component that the exact backend keeps whole. It can never merge two
/// that the exact backend keeps apart.
///
/// Which means the two backends do not answer the same question, and
/// that is worth stating plainly rather than discovering later.
/// Confounding chains: locked to locked is locked, so the exact answer
/// is the transitive closure. Modularity rewards assortativity, and a
/// long thin chain scores better cut in half than kept whole. On a
/// locked chain the search therefore splits a real bundle, at the true
/// modularity optimum, with no bug anywhere. There is a test for it.
/// Below the crossover this cannot reach a caller, because the exact
/// backend runs. Above it the search is what there is, and a bundle it
/// reports is a floor on the confounding rather than the whole of it.
fn search(
    n_vertices: usize,
    edges: &[(usize, usize)],
    settings: SearchSettings,
) -> (Vec<Vec<usize>>, BackendRecord) {
    // With no edges the genome has no genes, so every chromosome in
    // every generation is the same empty chromosome and every partition
    // is the singletons. Searching it is millions of evaluations of one
    // constant. The result below is what the search would have returned:
    // erbga's modularity is 0.0 on a graph with no edges by its own
    // early return.
    if edges.is_empty() {
        let groups = (0..n_vertices).map(|v| vec![v]).collect();
        let record = BackendRecord::Search(SearchRecord {
            seed: settings.seed,
            islands: settings.islands,
            params: settings.params,
            modularity: 0.0,
        });
        return (groups, record);
    }

    // The edge list is built in this file, from `measure`, which emits
    // ascending pairs of in-range vertex indices and never a self loop.
    // A GraphError here would be a bug a few lines up rather than bad
    // data, so there is nothing for a caller to handle.
    let graph = erbga::Graph::new(n_vertices, edges)
        .expect("co-variation edges are in range and have no self loops");
    let islands = erbga::run_islands(
        &graph,
        &erbga::Modularity,
        &settings.params,
        settings.islands,
        settings.seed,
    );
    let best = erbga::best_of(&islands);
    let partition = graph.partition(&best.chromosome);

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for v in 0..n_vertices {
        groups.entry(partition.label(v)).or_default().push(v);
    }
    // Sorted by lowest member, matching the exact backend, so the two
    // are compared on content and not on labelling.
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();
    groups.sort();

    let record = BackendRecord::Search(SearchRecord {
        seed: settings.seed,
        islands: settings.islands,
        params: settings.params,
        modularity: best.fitness,
    });
    (groups, record)
}

/// Connected components by union-find, as sorted vertex groups.
///
/// This is the exact backend. For a relation that is crisp it is not an
/// approximation of the answer, it is the answer, and it runs in near
/// linear time. Wherever it can run beside the search, it is a standing
/// check on the search rather than a fallback.
fn components(n_vertices: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n_vertices).collect();
    fn find(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    for &(a, b) in edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            // Union by smaller root, so the representative of a group is
            // its lowest vertex and the grouping does not depend on the
            // order edges arrived in.
            let (keep, drop) = (ra.min(rb), ra.max(rb));
            parent[drop] = keep;
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for v in 0..n_vertices {
        let root = find(&mut parent, v);
        groups.entry(root).or_default().push(v);
    }
    groups.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Store;
    use tempfile::tempdir;

    /// Write one condition row directly.
    ///
    /// The fixture speaks SQL rather than calling the recording API in
    /// `conditions.rs`, so the search layer is tested against the table
    /// it reads and not against another module's code. These three
    /// helpers are the only writes anywhere in this file.
    fn put(store: &Store, experiment: &str, key: &str, value: &str) {
        let seq = crate::id::next_seq();
        let id = format!("{experiment}-{key}-{seq}");
        store
            .conn
            .execute(
                "INSERT INTO conditions (id, experiment_id, condition_key, value_type, \
                 value_number, value_string, value_bool, recorded_at, seq) \
                 VALUES (?, ?, ?, 'string', NULL, ?, NULL, 0, ?)",
                duckdb::params![id, experiment, key, value, seq as i64],
            )
            .unwrap();
    }

    fn put_number(store: &Store, experiment: &str, key: &str, value: f64) {
        let seq = crate::id::next_seq();
        let id = format!("{experiment}-{key}-{seq}");
        store
            .conn
            .execute(
                "INSERT INTO conditions (id, experiment_id, condition_key, value_type, \
                 value_number, value_string, value_bool, recorded_at, seq) \
                 VALUES (?, ?, ?, 'number', ?, NULL, NULL, 0, ?)",
                duckdb::params![id, experiment, key, value, seq as i64],
            )
            .unwrap();
    }

    fn put_bool(store: &Store, experiment: &str, key: &str, value: bool) {
        let seq = crate::id::next_seq();
        let id = format!("{experiment}-{key}-{seq}");
        store
            .conn
            .execute(
                "INSERT INTO conditions (id, experiment_id, condition_key, value_type, \
                 value_number, value_string, value_bool, recorded_at, seq) \
                 VALUES (?, ?, ?, 'bool', NULL, NULL, ?, 0, ?)",
                duckdb::params![id, experiment, key, value, seq as i64],
            )
            .unwrap();
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        (dir, store)
    }

    /// The case `invariant_condition` is blind to: both keys vary, and
    /// they only ever vary together, so no observed effect can be put
    /// down to either one alone.
    #[test]
    fn two_keys_that_always_move_together_are_bundled() {
        let (_dir, store) = store();
        for (exp, harness, matcher) in [
            ("e1", "a", "x"),
            ("e2", "b", "y"),
            ("e3", "a", "x"),
            ("e4", "b", "y"),
        ] {
            put(&store, exp, "harness", harness);
            put(&store, exp, "matcher", matcher);
        }

        let found = store.confounded_conditions(4).unwrap();

        assert_eq!(
            found.bundles,
            vec![vec!["harness".to_string(), "matcher".to_string()]]
        );
    }

    /// A fully crossed two by two design is the case the whole thing is
    /// meant to distinguish. Both keys vary, and each varies while the
    /// other is held, so an effect can be attributed to either one.
    #[test]
    fn two_keys_that_vary_independently_are_not_bundled() {
        let (_dir, store) = store();
        for (exp, harness, matcher) in [
            ("e1", "a", "x"),
            ("e2", "b", "x"),
            ("e3", "a", "y"),
            ("e4", "b", "y"),
        ] {
            put(&store, exp, "harness", harness);
            put(&store, exp, "matcher", matcher);
        }

        let found = store.confounded_conditions(4).unwrap();

        assert!(found.bundles.is_empty(), "got {:?}", found.bundles);
    }

    /// Open question 7, on a dataset where every pair sits at one end.
    /// `harness` and `matcher` only ever move together, and neither ever
    /// moves with `temperature`, which never moves at all. Nothing is in
    /// the middle, so connected components answer the question exactly
    /// and a search would earn nothing.
    #[test]
    fn a_crisp_dataset_is_reported_as_crisp() {
        let (_dir, store) = store();
        for (exp, harness, matcher) in [
            ("e1", "a", "x"),
            ("e2", "b", "y"),
            ("e3", "a", "x"),
            ("e4", "b", "y"),
        ] {
            put(&store, exp, "harness", harness);
            put(&store, exp, "matcher", matcher);
            put(&store, exp, "temperature", "0");
        }

        let found = store.confounded_conditions(4).unwrap();

        assert_eq!(found.crispness.verdict, Relation::Crisp);
        assert!(found.crispness.is_crisp());
        assert_eq!(found.crispness.pairs_measured, 3);
        assert_eq!(found.crispness.locked, 1);
        assert_eq!(found.crispness.free, 2);
        assert!(found.crispness.graded.is_empty());
    }

    /// The other half of open question 7. `harness` and `matcher` move
    /// together four times out of five, and `matcher` moves alone once.
    /// That is the middle the spec names, independent twice in forty at
    /// scale, and it is where a partition stops being a lookup.
    #[test]
    fn a_graded_dataset_is_reported_as_graded() {
        let (_dir, store) = store();
        for (exp, harness, matcher) in [
            ("e1", "a", "x"),
            ("e2", "b", "y"),
            ("e3", "a", "x"),
            ("e4", "b", "y"),
            ("e5", "b", "z"),
        ] {
            put(&store, exp, "harness", harness);
            put(&store, exp, "matcher", matcher);
        }

        let found = store.confounded_conditions(5).unwrap();

        assert_eq!(found.crispness.verdict, Relation::Graded);
        assert!(!found.crispness.is_crisp());
        assert_eq!(found.crispness.graded.len(), 1);
        let pair = &found.crispness.graded[0];
        assert_eq!(pair.left, "harness");
        assert_eq!(pair.right, "matcher");
        assert_eq!(pair.support, 5);
        assert_eq!(pair.moved_together, 6);
        assert_eq!(pair.moved_independently, 2);
        // A graded pair is not an edge, so nothing is bundled on this
        // evidence. Deciding it should be is exactly the trade-off the
        // measurement exists to expose.
        assert!(found.bundles.is_empty(), "got {:?}", found.bundles);
    }

    /// Two keys that moved together twice look perfectly confounded, and
    /// on two experiments that means nothing. Below the support floor the
    /// layer says nothing at all, and does not measure the pair either.
    #[test]
    fn a_pair_below_min_support_stays_silent() {
        let (_dir, store) = store();
        for (exp, harness, matcher) in [("e1", "a", "x"), ("e2", "b", "y")] {
            put(&store, exp, "harness", harness);
            put(&store, exp, "matcher", matcher);
        }

        let found = store.confounded_conditions(3).unwrap();
        assert!(found.bundles.is_empty(), "got {:?}", found.bundles);
        assert_eq!(found.crispness.pairs_measured, 0);
        assert_eq!(found.min_support, 3);

        // The same data one experiment above the floor does speak, so
        // the silence above is the floor and not an empty read.
        let found = store.confounded_conditions(2).unwrap();
        assert_eq!(
            found.bundles,
            vec![vec!["harness".to_string(), "matcher".to_string()]]
        );
        assert_eq!(found.crispness.pairs_measured, 1);
    }

    /// Six keys in two groups of three. Inside a group the three keys
    /// share one schedule, so every within-group pair is locked. The two
    /// groups are fully crossed against each other, so every cross pair
    /// varied both ways and is not an edge. The co-variation graph is
    /// two disjoint triangles.
    fn two_bundles(store: &Store) {
        let rows = [
            ("e1", "p", "r"),
            ("e2", "q", "r"),
            ("e3", "p", "s"),
            ("e4", "q", "s"),
        ];
        for (exp, first, second) in rows {
            for key in ["a1", "a2", "a3"] {
                put(store, exp, key, first);
            }
            for key in ["b1", "b2", "b3"] {
                put(store, exp, key, second);
            }
        }
    }

    /// The two triangles as erbga sees them, for the brute force below.
    fn two_triangles() -> erbga::Graph {
        erbga::Graph::new(6, &[(0, 1), (0, 2), (1, 2), (3, 4), (3, 5), (4, 5)]).unwrap()
    }

    /// The best modularity on a graph, by exhaustion over every
    /// chromosome. Only used on graphs of a handful of edges, where
    /// enumeration settles the optimum, which is what makes the
    /// assertions below one sided against a proven optimum rather than
    /// against another search.
    fn best_modularity(g: &erbga::Graph) -> f64 {
        use erbga::{Chromosome, Objective};
        let m = g.edge_count();
        let mut best = f64::NEG_INFINITY;
        for mask in 0..(1u32 << m) {
            let mut c = Chromosome::zeros(m);
            for i in 0..m {
                c.set(i, mask & (1 << i) != 0);
            }
            best = best.max(erbga::Modularity.score(g, &g.partition(&c)));
        }
        best
    }

    fn small_search(seed: u64) -> SearchSettings {
        SearchSettings {
            seed,
            islands: 4,
            params: erbga::GaParams {
                population_size: 40,
                generations: 40,
                ..erbga::GaParams::thesis()
            },
        }
    }

    /// The regression check the spec asks for. Where both backends can
    /// run, the exact one is a standing check on the search, and it can
    /// give that on any graph, where the four fixed benchmark networks
    /// cannot.
    #[test]
    fn the_exact_and_the_erbga_backend_agree_on_a_small_graph() {
        let (_dir, store) = store();
        two_bundles(&store);

        let exact = store.confounded_conditions_with(4, Backend::Exact).unwrap();
        let searched = store
            .confounded_conditions_with(4, Backend::Search(small_search(7)))
            .unwrap();

        assert_eq!(
            exact.bundles,
            vec![
                vec!["a1".to_string(), "a2".to_string(), "a3".to_string()],
                vec!["b1".to_string(), "b2".to_string(), "b3".to_string()],
            ]
        );
        assert_eq!(searched.bundles, exact.bundles);
        assert!(matches!(exact.backend, BackendRecord::Exact));

        let BackendRecord::Search(record) = &searched.backend else {
            panic!("forcing Search should record a search");
        };
        // Integrity rule 8: the partition comes back with what produced it.
        assert_eq!(record.seed, 7);
        assert_eq!(record.islands, 4);
        assert_eq!(record.params.population_size, 40);
        assert_eq!(record.params.generations, 40);

        // The fixture is graded, because the two groups are fully
        // crossed against each other and every cross pair varied both
        // ways. Six locked pairs inside the groups, nine graded across.
        assert_eq!(exact.crispness.verdict, Relation::Graded);
        assert_eq!(exact.crispness.locked, 6);
        assert_eq!(exact.crispness.graded.len(), 9);

        // One sided, the way erbga's own karate club assertion is. A
        // search that beats a proven optimum has found a bug in the
        // objective, not a better partition.
        let optimum = best_modularity(&two_triangles());
        assert!(
            record.modularity <= optimum + 1e-9,
            "search scored {} above the proven optimum {optimum}",
            record.modularity
        );
        assert!(
            record.modularity >= optimum - 1e-9,
            "search scored {} below the proven optimum {optimum}",
            record.modularity
        );
    }

    /// Six keys in a chain. `k1` and `k2` are recorded together and
    /// only together, then `k2` and `k3`, and so on, so each
    /// consecutive pair is locked and no other pair has any support at
    /// all. The co-variation graph is a path of six vertices.
    fn locked_chain(store: &Store) {
        for link in 1..6 {
            let (lower, upper) = (format!("k{link}"), format!("k{}", link + 1));
            for (run, value) in [(1, "p"), (2, "q")] {
                let exp = format!("e{link}{run}");
                put(store, &exp, &lower, value);
                put(store, &exp, &upper, value);
            }
        }
    }

    /// The caveat the agreement test does not cover, made concrete.
    ///
    /// The relation here is crisp and the answer is one bundle of six:
    /// every consecutive pair is locked, and confounding chains. The
    /// search does not return that, because modularity rewards
    /// assortativity and a long thin chain scores better cut in half.
    /// Both backends are behaving correctly and they are answering
    /// different questions, which is worth knowing before a graph large
    /// enough to force the search shows up.
    #[test]
    fn on_a_locked_chain_the_search_splits_what_the_exact_backend_keeps_whole() {
        let (_dir, store) = store();
        locked_chain(&store);

        let exact = store.confounded_conditions_with(2, Backend::Exact).unwrap();
        let searched = store
            .confounded_conditions_with(2, Backend::Search(small_search(3)))
            .unwrap();

        assert_eq!(exact.crispness.verdict, Relation::Crisp);
        assert_eq!(exact.crispness.locked, 5);
        assert_eq!(
            exact.bundles,
            vec![(1..=6).map(|i| format!("k{i}")).collect::<Vec<String>>()]
        );

        assert!(
            searched.bundles.len() > 1,
            "modularity should cut this chain, got {:?}",
            searched.bundles
        );
        // And it is not the search failing. It found the modularity
        // optimum, which is a different thing from the answer.
        let path = erbga::Graph::new(6, &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]).unwrap();
        let optimum = best_modularity(&path);
        let BackendRecord::Search(record) = &searched.backend else {
            panic!("forcing Search should record a search");
        };
        assert!(
            (record.modularity - optimum).abs() < 1e-9,
            "search scored {} against a proven optimum of {optimum}",
            record.modularity
        );
        assert!(optimum > 0.0, "the split really does score better");
    }

    /// Deliberately too weak to converge: one island, four individuals,
    /// no generations. What comes back is the best of a seeded random
    /// draw, which is what makes the seed visible in the result.
    fn weak_search(seed: u64) -> SearchSettings {
        SearchSettings {
            seed,
            islands: 1,
            params: erbga::GaParams {
                population_size: 4,
                generations: 0,
                ..erbga::GaParams::thesis()
            },
        }
    }

    fn modularity_of(found: &ConfoundedConditions) -> f64 {
        match &found.backend {
            BackendRecord::Search(record) => record.modularity,
            BackendRecord::Exact => panic!("expected a search record"),
        }
    }

    /// Integrity rule 8 is only worth anything if the recorded seed
    /// actually reproduces the partition.
    #[test]
    fn the_same_seed_reproduces_the_same_partition() {
        let (_dir, store) = store();
        two_bundles(&store);

        let first = store
            .confounded_conditions_with(4, Backend::Search(weak_search(11)))
            .unwrap();
        let second = store
            .confounded_conditions_with(4, Backend::Search(weak_search(11)))
            .unwrap();

        assert_eq!(first.bundles, second.bundles);
        // Bit equality, not an epsilon. Two runs of the same seed are
        // the same arithmetic in the same order, so anything less exact
        // would hide a real difference.
        assert_eq!(
            modularity_of(&first).to_bits(),
            modularity_of(&second).to_bits()
        );

        // And the seed is what did it. Without this the assertion above
        // could be a search that converges from anywhere, which would
        // prove nothing about reproducibility.
        let scores: Vec<u64> = (0..12)
            .map(|seed| {
                let found = store
                    .confounded_conditions_with(4, Backend::Search(weak_search(seed)))
                    .unwrap();
                modularity_of(&found).to_bits()
            })
            .collect();
        let mut distinct = scores.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() > 1,
            "every seed gave the same result, so the seed is not reaching the search"
        );
    }

    /// The backend is chosen by `|V|`, and the crossover is the constant
    /// rather than a number written twice.
    #[test]
    fn auto_switches_backend_at_the_crossover() {
        let (_, at_the_line) = partition_graph(EXACT_MAX_VERTICES, &[], Backend::Auto);
        assert!(matches!(at_the_line, BackendRecord::Exact));

        let (_, past_it) = partition_graph(EXACT_MAX_VERTICES + 1, &[], Backend::Auto);
        assert!(matches!(past_it, BackendRecord::Search(_)));
    }

    /// Integrity rule 7. The search layer reads code-visible columns and
    /// nothing a model wrote. This is the whole of the layer's SQL, so
    /// checking it is checking the rule.
    #[test]
    fn the_query_names_no_model_authored_column() {
        let allowed = [
            "experiment_id",
            "condition_key",
            "value_type",
            "value_number",
            "value_string",
            "value_bool",
            "conditions",
            "seq",
        ];
        for word in CONDITIONS_QUERY
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|w| !w.is_empty())
        {
            let keyword = matches!(
                word.to_ascii_uppercase().as_str(),
                "SELECT" | "FROM" | "ORDER" | "BY"
            );
            assert!(
                keyword || allowed.contains(&word),
                "the search layer's query names {word:?}, which is not on its allowlist"
            );
        }

        // The columns that hold model-authored text live on other
        // tables. Naming them here would be the failure, so name them
        // here once, in the test, to be sure the check above would see
        // them.
        for forbidden in [
            "explanation",
            "hypothesis",
            "decision_notes",
            "prompt_shown",
            "assumptions",
        ] {
            assert!(
                !CONDITIONS_QUERY.contains(forbidden),
                "the search layer's query names {forbidden:?}"
            );
            assert!(!allowed.contains(&forbidden), "{forbidden:?} is allowed");
        }
    }

    /// Conditions are typed the way metrics are, so the comparison has
    /// to be too. `alpha` alternates 0.0 and -0.0 and `gamma` is NaN
    /// every time, and neither of those is a change. Compare the raw
    /// encodings instead and both would appear to move in step with a
    /// key that really does move, inventing a confound out of
    /// arithmetic.
    #[test]
    fn typed_values_compare_by_value_not_by_encoding() {
        let (_dir, store) = store();
        for (exp, alpha, beta, converged) in [
            ("e1", 0.0, 1.0, true),
            ("e2", -0.0, 2.0, false),
            ("e3", 0.0, 1.0, true),
            ("e4", -0.0, 2.0, false),
        ] {
            put_number(&store, exp, "alpha", alpha);
            put_number(&store, exp, "beta", beta);
            put_number(&store, exp, "gamma", f64::NAN);
            put_bool(&store, exp, "converged", converged);
        }

        let found = store.confounded_conditions(4).unwrap();

        assert_eq!(
            found.bundles,
            vec![vec!["beta".to_string(), "converged".to_string()]]
        );
        assert_eq!(found.crispness.verdict, Relation::Crisp);
        assert_eq!(found.crispness.locked, 1);
        assert_eq!(found.crispness.free, 4);
        // alpha and gamma both sit still, so their pair carries no
        // evidence either way and is held out of the verdict.
        assert_eq!(found.crispness.unvaried, 1);
    }

    /// A key recorded twice for one experiment keeps the later row.
    /// `seq` is insertion order, so the last write is the condition the
    /// run actually ran under. Here `harness` is corrected from "a" to
    /// "b" before the run; reading the correction as the value it
    /// replaced would show a change that never happened.
    #[test]
    fn a_key_recorded_twice_keeps_the_later_value() {
        let (_dir, store) = store();
        put(&store, "e1", "harness", "a");
        put(&store, "e1", "harness", "b");
        put(&store, "e2", "harness", "b");
        put(&store, "e3", "harness", "c");
        put(&store, "e4", "harness", "c");
        for (exp, matcher) in [("e1", "x"), ("e2", "x"), ("e3", "y"), ("e4", "y")] {
            put(&store, exp, "matcher", matcher);
        }

        let found = store.confounded_conditions(4).unwrap();

        assert_eq!(
            found.bundles,
            vec![vec!["harness".to_string(), "matcher".to_string()]]
        );
    }
}
