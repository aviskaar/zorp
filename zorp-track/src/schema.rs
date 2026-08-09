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
    file_path TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    git_commit_hash TEXT,
    committed_at BIGINT NOT NULL
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
    recorded_at BIGINT NOT NULL
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
);";
