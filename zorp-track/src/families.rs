//! aryabhatta step 7a: anomaly families, the search layer's second
//! caller.
//!
//! Two readers need to partition a graph. Confounded conditions, in
//! `partition`, sits on the ungated side and groups condition keys.
//! This one sits behind the calibration gate and groups ledger rows
//! into related deviations. They share one interface with two backends
//! chosen by `|V|`, because community detection entering the subsystem
//! twice would be two things that drift.
//!
//! Two rules from the spec bind everything here, and both are tested at
//! the bottom of the file.
//!
//! **Nothing in this module reads a column holding model-authored
//! text.** The similarity graph is built from `metric_key`, the
//! deviation's sign, and recorded conditions. Never
//! `anomalies.explanation`. Integrity rule 5 binds the search layer
//! exactly as it binds the detectors, and without it the agent's own
//! speculation becomes tomorrow's observation.
//!
//! **θ is swept, never chosen.** `erbga` takes unweighted edges, so a
//! continuous similarity has to become a binary one, and the honest way
//! to handle the cutoff is to refuse to pick it. The partition is
//! computed across a range of θ and only groups surviving a contiguous
//! band are kept. A family visible only at θ = 0.43 is an artifact of
//! the cutoff; one stable from 0.3 to 0.7 is structure. Nobody chooses
//! θ, so nobody can choose it to reach a wanted answer. Without this
//! the design repeats the defect that sank `evolve`, where the score
//! was maximized by an undifferentiated blob because edge addition was
//! priced free.

use crate::partition::{partition_graph, Backend, BackendRecord};
use crate::track::Store;
use crate::TrackError;
use std::collections::{BTreeMap, BTreeSet};

/// Which way a deviation went.
///
/// Derived from the recorded numbers, not from anything written about
/// them. Two anomalies pointing opposite ways are not the same
/// phenomenon even when everything else about them matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    Below,
    Above,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Below => "below",
            Direction::Above => "above",
        }
    }
}

/// The θ range the sweep covered, and how finely.
///
/// Recorded next to the result rather than assumed, because a family's
/// band means nothing without the range it was measured over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThetaSweep {
    pub low: f64,
    pub high: f64,
    pub steps: usize,
}

impl Default for ThetaSweep {
    /// 0.1 to 0.9 in nine steps.
    ///
    /// Deliberately not tuned. Tuning it against zorp's own ledgers
    /// would be a measurement, and there is no ledger to measure
    /// against yet. The endpoints avoid 0.0, where every pair sharing
    /// a metric and a direction is an edge whatever their conditions,
    /// and 1.0, where only identical condition sets are.
    fn default() -> Self {
        ThetaSweep {
            low: 0.1,
            high: 0.9,
            steps: 9,
        }
    }
}

impl ThetaSweep {
    /// The θ values, lowest first.
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

/// The contiguous run of θ values a family held together across.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThetaBand {
    pub low: f64,
    pub high: f64,
    /// How many swept values the run covers. This is what
    /// `min_band` is compared against, and reporting it means a reader
    /// can see how close a kept family came to being dropped.
    pub steps: usize,
}

/// A group of ledger rows the search layer found to hang together.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyFamily {
    /// Anomaly ids, sorted.
    pub members: Vec<String>,
    /// Every member shares this metric. An edge requires it, so a
    /// family cannot span two.
    pub metric_key: String,
    /// Every member deviated this way. An edge requires it too.
    pub direction: Direction,
    /// Conditions every member was recorded under, as key and rendered
    /// value. The overlap that produced the family, carried so a reader
    /// does not have to re-derive it.
    pub shared_conditions: Vec<(String, String)>,
    /// The θ band the family survived.
    pub band: ThetaBand,
}

/// Families, and everything needed to know what they are worth.
#[derive(Debug, Clone)]
pub struct AnomalyFamilies {
    pub families: Vec<AnomalyFamily>,
    /// The range swept, so the bands below can be read.
    pub swept: ThetaSweep,
    /// The band length a group had to survive to be kept.
    pub min_band: usize,
    /// How many ledger rows went in. A reader needs this to tell "no
    /// families" from "nothing to look at", which look the same if only
    /// the families are reported.
    pub anomalies_considered: usize,
    /// Groups that appeared at some θ but never across a long enough
    /// band. Counted rather than dropped silently: this is the number
    /// that says whether the sweep is doing work or whether every group
    /// is an artifact of the cutoff.
    pub discarded_as_unstable: usize,
    /// Which backend produced the partitions, and its seed and
    /// parameters if it was the stochastic one. Recorded from the
    /// widest θ in the sweep, the run with the most edges.
    pub backend: BackendRecord,
}

