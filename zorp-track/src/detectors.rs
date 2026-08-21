//! aryabhatta step 4: boredom detectors, pure reads over the record.
//!
//! Each detector asks the same question of a different table: what has
//! this line of work stopped varying? Detection is arithmetic here, so
//! a model's only job downstream is turning a finding into a candidate
//! question. It may not add invariants of its own.
//!
//! Two rules from the spec's integrity properties bind everything in
//! this module, and both are tested at the bottom of the file.
//!
//! Detectors read and never write. Nothing here runs an INSERT, an
//! UPDATE, a DELETE or a CREATE.
//!
//! No detector reads a column holding model-authored text.
//! `checkpoints.prompt_shown` and `checkpoints.decision_notes` are
//! prose, and grouping on them would let the agent's own speculation
//! become tomorrow's observation. A system built that way develops the
//! ability to be surprised by itself.

use crate::track::Store;
use crate::TrackError;

// Every detector takes a minimum support below which it says nothing,
// because "this varied in none of two runs" is not evidence of anything.
// These are starting values, to be revised once real distributions
// exist rather than defended as derived.

/// Checkpoints recur many times inside a single investigation, so this
/// detector accrues fastest and can afford the lower bar.
pub const CHECKPOINT_HABITUATION_MIN_SUPPORT: u64 = 8;

/// The bar for the three detectors that count whole experiments or
/// pre-registrations. Those accrue once per run rather than several
/// times per investigation, so the same eight would be a much weaker
/// claim.
pub const EXPERIMENT_MIN_SUPPORT: u64 = 12;

/// Columns holding model-authored text.
///
/// Integrity rule 5: no detector, and nothing in the search layer, may
/// read one of these. Without the rule the agent's own speculation
/// becomes tomorrow's observation, and the system develops the ability
/// to be surprised by itself.
///
/// One list, in one place, because the rule binds three modules. A
/// second copy would drift the first time a model-authored column was
/// added, and the drift would be silent.
pub const MODEL_AUTHORED_COLUMNS: [&str; 3] = ["decision_notes", "prompt_shown", "explanation"];

/// Something the record shows has never varied, with the evidence for
/// saying so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Which detector produced this.
    pub detector: &'static str,
    /// What the invariant is about: a checkpoint kind, a track id, a
    /// condition key.
    pub subject: String,
    /// The column that held still.
    pub invariant_column: &'static str,
    /// The one value it held, every time.
    pub invariant_value: String,
    /// How many rows back the claim.
    pub support: u64,
    /// The SQL that produced it, exactly as it ran.
    pub query: String,
}

/// A detector's name, the column it watches, and the SQL it runs.
///
/// The minimum support is interpolated into the SQL rather than bound
/// as a parameter. It is a `u64`, so there is nothing to inject, and it
/// buys the property the spec asks for: the query a finding carries is
/// the query that ran, character for character, so anyone can re-run it
/// and get the same rows.
type Query = (&'static str, &'static str, String);

fn checkpoint_habituation_query(min_support: u64) -> Query {
    (
        "checkpoint_habituation",
        "status",
        format!(
            "SELECT kind, MIN(status), COUNT(*) FROM checkpoints GROUP BY kind \
             HAVING COUNT(DISTINCT status) = 1 AND COUNT(*) >= {min_support} ORDER BY kind"
        ),
    )
}

fn metric_monoculture_query(min_support: u64) -> Query {
    (
        "metric_monoculture",
        "metric_key",
        format!(
            "SELECT e.track_id, MIN(m.metric_key), COUNT(DISTINCT m.experiment_id) \
             FROM metrics m JOIN experiments e ON m.experiment_id = e.id \
             GROUP BY e.track_id \
             HAVING COUNT(DISTINCT m.metric_key) = 1 \
             AND COUNT(DISTINCT m.experiment_id) >= {min_support} \
             ORDER BY e.track_id"
        ),
    )
}

fn threshold_direction_monoculture_query(min_support: u64) -> Query {
    (
        "threshold_direction_monoculture",
        "threshold_direction",
        format!(
            "SELECT 'project', MIN(threshold_direction), COUNT(*) FROM preregistrations \
             WHERE threshold_direction IS NOT NULL \
             HAVING COUNT(DISTINCT threshold_direction) = 1 AND COUNT(*) >= {min_support}"
        ),
    )
}

