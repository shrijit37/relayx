-- relay-x model catalog tables (Phase 7)
--
-- Populated by the control-plane sync from models.dev; consumed by the
-- data-plane as an ArcSwap in-memory snapshot + the vendored JSON fallback.

-- ── Sync metadata ────────────────────────────────────────────────────
-- Key-value store for sync state (ETag, last sync timestamp, etc.).
-- Avoids injecting sentinel rows into domain tables.
CREATE TABLE IF NOT EXISTS catalog_meta (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Providers ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS catalog_providers (
    id              TEXT PRIMARY KEY,
    display_name    TEXT NOT NULL,
    logo_path       TEXT,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Models ───────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS catalog_models (
    id              TEXT PRIMARY KEY,
    provider_id     TEXT NOT NULL REFERENCES catalog_providers(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    family          TEXT,
    modalities      JSONB NOT NULL DEFAULT '{}',
    capabilities    JSONB NOT NULL DEFAULT '{}',
    cost            JSONB,
    limits          JSONB,
    knowledge_cutoff TEXT,
    release_date    TEXT,
    open_weights    BOOLEAN NOT NULL DEFAULT false,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_catalog_models_provider ON catalog_models(provider_id);
CREATE INDEX IF NOT EXISTS idx_catalog_models_family   ON catalog_models(family);