/// One ledger row, reduced to what an edge may be built from.
///
/// Assembled from a query that names no model-authored column, and
/// holding no field that could carry one. A family cannot be influenced
/// by an explanation that does not reach this struct.
#[derive(Debug, Clone)]
struct Deviation {
    id: String,
    metric_key: String,
    direction: Direction,
    conditions: BTreeSet<(String, String)>,
}

/// The columns this module reads from `anomalies`.
///
/// Named one at a time rather than `SELECT *`, so integrity rule 5 is
/// checkable by reading one line. `explanation` is absent and the test
/// at the bottom of the file asserts it stays absent.
const DEVIATION_SQL: &str = "SELECT id, experiment_id, metric_key, observed_value, \
     interval_low, interval_high FROM anomalies WHERE track_id = ? ORDER BY seq";

/// Conditions, joined to the experiment each anomaly came from.
///
/// `conditions` is append-only, so a key recorded more than once has a
/// history and the last value is the one in force. The window picks it
/// without a second round trip.
///
/// The inner select names its columns too. It used to be `SELECT *`,
/// which was harmless only because `conditions` happens to hold no
/// prose, and a rule that holds by luck holds until the next column.
/// Naming them also lets the test below refuse a bare star outright
/// rather than carve out an exception for this query. `seq` and `id`
/// are not in the list and do not need to be: the window's ORDER BY
/// reads them from `conditions` directly.
const CONDITION_SQL: &str = "SELECT experiment_id, condition_key, value_type, value_number, \
     value_string, value_bool FROM ( \
       SELECT experiment_id, condition_key, value_type, value_number, value_string, \
              value_bool, ROW_NUMBER() OVER ( \
                PARTITION BY experiment_id, condition_key ORDER BY seq DESC, id DESC \
              ) AS rn FROM conditions \
     ) WHERE rn = 1";

/// How alike two deviations are, in [0, 1].
///
/// Zero unless they share a metric and a direction, which is the spec's
/// rule: an edge means shared metric, same deviation sign, overlapping
/// conditions. Above that it is the Jaccard overlap of the condition
/// sets, which is what makes the similarity continuous and therefore
/// what makes the θ sweep necessary.
///
/// Two deviations that both recorded no conditions score zero rather
/// than one. An empty intersection over an empty union is not perfect
/// agreement, it is the absence of evidence, and scoring it as a
/// certainty would bundle every unconditioned anomaly in the ledger
/// into one family.
fn similarity(a: &Deviation, b: &Deviation) -> f64 {
    if a.metric_key != b.metric_key || a.direction != b.direction {
        return 0.0;
    }
    let shared = a.conditions.intersection(&b.conditions).count();
    let union = a.conditions.union(&b.conditions).count();
    if union == 0 {
        return 0.0;
    }
    shared as f64 / union as f64
}

impl Store {
    /// Group a track's ledger into families of related deviations.
    ///
    /// `min_band` is how many consecutive θ values a group has to
    /// survive to be reported. Below 2 the sweep is not doing anything:
    /// a group present at one θ and nowhere else is exactly what the
    /// sweep exists to discard.
    ///
    /// Reads only.
    pub fn anomaly_families(
        &self,
        track_id: &str,
        min_band: usize,
    ) -> Result<AnomalyFamilies, TrackError> {
        self.anomaly_families_with(track_id, min_band, ThetaSweep::default(), Backend::Auto)
    }