fn invariant_condition_query(min_support: u64) -> Query {
    (
        "invariant_condition",
        "value",
        format!(
            "WITH shown AS ( \
               SELECT condition_key, experiment_id, value_type, \
                      CASE value_type \
                        WHEN 'number' THEN CAST(value_number AS VARCHAR) \
                        WHEN 'bool' THEN CAST(value_bool AS VARCHAR) \
                        ELSE COALESCE(value_string, '') \
                      END AS value \
               FROM conditions \
             ) \
             SELECT condition_key, MIN(value), COUNT(DISTINCT experiment_id) FROM shown \
             GROUP BY condition_key \
             HAVING COUNT(DISTINCT value_type || ':' || value) = 1 \
             AND COUNT(DISTINCT experiment_id) >= {min_support} \
             ORDER BY condition_key"
        ),
    )
}

/// Every query this module can run, in the order `boredom_findings`
/// runs them. One list means the integrity tests can hold all four to
/// the rules whether or not they happened to fire on a given store.
fn every_query(checkpoint_support: u64, experiment_support: u64) -> [Query; 4] {
    [
        checkpoint_habituation_query(checkpoint_support),
        metric_monoculture_query(experiment_support),
        threshold_direction_monoculture_query(experiment_support),
        invariant_condition_query(experiment_support),
    ]
}

impl Store {
    /// Checkpoint kinds resolved the same way every single time. A
    /// checkpoint presented 31 times and approved 31 times is a
    /// formality, not a decision. This is the detector that accrues
    /// fastest, since checkpoints recur many times per investigation.
    ///
    /// Groups on `kind` and `status` and reads nothing else. The other
    /// two columns on that table, `prompt_shown` and `decision_notes`,
    /// are prose a model or a human wrote, and integrity rule 5 keeps
    /// them out of every detector.
    pub fn checkpoint_habituation(&self, min_support: u64) -> Result<Vec<Finding>, TrackError> {
        self.findings(checkpoint_habituation_query(min_support))
    }

    /// Tracks whose experiments recorded exactly one distinct
    /// `metric_key`. Every run measured one thing and nothing has asked
    /// what that measure hides.
    ///
    /// Support counts experiments rather than metric rows: a single
    /// experiment writing the same key forty times is one observation,
    /// not forty.
    pub fn metric_monoculture(&self, min_support: u64) -> Result<Vec<Finding>, TrackError> {
        self.findings(metric_monoculture_query(min_support))
    }

    /// One finding when every pre-registration in this project points
    /// the same way. Every hypothesis registered can then be falsified
    /// by a number moving in one direction, and none by a distribution
    /// changing shape.
    ///
    /// The scope is the whole store, because a project is one store and
    /// a track may only be pre-registered once. Grouping by track would
    /// give every group a support of one.
    ///
    /// Pre-registrations with no recorded direction are left out rather
    /// than counted as a value of their own. A registration that names
    /// no direction is not evidence for the invariant or against it, and
    /// the support count says how many rows actually back the claim.
    pub fn threshold_direction_monoculture(
        &self,
        min_support: u64,
    ) -> Result<Vec<Finding>, TrackError> {
        self.findings(threshold_direction_monoculture_query(min_support))
    }

    /// Condition keys that took exactly one value across at least
    /// `min_support` experiments. The variable nobody has ever varied.
    ///
    /// `conditions` splits a value across four columns the way `metrics`
    /// does, so distinctness is judged on the rendered value carrying
    /// its type. Without the type, the number 1 and the string "1.0"
    /// would collapse into one value and a key that did move would read
    /// as invariant.
    pub fn invariant_condition(&self, min_support: u64) -> Result<Vec<Finding>, TrackError> {
        self.findings(invariant_condition_query(min_support))
    }

    /// All four detectors at their default minimum support, in a fixed
    /// order. Each detector sorts its own rows, so the whole list is
    /// deterministic: two runs over the same store agree.
    pub fn boredom_findings(&self) -> Result<Vec<Finding>, TrackError> {
        let mut out = Vec::new();
        for one in every_query(CHECKPOINT_HABITUATION_MIN_SUPPORT, EXPERIMENT_MIN_SUPPORT) {
            out.extend(self.findings(one)?);
        }
        Ok(out)
    }

