pub(crate) const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    hypothesis TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS preregistrations (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    hypothesis_snapshot TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    kill_threshold DOUBLE NOT NULL,
    threshold_direction TEXT,
    file_path TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    git_commit_hash TEXT,
    committed_at BIGINT NOT NULL,
    file_mtime_ms BIGINT,
    file_len BIGINT
);
CREATE TABLE IF NOT EXISTS experiments (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    prereg_id TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at BIGINT,
    completed_at BIGINT
);
CREATE TABLE IF NOT EXISTS metrics (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    value_type TEXT NOT NULL,
    value_number DOUBLE,
    value_string TEXT,
    value_bool BOOLEAN,
    recorded_at BIGINT NOT NULL,
    seq BIGINT NOT NULL
);
-- aryabhatta, step 1. The inputs a run was performed under. Same value
-- encoding as `metrics` so the two are symmetric: outputs were recorded
-- from the start, inputs were not.
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
-- aryabhatta, step 2. A quantitative forecast about one metric of one
-- experiment, recorded before that experiment produces the metric. Not a
-- pre-registration: that is one git-pinned commitment for a whole track,
-- this is a per-experiment forecast and there will be many.
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
-- aryabhatta, step 6. The ledger. One row per anomaly that cleared the
-- re-run gate, never deleted, and never written except through
-- `record_gate_verdict`, so nothing reaches it without being gated.
-- `explanation` is model-authored text; integrity rule 5 forbids any
-- detector and anything in the search layer from reading it.
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
-- aryabhatta, step 6. Every gate run, admitted or not. Rejected
-- anomalies are counted rather than discarded silently, which is the
-- only way the noisy TV rate becomes measurable: a rejection that
-- leaves no row cannot be counted later.
CREATE TABLE IF NOT EXISTS gate_runs (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    expectation_id TEXT NOT NULL,
    outcome TEXT NOT NULL,
    repeats BIGINT NOT NULL,
    admitted BOOLEAN NOT NULL,
    anomaly_id TEXT,
    created_at BIGINT NOT NULL,
    seq BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    prompt_shown TEXT NOT NULL,
    decision_notes TEXT,
    created_at BIGINT NOT NULL,
    resolved_at BIGINT
);
CREATE TABLE IF NOT EXISTS validations (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    redundancy_score DOUBLE NOT NULL,
    redundancy_citations TEXT NOT NULL,
    feasibility_score DOUBLE NOT NULL,
    feasibility_citations TEXT NOT NULL,
    verdict TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS critiques (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    round BIGINT NOT NULL,
    draft_hash TEXT NOT NULL,
    findings TEXT NOT NULL,
    accepted BOOLEAN NOT NULL,
    created_at BIGINT NOT NULL,
    seq BIGINT
);
-- Monotonic insert order within a track. created_at is milliseconds, so
-- two rows written in the same millisecond tie and give no ordering;
-- seq breaks the tie, the same way metrics already do. Both are derived
-- from the table itself on insert, not from a process-local counter, so
-- the order survives a restart. Rows written before these columns
-- existed hold NULL and sort last within their millisecond.
ALTER TABLE validations ADD COLUMN IF NOT EXISTS seq BIGINT;
ALTER TABLE checkpoints ADD COLUMN IF NOT EXISTS seq BIGINT;
ALTER TABLE preregistrations ADD COLUMN IF NOT EXISTS threshold_direction TEXT;
ALTER TABLE preregistrations ADD COLUMN IF NOT EXISTS file_mtime_ms BIGINT;
ALTER TABLE preregistrations ADD COLUMN IF NOT EXISTS file_len BIGINT;
CREATE INDEX IF NOT EXISTS idx_preregistrations_track_id ON preregistrations(track_id);
CREATE INDEX IF NOT EXISTS idx_experiments_track_id ON experiments(track_id);
CREATE INDEX IF NOT EXISTS idx_metrics_experiment_id ON metrics(experiment_id);
CREATE INDEX IF NOT EXISTS idx_checkpoints_track_id ON checkpoints(track_id);
CREATE INDEX IF NOT EXISTS idx_validations_track_id ON validations(track_id);
CREATE INDEX IF NOT EXISTS idx_critiques_track_id ON critiques(track_id);
CREATE INDEX IF NOT EXISTS idx_conditions_experiment_id ON conditions(experiment_id);
CREATE INDEX IF NOT EXISTS idx_expectations_experiment_id ON expectations(experiment_id);
CREATE INDEX IF NOT EXISTS idx_anomalies_track_id ON anomalies(track_id);
CREATE INDEX IF NOT EXISTS idx_gate_runs_experiment_id ON gate_runs(experiment_id);";
