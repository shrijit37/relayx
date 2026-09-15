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
      models[mid] = model;
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

    for (const p of providers) {
      await client.query(
        `INSERT INTO catalog_providers (id, display_name, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (id) DO UPDATE SET display_name = $2, updated_at = now()`,
        [p.id, p.display_name],
      );
    }

    // Collect all model IDs so we can delete stale ones after upsert.
    const modelIds: string[] = [];

    for (const [mid, model] of Object.entries(models)) {
      const providerId = mid.includes("/") ? mid.split("/")[0] : "unknown";
      // Ensure the provider row exists (handles models whose provider
      // wasn't in api.json or has a different shape).
      if (!providers.find((p) => p.id === providerId)) {
        await client.query(
          `INSERT INTO catalog_providers (id, display_name, updated_at)
           VALUES ($1, $1, now())
           ON CONFLICT (id) DO NOTHING`,
          [providerId],
        );
      }
      modelIds.push(mid);

      await client.query(
        `INSERT INTO catalog_models (id, provider_id, name, description, family,
            modalities, capabilities, cost, limits, knowledge_cutoff,
            release_date, open_weights, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12, now())
         ON CONFLICT (id) DO UPDATE SET
            provider_id=$2, name=$3, description=$4, family=$5,
            modalities=$6, capabilities=$7, cost=$8, limits=$9,
            knowledge_cutoff=$10, release_date=$11, open_weights=$12, updated_at=now()`,
        [
          mid,
          providerId,
          model.name,
          model.description,
          model.family ?? null,
          JSON.stringify(model.modalities),
          JSON.stringify({
            tool_call: model.tool_call,
            reasoning: model.reasoning,
            structured_output: model.structured_output ?? false,
            attachment: model.attachment,
            temperature: model.temperature,
          }),
          model.cost ? JSON.stringify(model.cost) : null,
          model.limit ? JSON.stringify(model.limit) : null,
          model.knowledge ?? null,
          model.release_date ?? null,
          model.open_weights,
        ],
      );
    }

    // Remove models that no longer appear in the catalog.
    if (modelIds.length > 0) {
      await client.query(
        `DELETE FROM catalog_models WHERE id != ALL($1)`,
        [modelIds],
      );
    }

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
export function startCatalogSync(pool: Pool): void {
  const state: SyncState = { lastSyncAt: 0, lastSyncOk: false };

  // Trigger first sync immediately (async, not blocking boot).
  syncCatalog(pool, state).catch((e) =>
    console.warn("[models-dev] initial sync failed:", e),
  );

  setInterval(() => {
    syncCatalog(pool, state).catch((e) =>
      console.warn("[models-dev] periodic sync failed:", e),
    );
  }, SYNC_TTL_MS);

  console.log("[models-dev] sync loop started (TTL 24h)");
}
