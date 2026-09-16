/**
 * models.dev catalog sync — scheduled fetch, transform, persist.
 *
 * Rules:
 * - Only the control-plane fetches models.dev (gateway + web never do).
 * - ETag/If-None-Match to avoid redundant transfer.
 * - Retry with exponential backoff on transient errors (3 attempts).
 * - Fail-open: if all retries fail, log the error and continue serving
 *   whatever is in the DB (populated from a previous sync or vendored).
 * - 24h default TTL between syncs.
 * - Sync state is owned by the caller (no module-level singletons).
 */

import type { Pool } from "pg";
import type { ProviderCatalogEntry, CatalogMeta, ModelCatalog } from "./types";

const MODELS_DEV_API = "https://models.dev/api.json";
const SYNC_TTL_MS = 24 * 60 * 60 * 1000; // 24h
const FETCH_TIMEOUT_MS = 15_000;
const MAX_RETRIES = 3;

// ─── Sync state (owned by startCatalogSync closure, not module-level) ────

interface SyncState {
  lastSyncAt: number;
  lastSyncOk: boolean;
}

// ─── Core sync logic ──────────────────────────────────────────────────────

/**
 * Attempt a catalog sync. Returns metadata on success, null on failure.
 * Intended to be called on an interval; short-circuits if TTL hasn't expired.
 */
export async function syncCatalog(
  pool: Pool,
  state: SyncState,
): Promise<CatalogMeta | null> {
  const now = Date.now();
  if (state.lastSyncOk && now - state.lastSyncAt < SYNC_TTL_MS) return null;

  const existingEtag = await getStoredEtag(pool);

  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    try {
      const headers: Record<string, string> = {};
      if (existingEtag) headers["If-None-Match"] = existingEtag;

      const resp = await fetch(MODELS_DEV_API, {
        headers,
        signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
      });

      if (resp.status === 304) {
        console.log("[models-dev] catalog unchanged (304)");
        // Persist the sync timestamp even on 304 so /catalog/status
        // reflects the latest successful check, not the last full sync.
        await pool
          .query(
            `INSERT INTO catalog_meta (key, value, updated_at)
             VALUES ('last_sync_at', $1, now())
             ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = now()`,
            [String(now)],
          )
          .catch((e) =>
            console.warn("[models-dev] failed to persist 304 sync timestamp:", e),
          );
        state.lastSyncAt = now;
        state.lastSyncOk = true;
        return await getCatalogMeta(pool, state);
      }

      if (!resp.ok) {
        throw new Error(`models.dev returned HTTP ${resp.status}`);
      }

      const newEtag = resp.headers.get("etag") ?? null;
      const data = (await resp.json()) as Record<string, ProviderCatalogEntry>;

      const { providers, models, providerCount, modelCount } =
        transformProviders(data);

      await upsertCatalog(pool, providers, models, newEtag);

      const meta: CatalogMeta = {
        version: now,
        last_sync: new Date().toISOString(),
        source: "models.dev",
        model_count: modelCount,
        provider_count: providerCount,
      };

      state.lastSyncAt = now;
      state.lastSyncOk = true;
      console.log(
        `[models-dev] synced ${modelCount} models from ${providerCount} providers`,
      );
      return meta;
    } catch (err) {
      const delay = Math.min(1000 * 2 ** attempt, 10_000);
      console.warn(
        `[models-dev] sync attempt ${attempt + 1}/${MAX_RETRIES} failed: ${err instanceof Error ? err.message : String(err)}, retry in ${delay}ms`,
      );
      if (attempt < MAX_RETRIES - 1) {
        await Bun.sleep(delay);
      }
    }
  }

  console.warn(
    "[models-dev] all sync attempts failed — serving existing catalog",
  );
  state.lastSyncAt = now;
  state.lastSyncOk = false;
  return null;
}

// ─── Transform ────────────────────────────────────────────────────────────

/**
 * Transform the provider-centric api.json into flat provider + model arrays.
 */
function transformProviders(
  raw: Record<string, ProviderCatalogEntry>,
): {
  providers: { id: string; display_name: string }[];
  models: ModelCatalog;
  providerCount: number;
  modelCount: number;
} {
  const providers: { id: string; display_name: string }[] = [];
  const models: ModelCatalog = {};

  for (const [pid, entry] of Object.entries(raw)) {
    providers.push({ id: pid, display_name: entry.name });
    for (const [mid, model] of Object.entries(entry.models)) {
      // Qualify the key with the provider slug so different providers
      // with the same bare model id (e.g. "gpt-4o") don't collide.
      models[`${pid}/${mid}`] = model;
    }
  }

  return {
    providers,
    models,
    providerCount: providers.length,
    modelCount: Object.keys(models).length,
  };
}

// ─── Database ─────────────────────────────────────────────────────────────

/**
 * Upsert providers + models (all in one transaction).
 */