    /// [`Store::anomaly_families`] with the sweep and the backend named.
    ///
    /// Naming a backend is for the band where both can run, where the
    /// exact answer is a standing regression check on the search.
    ///
    /// Reads only.
    pub fn anomaly_families_with(
        &self,
        track_id: &str,
        min_band: usize,
        swept: ThetaSweep,
        backend: Backend,
    ) -> Result<AnomalyFamilies, TrackError> {
        if min_band < 2 {
            return Err(TrackError::Malformed {
                what: "anomaly families",
                detail: format!(
                    "min_band {min_band} does not sweep; a group present at one \
                     threshold and nowhere else is what the sweep exists to discard"
                ),
            });
        }
        let deviations = self.read_deviations(track_id)?;
        let thetas = swept.values();

        // Which θ values each group survived, keyed by the group. A
        // group is its sorted member ids, so two partitions agree only
        // when they produced the same set, not merely a similar one.
        let mut seen: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
        let mut record = BackendRecord::Exact;

        for (index, &theta) in thetas.iter().enumerate() {
            let mut edges = Vec::new();
            for i in 0..deviations.len() {
                for j in (i + 1)..deviations.len() {
                    if similarity(&deviations[i], &deviations[j]) >= theta {
                        edges.push((i, j));
                    }
                }
            }
            let (groups, ran) = partition_graph(deviations.len(), &edges, backend.clone());
            // The lowest θ has the most edges, so it is the run whose
            // backend record is worth keeping: it is where the search
            // had the most to do, and where `Auto` is most likely to
            // have chosen the stochastic backend.
            if index == 0 {
                record = ran;
            }
            for group in groups {
                if group.len() < 2 {
                    continue;
                }
                let members: Vec<String> =
                    group.iter().map(|&v| deviations[v].id.clone()).collect();
                seen.entry(members).or_default().push(index);
            }
        }

        let mut families = Vec::new();
        let mut discarded_as_unstable = 0;
        for (members, indices) in seen {
            match longest_run(&indices) {
                Some((start, len)) if len >= min_band => {
                    let representative = &deviations
                        .iter()
                        .find(|d| d.id == members[0])
                        .expect("members came from deviations");
                    families.push(AnomalyFamily {
                        metric_key: representative.metric_key.clone(),
                        direction: representative.direction,
                        shared_conditions: shared_conditions(&deviations, &members),
                        band: ThetaBand {
                            low: thetas[start],
                            high: thetas[start + len - 1],
                            steps: len,
                        },
                        members,
                    });
                }
                _ => discarded_as_unstable += 1,
            }
        }
        // Widest band first, so the most stable family is the one a
        // reader sees. Ties broken by member list, so the order does
        // not depend on the map's iteration.
        families.sort_by(|a, b| {
            b.band
                .steps
                .cmp(&a.band.steps)
                .then_with(|| a.members.cmp(&b.members))
        });

        Ok(AnomalyFamilies {
            families,
            swept,
            min_band,
            anomalies_considered: deviations.len(),
            discarded_as_unstable,
            backend: record,
        })
    }

