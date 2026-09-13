-- relay-x control-plane schema (Phase 6)
--
-- Reproducible from an empty database: psql -d relayx -f 001_initial.sql
-- Migrations are applied in order; each file is idempotent-agnostic (they
-- run exactly once against a fresh DB). No destructive statements.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ── Projects ────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Providers ───────────────────────────────────────────────────────────
-- Provider = protocol + model identity. Credentials live on the LANE that
-- carries the network egress (one source of truth for per-lane auth).
CREATE TABLE IF NOT EXISTS providers (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    protocol       TEXT NOT NULL,
    base_url       TEXT NOT NULL,
    model          TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE TABLE IF NOT EXISTS lanes (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider_id    TEXT REFERENCES providers(id) ON DELETE SET NULL,
    endpoint       TEXT NOT NULL,
    base_url       TEXT NOT NULL,
    egress         TEXT NOT NULL DEFAULT 'direct',
    policies       TEXT[] NOT NULL DEFAULT '{}',
    credential_ref JSONB,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS policies (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    rules       JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

-- ── Workflows │ versions ────────────────────────────────────────────────
-- A workflow has durable identity; edits always create a NEW version (never
-- mutate an active published one).
CREATE TABLE IF NOT EXISTS workflows (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'draft'
                CHECK (status IN ('draft','validated','compiled','published','active')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS workflow_versions (
    id             TEXT PRIMARY KEY,
    workflow_id    TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    version        INTEGER NOT NULL,
    workflow_json  JSONB NOT NULL,
    plan_hash      TEXT,
    status         TEXT NOT NULL DEFAULT 'draft'
                   CHECK (status IN ('draft','validated','compiled','published','active')),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_id, version)
);

-- ── Publications (audit history, independent of the runtime) ────────────
CREATE TABLE IF NOT EXISTS publications (
    id                 TEXT PRIMARY KEY,
    workflow_id        TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    workflow_version   INTEGER NOT NULL,
    plan_hash          TEXT NOT NULL,
    snapshot_version   BIGINT NOT NULL,
    status             TEXT NOT NULL CHECK (status IN ('succeeded','failed')),
    error              TEXT,
    published_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Active version pointer (the version the data plane serves).
CREATE TABLE IF NOT EXISTS workflow_active (
    workflow_id    TEXT PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
    workflow_version INTEGER NOT NULL,
    plan_hash      TEXT NOT NULL,
    snapshot_version BIGINT NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);