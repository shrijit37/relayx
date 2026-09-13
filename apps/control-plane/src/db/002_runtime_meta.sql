-- Phase 6 review hardening: global monotonic snapshot version.
--
-- The runtime bundle version is independent of any single workflow's version.
-- Rehydrate republishes AFTER a restart; this counter guarantees
-- snapshot_version keeps increasing across restarts and across workflows,
-- so consumers keyed on snapshot monotonicity see no regressions.
-- Applied AFTER 001 (runtime_meta did not exist in the initial schema).

CREATE TABLE IF NOT EXISTS runtime_meta (
    id               TEXT PRIMARY KEY,
    snapshot_version BIGINT NOT NULL DEFAULT 0,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO runtime_meta (id) VALUES ('global')
ON CONFLICT (id) DO NOTHING;