async function upsertCatalog(
  pool: Pool,
  providers: { id: string; display_name: string }[],
  models: ModelCatalog,
  etag: string | null,
): Promise<void> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");

    // Bulk upsert for providers: one round-trip instead of N. Includes
    // provider rows derived from model ids that weren't in api.json.
    const providerRows = new Map<string, string>();
    for (const p of providers) providerRows.set(p.id, p.display_name);
    for (const mid of Object.keys(models)) {
      const pid = mid.includes("/") ? mid.split("/")[0]! : "unknown";
      if (!providerRows.has(pid)) providerRows.set(pid, pid);
    }
    await client.query(
      `INSERT INTO catalog_providers (id, display_name, updated_at)
       SELECT r.id, r.display_name, now()
       FROM jsonb_to_recordset($1::jsonb) AS r(id text, display_name text)
       ON CONFLICT (id) DO UPDATE SET
          display_name = EXCLUDED.display_name, updated_at = now()`,
      [JSON.stringify([...providerRows].map(([id, display_name]) => ({ id, display_name })))],
    );

    // Bulk upsert for models: one round-trip instead of ~600 sequential
    // INSERTs (models.dev ships 600+ entries; the O(N) loop wasted a
    // round-trip per row inside an already-open transaction).
    const modelRows: unknown[] = [];
    for (const [mid, model] of Object.entries(models)) {
      // Keys are now qualified as "provider/model-id" (e.g. "openai/gpt-4o").
      const providerId = mid.includes("/") ? mid.split("/")[0]! : "unknown";
      modelRows.push({
        id: mid,
        provider_id: providerId,
        name: model.name,
        description: model.description ?? "",
        family: model.family ?? null,
        modalities: model.modalities,
        capabilities: {
          tool_call: model.tool_call,
          reasoning: model.reasoning,
          structured_output: model.structured_output ?? false,
          attachment: model.attachment,
          temperature: model.temperature,
        },
        cost: model.cost ?? null,
        limits: model.limit ?? null,
        knowledge_cutoff: model.knowledge ?? null,
        release_date: model.release_date ?? null,
        open_weights: model.open_weights,
      });
    }
    await client.query(
      `INSERT INTO catalog_models (id, provider_id, name, description, family,
            modalities, capabilities, cost, limits, knowledge_cutoff,
            release_date, open_weights, updated_at)
       SELECT r.id, r.provider_id, r.name, r.description, r.family,
            r.modalities, r.capabilities, r.cost, r.limits,
            r.knowledge_cutoff, r.release_date,
            COALESCE(r.open_weights, false), now()
       FROM jsonb_to_recordset($1::jsonb) AS r(
            id text, provider_id text, name text, description text, family text,
            modalities jsonb, capabilities jsonb, cost jsonb, limits jsonb,
            knowledge_cutoff text, release_date text, open_weights boolean)
       ON CONFLICT (id) DO UPDATE SET
            provider_id = EXCLUDED.provider_id,
            name = EXCLUDED.name,
            description = EXCLUDED.description,
            family = EXCLUDED.family,
            modalities = EXCLUDED.modalities,
            capabilities = EXCLUDED.capabilities,
            cost = EXCLUDED.cost,
            limits = EXCLUDED.limits,
            knowledge_cutoff = EXCLUDED.knowledge_cutoff,
            release_date = EXCLUDED.release_date,
            open_weights = EXCLUDED.open_weights,
            updated_at = now()`,
      [JSON.stringify(modelRows)],
    );

    // Remove models that no longer appear in the catalog.
    const modelIds = Object.keys(models);
    if (modelIds.length > 0) {
      await client.query(
        `DELETE FROM catalog_models WHERE id != ALL($1)`,
        [modelIds],
      );
    }

    // Remove providers whose models have all been deleted (stale provider
    // cleanup). Must run AFTER model deletion so orphaned providers are
    // identified correctly.
    await client.query(
      `DELETE FROM catalog_providers WHERE id NOT IN (SELECT DISTINCT provider_id FROM catalog_models)`,
    );

    // Store ETag in catalog_meta (proper key-value, not a sentinel row).
    await client.query(
      `INSERT INTO catalog_meta (key, value, updated_at)
       VALUES ('etag', $1, now())
       ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = now()`,
      [etag ?? ""],
    );

    // Store sync timestamp.
    await client.query(
      `INSERT INTO catalog_meta (key, value, updated_at)
       VALUES ('last_sync_at', $1, now())
       ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = now()`,
      [String(Date.now())],
    );

    await client.query("COMMIT");
  } catch (err) {
    await client.query("ROLLBACK");
    throw err;
  } finally {
    client.release();
  }
}

async function getStoredEtag(pool: Pool): Promise<string | null> {
  const r = await pool.query(
    `SELECT value FROM catalog_meta WHERE key = 'etag'`,
  );
  if (r.rowCount === 0) return null;
  const val = r.rows[0].value as string;
  return val || null;
}

async function getCatalogMeta(
  pool: Pool,
  state: SyncState,
): Promise<CatalogMeta> {
  const counts = await pool.query(
    `SELECT
        (SELECT count(*) FROM catalog_providers) AS providers,
        (SELECT count(*) FROM catalog_models) AS models`,
  );
  const row = counts.rows[0] as { providers: string; models: string };
  return {
    version: state.lastSyncAt,
    last_sync: new Date(state.lastSyncAt).toISOString(),
    source: "models.dev",
    model_count: Number(row.models),
    provider_count: Number(row.providers),
  };
}

// ─── Public API ───────────────────────────────────────────────────────────

/**
 * Start the background sync loop. Called once from the control-plane boot.
 * Returns immediately; sync runs on the event loop.
 *
 * Sync state is closure-owned — no module-level singletons, safe for tests
 * that call startCatalogSync multiple times with independent pools.
 */
export function startCatalogSync(pool: Pool): ReturnType<typeof setInterval> {
  const state: SyncState = { lastSyncAt: 0, lastSyncOk: false };

  // Trigger first sync immediately (async, not blocking boot).
  syncCatalog(pool, state).catch((e) =>
    console.warn("[models-dev] initial sync failed:", e),
  );

  const handle = setInterval(() => {
    syncCatalog(pool, state).catch((e) =>
      console.warn("[models-dev] periodic sync failed:", e),
    );
  }, SYNC_TTL_MS);

  console.log("[models-dev] sync loop started (TTL 24h)");
  return handle;
}
