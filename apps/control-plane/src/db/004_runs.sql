-- relay-x control-plane schema (Phase 6.8)
--
-- Run-history persistence: durable records for every workflow execution.
-- Mirrors the workflows/workflow_versions conventions:
--   * `runs.workflow_id` references workflows(id) ON DELETE CASCADE, so
--     deleting a workflow cleans up its run history.
--   * `input_body` stores the request body for replay/debug.
--   * `output` holds the buffered result on completion (null for streaming
--     runs, whose tokens are ephemeral).
--   * `status` lifecycle: running -> completed | failed | cancelled.

CREATE TABLE IF NOT EXISTS runs (
    id               TEXT PRIMARY KEY,
    workflow_id      TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    workflow_version INTEGER NOT NULL,
    snapshot_version BIGINT NOT NULL,
    plan_hash        TEXT,
    status           TEXT NOT NULL CHECK (status IN ('running','completed','failed','cancelled')),
    input_body       JSONB,
    output           JSONB,
    error            TEXT,
    started_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS runs_workflow_id_idx ON runs (workflow_id, started_at DESC);
CREATE INDEX IF NOT EXISTS runs_started_at_idx ON runs (started_at DESC);
