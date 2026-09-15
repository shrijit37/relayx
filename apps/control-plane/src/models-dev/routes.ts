/**
 * Model catalog routes — read-only API backed by the catalog tables.
 *
 * GET /catalog/status      — sync metadata
 * GET /catalog/models      — list models (filterable by provider, capability, search)
 * GET /catalog/providers   — list providers
 * GET /catalog/logos/:id   — SVG proxy (disk-cached)
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";

const LOGO_CACHE = new Map<string, { svg: string; fetchedAt: number }>();
const LOGO_TTL_MS = 60 * 60 * 1000; // 1h
const LOGO_BASE = "https://models.dev/logos";

export function registerCatalogRoutes(
  app: FastifyInstance,
  pool: Pool,
): void {
  // ── Status ────────────────────────────────────────────────────────
  app.get("/catalog/status", async () => {
    const counts = await pool.query(
      `SELECT
          (SELECT count(*) FROM catalog_providers) AS providers,
          (SELECT count(*) FROM catalog_models) AS models`,
    );
    const row = counts.rows[0] as { providers: string; models: string };
    const etagRow = await pool.query(
      `SELECT value, updated_at FROM catalog_meta WHERE key = 'last_sync_at'`,
    );
    const meta = etagRow.rows[0] as
      | { value: string; updated_at: string }
      | undefined;
    return {
      version: meta ? meta.updated_at : "never",
      last_sync: meta ? new Date(Number(meta.value)).toISOString() : "never",
      source: "models.dev",
      model_count: Number(row.models),
      provider_count: Number(row.providers),
    };
  });

  // ── Models ────────────────────────────────────────────────────────
  app.get("/catalog/models", async (req) => {
    const q = req.query as Record<string, string>;
    const provider = q.provider ?? null;
    const capability = q.capability ?? null;
    const search = q.search ?? null;

    let sql = `
        SELECT m.id, m.name, m.description, m.family, m.modalities,
               m.capabilities, m.cost, m.limits, m.knowledge_cutoff,
               m.release_date, m.open_weights, m.provider_id,
               p.display_name AS provider_name
        FROM catalog_models m
        LEFT JOIN catalog_providers p ON p.id = m.provider_id
        WHERE 1=1`;
    const params: string[] = [];
    let i = 1;

    if (provider) {
      sql += ` AND m.provider_id = $${i++}`;
      params.push(provider);
    }
    if (capability) {
      sql += ` AND (m.capabilities->>$${i++})::boolean = true`;
      params.push(capability);
    }
    if (search) {
      sql += ` AND (m.name ILIKE $${i} OR m.id ILIKE $${i} OR m.description ILIKE $${i})`;
      params.push(`%${search}%`);
      i++;
    }

    sql += " ORDER BY p.display_name, m.name";
    sql += " LIMIT 500";

    const result = await pool.query(sql, params);
    return result.rows;
  });

  // ── Providers ─────────────────────────────────────────────────────
  app.get("/catalog/providers", async () => {
    const result = await pool.query(
      `SELECT id, display_name, logo_path, updated_at
       FROM catalog_providers
       ORDER BY display_name`,
    );
    return result.rows;
  });

  // ── Logo proxy ────────────────────────────────────────────────────
  app.get("/catalog/logos/:id", async (req, reply) => {
    const { id } = req.params as { id: string };
    const safeId = id.replace(/\.svg$/, "").replace(/[^a-zA-Z0-9_\-]/g, "");

    const cached = LOGO_CACHE.get(safeId);
    if (cached && Date.now() - cached.fetchedAt < LOGO_TTL_MS) {
      return reply
        .header("Content-Type", "image/svg+xml")
        .header("Cache-Control", "public, max-age=3600")
        .send(cached.svg);
    }

    try {
      const url = `${LOGO_BASE}/${safeId}.svg`;
      const resp = await fetch(url, {
        signal: AbortSignal.timeout(5_000),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const svg = await resp.text();
      LOGO_CACHE.set(safeId, { svg, fetchedAt: Date.now() });
      return reply
        .header("Content-Type", "image/svg+xml")
        .header("Cache-Control", "public, max-age=3600")
        .send(svg);
    } catch {
      return reply.code(404).send({ error: "logo not found" });
    }
  });
}
