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
CREATE INDEX IF NOT EXISTS idx_validations_track_id ON validations(track_id);";