    /// Every detector has the same shape: one SELECT returning subject,
    /// the single value, and the supporting count, in that order. The
    /// loop lives here so each detector above is just its SQL, which is
    /// the part worth reading.
    fn findings(&self, query: Query) -> Result<Vec<Finding>, TrackError> {
        let (detector, invariant_column, sql) = query;
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(Finding {
                detector,
                subject: r.get(0)?,
                invariant_column,
                invariant_value: r.get(1)?,
                support: r.get::<_, i64>(2)? as u64,
                query: sql.clone(),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::{CheckpointMode, Decider};
    use crate::experiment::MetricValue;
    use crate::prereg::ThresholdDirection;
    use crate::test_support::table_counts;
    use crate::track::Store;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Answers every checkpoint the same way, so a fixture can decide
    /// exactly how many times a kind was approved and how many rejected.
    struct Fixed(bool);
    impl Decider for Fixed {
        fn decide(&self, _prompt: &str) -> bool {
            self.0
        }
    }

    fn open() -> (TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        (dir, store)
    }

    fn checkpoints(store: &Store, track: &str, kind: &str, approvals: usize, rejections: usize) {
        store.create_track(track, "hyp").ok();
        let yes = CheckpointMode::Interactive(Arc::new(Fixed(true)));
        let no = CheckpointMode::Interactive(Arc::new(Fixed(false)));
        for _ in 0..approvals {
            store
                .record_checkpoint(track, kind, &yes, "proceed?")
                .unwrap();
        }
        for _ in 0..rejections {
            store
                .record_checkpoint(track, kind, &no, "proceed?")
                .unwrap();
        }
    }

    #[test]
    fn checkpoint_habituation_fires_when_a_kind_never_varies() {
        let (_dir, store) = open();
        checkpoints(&store, "t1", "validate", 8, 0);

        let found = store.checkpoint_habituation(8).unwrap();

        assert_eq!(found.len(), 1, "one kind never varied: {found:?}");
        assert_eq!(found[0].subject, "validate");
        assert_eq!(found[0].invariant_value, "approved");
        assert_eq!(found[0].support, 8);
    }

    #[test]
    fn checkpoint_habituation_stays_silent_when_the_kind_varied() {
        let (_dir, store) = open();
        checkpoints(&store, "t1", "validate", 7, 1);

        let found = store.checkpoint_habituation(8).unwrap();

        assert!(found.is_empty(), "one rejection is variation: {found:?}");
    }

    /// The boundary is inclusive at n and silent at n-1. Seven identical
    /// resolutions is a habit forming, not a habit.
    #[test]
    fn checkpoint_habituation_is_silent_below_its_minimum_support() {
        let (_dir, store) = open();
        checkpoints(&store, "t1", "validate", 7, 0);
        assert!(store.checkpoint_habituation(8).unwrap().is_empty());

        checkpoints(&store, "t1", "validate", 1, 0);
        assert_eq!(store.checkpoint_habituation(8).unwrap().len(), 1);
    }

    /// The query on a finding is the query that ran, not a paraphrase
    /// of it, so anyone can re-run it and get the same row back.
    #[test]
    fn a_finding_carries_the_query_that_produced_it() {
        let (_dir, store) = open();
        checkpoints(&store, "t1", "validate", 8, 0);

        let found = store.checkpoint_habituation(8).unwrap();

        let replayed: String = store
            .conn
            .query_row(&found[0].query, [], |r| r.get(0))
            .unwrap();
        assert_eq!(replayed, found[0].subject);
    }

    /// The starting values from the spec. They are guesses, and the
    /// point of pinning them is that changing one is a deliberate edit.
    #[test]
    fn the_default_minimum_supports_are_eight_and_twelve() {
        assert_eq!(CHECKPOINT_HABITUATION_MIN_SUPPORT, 8);
        assert_eq!(EXPERIMENT_MIN_SUPPORT, 12);
    }

    /// `count` experiments on `track`, each recording every key in
    /// `keys`. One key repeated is the monoculture fixture, two keys is
    /// the same fixture with the invariant absent.
    fn experiments(store: &Store, track: &str, count: usize, keys: &[&str]) {
        store.create_track(track, "hyp").ok();
        for _ in 0..count {
            let exp = store.create_experiment(track, "prereg").unwrap();
            for key in keys {
                store
                    .record_metric(&exp.id, key, MetricValue::Number(1.0))
                    .unwrap();
            }
        }
    }

    #[test]
    fn metric_monoculture_fires_when_a_track_only_ever_measured_one_thing() {
        let (_dir, store) = open();
        experiments(&store, "t1", 12, &["accuracy"]);

        let found = store.metric_monoculture(12).unwrap();

        assert_eq!(found.len(), 1, "one track never varied: {found:?}");
        assert_eq!(found[0].subject, "t1");
        assert_eq!(found[0].invariant_value, "accuracy");
        assert_eq!(found[0].support, 12);
    }

    #[test]
    fn metric_monoculture_stays_silent_when_a_second_measure_exists() {
        let (_dir, store) = open();
        experiments(&store, "t1", 11, &["accuracy"]);
        experiments(&store, "t1", 1, &["accuracy", "latency_ms"]);

        let found = store.metric_monoculture(12).unwrap();

        assert!(found.is_empty(), "a second key is variation: {found:?}");
    }

    #[test]
    fn metric_monoculture_is_silent_below_its_minimum_support() {
        let (_dir, store) = open();
        experiments(&store, "t1", 11, &["accuracy"]);
        assert!(store.metric_monoculture(12).unwrap().is_empty());

        experiments(&store, "t1", 1, &["accuracy"]);
        assert_eq!(store.metric_monoculture(12).unwrap().len(), 1);
    }

    /// One pre-registration per track, since a track may only be
    /// registered once, so `count` of them means `count` tracks.
    fn preregs(store: &Store, from: usize, count: usize, direction: ThresholdDirection) {
        for i in from..from + count {
            let track = format!("t{i}");
            store.create_track(&track, "hyp").unwrap();
            crate::prereg::insert_preregistration_row(
                store,
                &track,
                "hyp",
                "accuracy",
                0.9,
                Some(direction),
                std::path::Path::new("prereg.md"),
                "hash",
                None,
                0,
            )
            .unwrap();
        }
    }

    #[test]
    fn threshold_direction_monoculture_fires_when_every_prereg_points_the_same_way() {
        let (_dir, store) = open();
        preregs(&store, 0, 12, ThresholdDirection::LowerIsBetter);

        let found = store.threshold_direction_monoculture(12).unwrap();

        assert_eq!(found.len(), 1, "every direction is the same: {found:?}");
        assert_eq!(found[0].invariant_value, "lower-is-better");
        assert_eq!(found[0].support, 12);
    }

    #[test]
    fn threshold_direction_monoculture_stays_silent_when_one_points_the_other_way() {
        let (_dir, store) = open();
        preregs(&store, 0, 11, ThresholdDirection::LowerIsBetter);
        preregs(&store, 11, 1, ThresholdDirection::HigherIsBetter);

        let found = store.threshold_direction_monoculture(12).unwrap();

        assert!(found.is_empty(), "directions differ: {found:?}");
    }

    #[test]
    fn threshold_direction_monoculture_is_silent_below_its_minimum_support() {
        let (_dir, store) = open();
        preregs(&store, 0, 11, ThresholdDirection::LowerIsBetter);
        assert!(store
            .threshold_direction_monoculture(12)
            .unwrap()
            .is_empty());

        preregs(&store, 11, 1, ThresholdDirection::LowerIsBetter);
        assert_eq!(store.threshold_direction_monoculture(12).unwrap().len(), 1);
    }

    /// One experiment per entry in `values`, each carrying `key` at
    /// that entry's type and value. Written with plain SQL rather than
    /// through the recording API, which is being built next door: a
    /// detector reads the table, and its tests should not be coupled to
    /// whoever writes it.
    fn conditions(store: &Store, track: &str, key: &str, values: &[(&str, &str)]) {
        store.create_track(track, "hyp").ok();
        for (value_type, value) in values {
            let exp = store.create_experiment(track, "prereg").unwrap();
            let (num, text, boolean) = match *value_type {
                "number" => (Some(value.parse::<f64>().unwrap()), None, None),
                "bool" => (None, None, Some(*value == "true")),
                _ => (None, Some(value.to_string()), None),
            };
            store
                .conn
                .execute(
                    "INSERT INTO conditions (id, experiment_id, condition_key, value_type, value_number, value_string, value_bool, recorded_at, seq) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0)",
                    duckdb::params![
                        format!("{}-{key}", exp.id),
                        exp.id,
                        key,
                        value_type,
                        num,
                        text,
                        boolean
                    ],
                )
                .unwrap();
        }
    }

    fn same(
        value_type: &'static str,
        value: &'static str,
        n: usize,
    ) -> Vec<(&'static str, &'static str)> {
        vec![(value_type, value); n]
    }

    #[test]
    fn invariant_condition_fires_when_a_key_never_took_a_second_value() {
        let (_dir, store) = open();
        conditions(&store, "t1", "temperature", &same("number", "0", 12));

        let found = store.invariant_condition(12).unwrap();

        assert_eq!(found.len(), 1, "temperature never moved: {found:?}");
        assert_eq!(found[0].subject, "temperature");
        assert_eq!(found[0].invariant_value, "0.0");
        assert_eq!(found[0].support, 12);
    }

    #[test]
    fn invariant_condition_stays_silent_when_the_key_took_a_second_value() {
        let (_dir, store) = open();
        let mut values = same("number", "0", 11);
        values.push(("number", "1"));
        conditions(&store, "t1", "temperature", &values);

        let found = store.invariant_condition(12).unwrap();

        assert!(found.is_empty(), "temperature moved once: {found:?}");
    }

    #[test]
    fn invariant_condition_is_silent_below_its_minimum_support() {
        let (_dir, store) = open();
        conditions(&store, "t1", "temperature", &same("number", "0", 11));
        assert!(store.invariant_condition(12).unwrap().is_empty());

        conditions(&store, "t1", "temperature", &same("number", "0", 1));
        assert_eq!(store.invariant_condition(12).unwrap().len(), 1);
    }

    /// DuckDB renders the number 1 as "1.0", so the string "1.0" is its
    /// exact twin once rendered. Only the type tells them apart, and
    /// without that a key that did move would read as invariant.
    #[test]
    fn invariant_condition_does_not_confuse_a_number_with_its_text() {
        let (_dir, store) = open();
        let mut values = same("number", "1", 11);
        values.push(("string", "1.0"));
        conditions(&store, "t1", "harness", &values);

        let found = store.invariant_condition(12).unwrap();

        assert!(found.is_empty(), "two types are two values: {found:?}");
    }

    /// A store where all four invariants hold at once, so the tests
    /// below can hold every detector to the integrity rules in one go.
    fn all_four(store: &Store) {
        checkpoints(store, "t-check", "validate", 8, 0);
        experiments(store, "t-metric", 12, &["accuracy"]);
        preregs(store, 0, 12, ThresholdDirection::LowerIsBetter);
        conditions(store, "t-cond", "temperature", &same("number", "0", 12));
    }

    #[test]
    fn boredom_findings_runs_every_detector_in_a_fixed_order() {
        let (_dir, store) = open();
        all_four(&store);

        let found = store.boredom_findings().unwrap();

        let detectors: Vec<&str> = found.iter().map(|f| f.detector).collect();
        assert_eq!(
            detectors,
            [
                "checkpoint_habituation",
                "metric_monoculture",
                "threshold_direction_monoculture",
                "invariant_condition",
            ]
        );
    }

    /// A fresh project has no history, so every detector has nothing to
    /// say. This also covers the one query with no GROUP BY, which
    /// aggregates over the whole table and must return no row at all
    /// rather than a row of nulls.
    #[test]
    fn an_empty_store_produces_no_findings() {
        let (_dir, store) = open();
        assert!(store.boredom_findings().unwrap().is_empty());
    }

    /// Integrity rule 4. Detectors perform reads only. Nothing in this
    /// module inserts, updates, deletes or creates, and the cheapest way
    /// to hold it to that is to weigh the whole database twice.
    #[test]
    fn every_detector_leaves_every_table_exactly_as_it_found_it() {
        let (_dir, store) = open();
        all_four(&store);

        let before = table_counts(&store);
        assert!(
            before.iter().filter(|(_, n)| *n > 0).count() >= 4,
            "the fixture must have rows or this proves nothing: {before:?}"
        );

        store.checkpoint_habituation(8).unwrap();
        store.metric_monoculture(12).unwrap();
        store.threshold_direction_monoculture(12).unwrap();
        store.invariant_condition(12).unwrap();
        store.boredom_findings().unwrap();

        assert_eq!(table_counts(&store), before);
    }

    /// Integrity rule 5. No detector reads a column holding
    /// model-authored text. Checked against every query the module can
    /// issue, not only the ones that fired, since a silent detector
    /// still ran its SQL.
    #[test]
    fn no_detector_query_names_a_model_authored_column() {
        for (detector, _, sql) in
            every_query(CHECKPOINT_HABITUATION_MIN_SUPPORT, EXPERIMENT_MIN_SUPPORT)
        {
            for forbidden in MODEL_AUTHORED_COLUMNS {
                assert!(
                    !sql.contains(forbidden),
                    "{detector} names {forbidden}: {sql}"
                );
            }
        }
    }

    /// The same rule stated the other way round: no query writes. A
    /// detector that could write would let the reader change the record
    /// it is reading.
    #[test]
    fn no_detector_query_writes() {
        for (detector, _, sql) in
            every_query(CHECKPOINT_HABITUATION_MIN_SUPPORT, EXPERIMENT_MIN_SUPPORT)
        {
            let upper = sql.to_uppercase();
            for verb in ["INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER"] {
                assert!(!upper.contains(verb), "{detector} runs a {verb}: {sql}");
            }
        }
    }
}