    /// Every ledger row for a track, with the conditions of the
    /// experiment it came from.
    fn read_deviations(&self, track_id: &str) -> Result<Vec<Deviation>, TrackError> {
        let mut by_experiment: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
        let mut stmt = self.conn.prepare(CONDITION_SQL)?;
        let rows = stmt.query_map([], |r| {
            let experiment_id: String = r.get(0)?;
            let key: String = r.get(1)?;
            let value_type: String = r.get(2)?;
            // Rendered with its type tag, the same way the re-run gate
            // fingerprints conditions, so the number 1 and the string
            // "1" are different conditions rather than one.
            let rendered = match value_type.as_str() {
                "number" => {
                    let n: f64 = r.get(3)?;
                    format!("number:{n}")
                }
                "bool" => {
                    let b: bool = r.get(5)?;
                    format!("bool:{b}")
                }
                "string" => {
                    let s: String = r.get(4)?;
                    format!("string:{s}")
                }
                // No catch-all. A value_type this crate never wrote is
                // corruption, and rendering it as text would fold a
                // broken row into a real condition set.
                other => {
                    return Err(duckdb::Error::InvalidColumnType(
                        2,
                        format!("unknown condition value_type '{other}'"),
                        duckdb::types::Type::Text,
                    ))
                }
            };
            Ok((experiment_id, key, rendered))
        })?;
        for row in rows {
            let (experiment_id, key, rendered) = row?;
            by_experiment
                .entry(experiment_id)
                .or_default()
                .insert((key, rendered));
        }

        let mut stmt = self.conn.prepare(DEVIATION_SQL)?;
        let rows = stmt.query_map(duckdb::params![track_id], |r| {
            let id: String = r.get(0)?;
            let experiment_id: String = r.get(1)?;
            let metric_key: String = r.get(2)?;
            let observed: f64 = r.get(3)?;
            let low: f64 = r.get(4)?;
            let high: f64 = r.get(5)?;
            Ok((id, experiment_id, metric_key, observed, low, high))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, experiment_id, metric_key, observed, low, high) = row?;
            // An admitted row landed outside its interval, so one of
            // these holds. `unverifiable` rows are the exception: they
            // were admitted without the gate confirming a side, and
            // they are still on one side of the interval by
            // arithmetic, which is all this needs.
            let direction = if observed > high {
                Direction::Above
            } else if observed < low {
                Direction::Below
            } else {
                // Not reachable through `record_gate_verdict`, which
                // refuses to gate a value inside its own interval. A
                // row edited by hand could be here, and it is skipped
                // rather than assigned a direction it does not have.
                continue;
            };
            out.push(Deviation {
                id,
                metric_key,
                direction,
                conditions: by_experiment
                    .get(&experiment_id)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
        Ok(out)
    }
}

/// Conditions every member of a family was recorded under.
fn shared_conditions(deviations: &[Deviation], members: &[String]) -> Vec<(String, String)> {
    let mut shared: Option<BTreeSet<(String, String)>> = None;
    for id in members {
        let Some(deviation) = deviations.iter().find(|d| &d.id == id) else {
            continue;
        };
        shared = Some(match shared {
            None => deviation.conditions.clone(),
            Some(so_far) => so_far
                .intersection(&deviation.conditions)
                .cloned()
                .collect(),
        });
    }
    shared.unwrap_or_default().into_iter().collect()
}

/// The start and length of the longest run of consecutive integers.
///
/// `indices` is ascending and has no duplicates: it is built by pushing
/// the loop counter once per θ.
fn longest_run(indices: &[usize]) -> Option<(usize, usize)> {
    let first = *indices.first()?;
    let (mut best_start, mut best_len) = (first, 1);
    let (mut run_start, mut run_len) = (first, 1);
    for pair in indices.windows(2) {
        if pair[1] == pair[0] + 1 {
            run_len += 1;
        } else {
            run_start = pair[1];
            run_len = 1;
        }
        if run_len > best_len {
            best_start = run_start;
            best_len = run_len;
        }
    }
    Some((best_start, best_len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anomalies::Anomaly;
    use crate::experiment::MetricValue;
    use crate::test_support::table_counts;
    use crate::track::Store;
    use tempfile::tempdir;

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        (dir, store)
    }

    /// Put one admitted anomaly in the ledger, through the gate,
    /// because there is no other way in and this module should be
    /// tested against rows that arrived the way real ones do.
    ///
    /// `observed` is the original's outcome and `repeat` is a value
    /// that reproduces it. The forecast is always [0.70, 0.90].
    fn admit(
        store: &Store,
        metric_key: &str,
        conditions: &[(&str, &str)],
        observed: f64,
        repeat: f64,
    ) -> Anomaly {
        let original = store.create_experiment("t1", "prereg").unwrap();
        let replay = store.create_experiment("t1", "prereg").unwrap();
        for experiment in [&original, &replay] {
            for (key, value) in conditions {
                store
                    .record_condition(
                        &experiment.id,
                        key,
                        &MetricValue::Text((*value).to_string()),
                    )
                    .unwrap();
            }
        }
        store
            .record_expectation(&original.id, metric_key, 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap();
        store
            .record_metric(&original.id, metric_key, MetricValue::Number(observed))
            .unwrap();
        store
            .record_metric(&replay.id, metric_key, MetricValue::Number(repeat))
            .unwrap();
        let verdict = store
            .rerun_gate(&original.id, metric_key, &[replay.id.as_str()])
            .unwrap();
        store.record_gate_verdict(&verdict).unwrap().unwrap()
    }

    #[test]
    fn an_empty_ledger_has_no_families() {
        let (_dir, store) = open();
        let found = store.anomaly_families("t1", 2).unwrap();
        assert!(found.families.is_empty());
        assert_eq!(found.anomalies_considered, 0);
    }

    /// Two deviations with identical conditions, the same metric and
    /// the same direction agree at every θ, so they survive the whole
    /// sweep.
    #[test]
    fn identical_deviations_form_one_family_across_the_whole_sweep() {
        let (_dir, store) = open();
        let conditions = [("model", "opus"), ("context", "8k")];
        let a = admit(&store, "accuracy", &conditions, 0.50, 0.51);
        let b = admit(&store, "accuracy", &conditions, 0.52, 0.53);

        let found = store.anomaly_families("t1", 2).unwrap();
        assert_eq!(found.families.len(), 1, "{found:?}");
        let family = &found.families[0];
        assert_eq!(family.members, sorted(vec![a.id, b.id]));
        assert_eq!(family.metric_key, "accuracy");
        assert_eq!(family.direction, Direction::Below);
        assert_eq!(family.band.steps, 9);
        assert_eq!(family.shared_conditions.len(), 2);
    }

    /// The edge rule requires a shared metric. Two deviations under
    /// identical conditions measuring different things are not one
    /// phenomenon.
    #[test]
    fn deviations_on_different_metrics_never_join() {
        let (_dir, store) = open();
        let conditions = [("model", "opus")];
        admit(&store, "accuracy", &conditions, 0.50, 0.51);
        admit(&store, "latency", &conditions, 0.50, 0.51);

        let found = store.anomaly_families("t1", 2).unwrap();
        assert!(found.families.is_empty(), "{found:?}");
        assert_eq!(found.anomalies_considered, 2);
    }

    /// The edge rule requires the same sign. One metric that came in
    /// low and one that came in high are not the same deviation.
    #[test]
    fn deviations_in_opposite_directions_never_join() {
        let (_dir, store) = open();
        let conditions = [("model", "opus")];
        admit(&store, "accuracy", &conditions, 0.50, 0.51);
        admit(&store, "accuracy", &conditions, 0.99, 0.98);

        let found = store.anomaly_families("t1", 2).unwrap();
        assert!(found.families.is_empty(), "{found:?}");
    }

    /// The whole point of the sweep. These two share one condition of
    /// three, so they are an edge only where θ is low, and a group that
    /// appears at the bottom of the range and vanishes is a cutoff
    /// artifact rather than structure.
    #[test]
    fn a_group_that_only_survives_a_short_band_is_discarded() {
        let (_dir, store) = open();
        admit(
            &store,
            "accuracy",
            &[("model", "opus"), ("a", "1"), ("b", "2")],
            0.50,
            0.51,
        );
        admit(
            &store,
            "accuracy",
            &[("model", "opus"), ("c", "3"), ("d", "4")],
            0.52,
            0.53,
        );

        // Jaccard here is 1 shared over 5 union = 0.2, so the pair is
        // an edge at θ = 0.1 and 0.2 and nowhere above.
        let found = store.anomaly_families("t1", 4).unwrap();
        assert!(found.families.is_empty(), "{found:?}");
        assert_eq!(found.discarded_as_unstable, 1);

        // The same data with a band of 2 keeps it, which is what makes
        // the discard above a threshold effect and not an empty query.
        let kept = store.anomaly_families("t1", 2).unwrap();
        assert_eq!(kept.families.len(), 1, "{kept:?}");
        assert_eq!(kept.families[0].band.steps, 2);
    }

    /// Two anomalies that recorded no conditions at all score zero
    /// similarity, not one. Empty over empty is the absence of
    /// evidence, and scoring it as certainty would bundle every
    /// unconditioned row in the ledger into a single family.
    #[test]
    fn deviations_with_no_conditions_do_not_bundle() {
        let (_dir, store) = open();
        admit(&store, "accuracy", &[], 0.50, 0.51);
        admit(&store, "accuracy", &[], 0.52, 0.53);

        let found = store.anomaly_families("t1", 2).unwrap();
        assert!(found.families.is_empty(), "{found:?}");
        assert_eq!(found.anomalies_considered, 2);
    }

    #[test]
    fn shared_conditions_are_the_intersection_over_the_family() {
        let (_dir, store) = open();
        admit(
            &store,
            "accuracy",
            &[("model", "opus"), ("seed", "1")],
            0.50,
            0.51,
        );
        admit(
            &store,
            "accuracy",
            &[("model", "opus"), ("seed", "2")],
            0.52,
            0.53,
        );

        let found = store.anomaly_families("t1", 2).unwrap();
        assert_eq!(found.families.len(), 1, "{found:?}");
        let shared = &found.families[0].shared_conditions;
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].0, "model");
        assert!(shared[0].1.contains("opus"));
    }

    /// A sweep of one θ is not a sweep, and the whole defence against
    /// picking a favourable cutoff rests on it being more than one.
    #[test]
    fn a_min_band_below_two_is_refused() {
        let (_dir, store) = open();
        let err = store
            .anomaly_families("t1", 1)
            .expect_err("a band of one does not sweep");
        assert!(err.to_string().contains("does not sweep"), "{err}");
    }

    /// The published protocol trimmed to what a unit test needs.
    ///
    /// 25 islands at 250 by 1000 generations, run once per swept θ, is
    /// a minute of wall clock on a four-vertex graph. These tests are
    /// about the shape of the answer, the refinement property and the
    /// recorded seed, not about search quality, and search quality has
    /// its own benchmark suite in `erbga`. Anything that depends on
    /// tuning belongs there and not here.
    fn quick_search() -> Backend {
        let mut params = erbga::GaParams::thesis();
        params.population_size = 20;
        params.generations = 20;
        Backend::Search(crate::partition::SearchSettings {
            seed: crate::partition::DEFAULT_SEED,
            islands: 2,
            params,
        })
    }

    /// A narrow sweep, for the same reason.
    fn quick_sweep() -> ThetaSweep {
        ThetaSweep {
            low: 0.2,
            high: 0.6,
            steps: 3,
        }
    }

    /// The exact backend and the search backend are compared on the
    /// same graph, which is the free regression check the design asks
    /// for. `erbga` only removes edges, so its partition is always a
    /// refinement: it may split a family the exact backend keeps whole,
    /// and can never merge two it keeps apart.
    #[test]
    fn the_search_backend_returns_a_refinement_of_the_exact_one() {
        let (_dir, store) = open();
        let conditions = [("model", "opus"), ("context", "8k")];
        for observed in [0.50, 0.52, 0.54, 0.56] {
            admit(&store, "accuracy", &conditions, observed, observed + 0.01);
        }

        let exact = store
            .anomaly_families_with("t1", 2, quick_sweep(), Backend::Exact)
            .unwrap();
        let searched = store
            .anomaly_families_with("t1", 2, quick_sweep(), quick_search())
            .unwrap();

        // Every searched family sits inside one exact family.
        for family in &searched.families {
            let members: BTreeSet<&String> = family.members.iter().collect();
            assert!(
                exact.families.iter().any(|e| {
                    let whole: BTreeSet<&String> = e.members.iter().collect();
                    members.is_subset(&whole)
                }),
                "searched family {:?} is not inside any exact family {:?}",
                family.members,
                exact
                    .families
                    .iter()
                    .map(|f| &f.members)
                    .collect::<Vec<_>>()
            );
        }
    }

    /// A searched partition is not a result anyone can check without
    /// the seed and the parameters that produced it.
    #[test]
    fn a_searched_partition_carries_its_seed_and_parameters() {
        let (_dir, store) = open();
        let conditions = [("model", "opus")];
        admit(&store, "accuracy", &conditions, 0.50, 0.51);
        admit(&store, "accuracy", &conditions, 0.52, 0.53);

        let found = store
            .anomaly_families_with("t1", 2, quick_sweep(), quick_search())
            .unwrap();
        match found.backend {
            BackendRecord::Search(record) => {
                assert_eq!(record.seed, crate::partition::DEFAULT_SEED);
                assert_eq!(record.islands, 2);
                assert_eq!(record.params.generations, 20);
            }
            BackendRecord::Exact => panic!("asked for the search backend and got the exact one"),
        }
    }

    /// Integrity rule 5, from the search layer's side. Neither query
    /// this module can issue may reach a column holding model-authored
    /// text. Checked against the SQL itself rather than against the
    /// result, since a query that returned nothing still ran.
    ///
    /// The check is the detector module's, not a copy of it, and it
    /// refuses a bare `*` as well as a named column. Put `explanation`
    /// back in `DEVIATION_SQL`, or restore the `SELECT *` the inner
    /// half of `CONDITION_SQL` used to have, and this goes red.
    #[test]
    fn no_family_query_can_read_a_model_authored_column() {
        for (name, sql) in [("deviations", DEVIATION_SQL), ("conditions", CONDITION_SQL)] {
            if let Some(why) = crate::detectors::breaks_model_authored_rule(sql) {
                panic!("{name} {why}: {sql}");
            }
        }
    }

    #[test]
    fn no_family_query_writes() {
        for (name, sql) in [("deviations", DEVIATION_SQL), ("conditions", CONDITION_SQL)] {
            let upper = sql.to_uppercase();
            for verb in ["INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER"] {
                assert!(!upper.contains(verb), "{name} runs a {verb}: {sql}");
            }
        }
    }

    /// The same rule weighed rather than read: grouping the ledger must
    /// leave the ledger exactly as it found it.
    #[test]
    fn grouping_the_ledger_writes_nothing() {
        let (_dir, store) = open();
        let conditions = [("model", "opus")];
        admit(&store, "accuracy", &conditions, 0.50, 0.51);
        admit(&store, "accuracy", &conditions, 0.52, 0.53);

        let before = table_counts(&store);
        store.anomaly_families("t1", 2).unwrap();
        assert_eq!(table_counts(&store), before);
    }

    /// An explanation written onto a member must not change the
    /// families. This is integrity rule 5 checked by outcome rather
    /// than by reading the SQL, which is the check that survives
    /// someone rewriting the queries.
    #[test]
    fn writing_an_explanation_does_not_change_any_family() {
        let (_dir, store) = open();
        let conditions = [("model", "opus")];
        let a = admit(&store, "accuracy", &conditions, 0.50, 0.51);
        admit(&store, "accuracy", &conditions, 0.52, 0.53);

        let before = store.anomaly_families("t1", 2).unwrap();
        store
            .set_anomaly_status(
                &a.id,
                crate::anomalies::AnomalyStatus::Explained,
                Some("a totally different metric on a totally different model"),
            )
            .unwrap();
        let after = store.anomaly_families("t1", 2).unwrap();

        assert_eq!(
            before
                .families
                .iter()
                .map(|f| f.members.clone())
                .collect::<Vec<_>>(),
            after
                .families
                .iter()
                .map(|f| f.members.clone())
                .collect::<Vec<_>>()
        );
    }

    /// Corruption is refused rather than folded into a real condition
    /// set. A row holding a `value_type` this crate never wrote would,
    /// under a catch-all decode, become a text condition and quietly
    /// change which anomalies look alike. This is the shape of the
    /// 2026-08-15 `TrackStatus` defect, checked rather than assumed
    /// away.
    #[test]
    fn an_unknown_condition_value_type_is_refused_rather_than_read_as_text() {
        let (_dir, store) = open();
        let conditions = [("model", "opus")];
        admit(&store, "accuracy", &conditions, 0.50, 0.51);
        admit(&store, "accuracy", &conditions, 0.52, 0.53);
        // Sanity: this store groups cleanly before the row is broken,
        // so the refusal below is the corruption and not the fixture.
        assert_eq!(store.anomaly_families("t1", 2).unwrap().families.len(), 1);

        store
            .conn
            .execute(
                "UPDATE conditions SET value_type = 'duration' WHERE condition_key = 'model'",
                [],
            )
            .unwrap();

        let err = store
            .anomaly_families("t1", 2)
            .expect_err("a value_type this crate never wrote must not decode to text");
        assert!(err.to_string().contains("duration"), "{err}");
    }

    #[test]
    fn the_sweep_covers_its_stated_range() {
        let sweep = ThetaSweep {
            low: 0.2,
            high: 0.6,
            steps: 5,
        };
        let values = sweep.values();
        assert_eq!(values.len(), 5);
        assert!((values[0] - 0.2).abs() < 1e-12);
        assert!((values[4] - 0.6).abs() < 1e-12);
        assert!((values[2] - 0.4).abs() < 1e-12);
    }

    #[test]
    fn the_longest_run_is_the_longest_one_and_not_the_first() {
        assert_eq!(longest_run(&[0, 2, 3, 4, 7]), Some((2, 3)));
        assert_eq!(longest_run(&[5]), Some((5, 1)));
        assert_eq!(longest_run(&[]), None);
        assert_eq!(longest_run(&[0, 1, 3, 4, 5]), Some((3, 3)));
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }
}
