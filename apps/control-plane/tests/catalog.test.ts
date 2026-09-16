/**
 * Catalog tests: migration, transform, API filters, sync state isolation.
 *
 * Uses the same real-Postgres test harness as other CP tests.
 */

import { afterEach, beforeEach, test, expect } from "bun:test";
import Fastify from "fastify";
import { freshDb } from "./helpers";

let db: Awaited<ReturnType<typeof freshDb>> | null = null;
let app: Awaited<ReturnType<typeof Fastify>> | null = null;

beforeEach(async () => {
  // Empty-state tests must see a truly empty DB, so each test gets a fresh
  // throwaway database (created once, migrations run once, then dropped).
  db = await freshDb("catalog");
  // buildApp calls startCatalogSync — we DON'T want the real network sync
  // running in tests, so we use a separate pool without the sync.
  // Instead, we test the routes against the tables directly.
  app = Fastify({ logger: false });
  // Register only the catalog routes (skip the full app wiring which
  // requires a mock gateway).
  const { registerCatalogRoutes } = await import(
    "../src/models-dev/routes"
  );
  registerCatalogRoutes(app, db.pool);
  await app.listen({ port: 0, host: "127.0.0.1" });
});

afterEach(async () => {
  await app?.close();
  await db?.close();
});

const base = () => {
  const p = app!.server.address() as { port: number };
  return `http://127.0.0.1:${p.port}`;
};

// ─── Seed helpers ──────────────────────────────────────────────────────

async function seedCatalog(pool: import("pg").Pool) {
  await pool.query(
    `INSERT INTO catalog_providers (id, display_name, updated_at)
     VALUES ('openai', 'OpenAI', now()), ('anthropic', 'Anthropic', now())
     ON CONFLICT (id) DO NOTHING`,
  );
  await pool.query(
    `INSERT INTO catalog_models
       (id, provider_id, name, description, family, modalities,
        capabilities, cost, limits, knowledge_cutoff, release_date,
        open_weights, updated_at)
     VALUES
       ('openai/gpt-4o', 'openai', 'GPT-4o', 'Flagship', 'gpt4',
        '{"input":["text","image"],"output":["text"]}',
        '{"tool_call":true,"reasoning":false,"structured_output":true,"attachment":false,"temperature":true}',
        '{"input":5,"output":15}', '{"context":128000,"output":16384}',
        '2024-10', '2024-08', true, now()),
       ('anthropic/claude-sonnet-4-6', 'anthropic', 'Claude Sonnet 4.6', 'Sonnet', 'claude',
        '{"input":["text","image"],"output":["text"]}',
        '{"tool_call":true,"reasoning":true,"structured_output":true,"attachment":false,"temperature":true}',
        '{"input":3,"output":15}', '{"context":200000,"output":8192}',
        '2025-06', '2025-06', false, now()),
       ('openai/gpt-4o-mini', 'openai', 'GPT-4o Mini', 'Small and fast', 'gpt4',
        '{"input":["text"],"output":["text"]}',
        '{"tool_call":true,"reasoning":false,"structured_output":false,"attachment":false,"temperature":true}',
        '{"input":0.15,"output":0.6}', '{"context":128000,"output":16384}',
        '2024-10', '2024-08', true, now())
     ON CONFLICT (id) DO NOTHING`,
  );
}

// ─── Migration tests ──────────────────────────────────────────────────

test("migration 003 creates catalog tables", async () => {
  // freshDb already runs all migrations. Verify the tables exist.
  const pool = db!.pool;
  const r = await pool.query(
    `SELECT table_name FROM information_schema.tables
     WHERE table_schema = 'public'
       AND table_name IN ('catalog_providers', 'catalog_models', 'catalog_meta')
     ORDER BY table_name`,
  );
  const names = r.rows.map((row: { table_name: string }) => row.table_name);
  expect(names).toContain("catalog_providers");
  expect(names).toContain("catalog_models");
  expect(names).toContain("catalog_meta");
});

test("migration is idempotent (re-run does not fail)", async () => {
  // Fresh DB already has migration applied; apply it again. The directory
  // is resolved from db.ts (never cwd), so this works from any dir.
  const { migrate } = await import("../src/db/db");
  const pool = db!.pool;
  const applied = await migrate(pool);
  expect(applied).toEqual([]);
});

// ─── API: empty state ─────────────────────────────────────────────────

test("GET /catalog/models returns empty array when no data", async () => {
  const resp = await fetch(`${base()}/catalog/models`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body).toEqual([]);
});

test("GET /catalog/providers returns empty array when no data", async () => {
  const resp = await fetch(`${base()}/catalog/providers`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body).toEqual([]);
});

test("GET /catalog/status returns zero counts when empty", async () => {
  const resp = await fetch(`${base()}/catalog/status`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.model_count).toBe(0);
  expect(body.provider_count).toBe(0);
  expect(body.source).toBe("models.dev");
});

// ─── API: with data ───────────────────────────────────────────────────

test("GET /catalog/models returns all models", async () => {
  await seedCatalog(db!.pool);
  const resp = await fetch(`${base()}/catalog/models`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(3);
  expect(body[0].id).toBeDefined();
  expect(body[0].provider_name).toBeDefined();
});

test("GET /catalog/models filters by provider", async () => {
  await seedCatalog(db!.pool);
  const resp = await fetch(`${base()}/catalog/models?provider=openai`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(2);
  for (const m of body) {
    expect(m.provider_id).toBe("openai");
  }
});

test("GET /catalog/models filters by provider display name (not just slug)", async () => {
  // The web picker sends the provider row's display name ("Anthropic"), not
  // the models.dev slug ("anthropic"). The route must accept both.
  await seedCatalog(db!.pool);
  const resp = await fetch(`${base()}/catalog/models?provider=Anthropic`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(1);
  expect(body[0].id).toBe("anthropic/claude-sonnet-4-6");
});

test("GET /catalog/models returns ALL models (no silent LIMIT truncation)", async () => {
  // Regression guard for the LIMIT 500 bug: seed more than 500 models and
  // assert every one comes back.
  const pool = db!.pool;
  await seedCatalog(pool);
  await pool.query(
    `INSERT INTO catalog_providers (id, display_name, updated_at)
     VALUES ('bulk', 'Bulk Provider', now()) ON CONFLICT (id) DO NOTHING`,
  );
  const values: string[] = [];
  const inserts: string[] = [];
  for (let i = 0; i < 600; i++) {
    const id = `bulk/model-${i}`;
    inserts.push(
      `($${values.length + 1}, 'bulk', 'Bulk Model ${i}', 'bulk', 'bulk',
        '{"input":["text"],"output":["text"]}',
        '{"tool_call":false,"reasoning":false,"structured_output":false,"attachment":false,"temperature":true}',
        NULL, NULL, NULL, NULL, false, now())`,
    );
    values.push(id);
  }
  await pool.query(
    `INSERT INTO catalog_models
       (id, provider_id, name, description, family, modalities,
        capabilities, cost, limits, knowledge_cutoff, release_date,
        open_weights, updated_at)
     VALUES ${inserts.join(",")}
     ON CONFLICT (id) DO NOTHING`,
    values,
  );

  const resp = await fetch(`${base()}/catalog/models`);
  expect(resp.ok).toBe(true);
  const body = (await resp.json()) as Array<{ id: string }>;
  // 3 seeded + 600 bulk = 603; a LIMIT 500 would have returned 500.
  expect(body.length).toBe(603);
});

test("GET /catalog/models filters by capability", async () => {
  await seedCatalog(db!.pool);
  // claude-sonnet-4-6 has reasoning=true, gpt-4o does not
  const resp = await fetch(`${base()}/catalog/models?capability=reasoning`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(1);
  expect(body[0].id).toBe("anthropic/claude-sonnet-4-6");
});

test("GET /catalog/models filters by search", async () => {
  await seedCatalog(db!.pool);
  const resp = await fetch(`${base()}/catalog/models?search=Mini`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(1);
  expect(body[0].id).toBe("openai/gpt-4o-mini");
});

test("GET /catalog/models combines provider + capability filters", async () => {
  await seedCatalog(db!.pool);
  // OpenAI + reasoning: none match (only Anthropic has reasoning=true)
  const resp = await fetch(
    `${base()}/catalog/models?provider=openai&capability=reasoning`,
  );
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(0);
});

test("GET /catalog/providers returns seeded providers", async () => {
  await seedCatalog(db!.pool);
  const resp = await fetch(`${base()}/catalog/providers`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.length).toBe(2);
  const names = body.map((p: { display_name: string }) => p.display_name);
  expect(names).toContain("OpenAI");
  expect(names).toContain("Anthropic");
});

test("GET /catalog/status reflects seeded data", async () => {
  await seedCatalog(db!.pool);
  const resp = await fetch(`${base()}/catalog/status`);
  expect(resp.ok).toBe(true);
  const body = await resp.json();
  expect(body.model_count).toBe(3);
  expect(body.provider_count).toBe(2);
  expect(body.source).toBe("models.dev");
});

// ─── Sync: transform + upsert (stubbed fetch, no network) ─────────────

const FIXTURE_API_JSON = {
  openai: {
    id: "openai",
    name: "OpenAI",
    models: {
      "gpt-4o": {
        id: "gpt-4o",
        name: "GPT-4o",
        description: "Flagship",
        attachment: false,
        reasoning: false,
        structured_output: true,
        temperature: true,
        tool_call: true,
        modalities: { input: ["text", "image"], output: ["text"] },
        open_weights: false,
        limit: { context: 128000, output: 16384 },
        cost: { input: 5, output: 15 },
      },
    },
  },
};

test("syncCatalog upserts models from a stubbed fetch", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (() =>
    Promise.resolve(
      new Response(JSON.stringify(FIXTURE_API_JSON), {
        status: 200,
        headers: { ETag: '"abc123"' },
      }),
    )) as unknown as typeof fetch;
  try {
    const { syncCatalog } = await import("../src/models-dev/sync");
    const state = { lastSyncAt: 0, lastSyncOk: false };
    const meta = await syncCatalog(db!.pool, state);

    expect(meta).not.toBeNull();
    expect(meta?.model_count).toBe(1);
    expect(meta?.provider_count).toBe(1);
    expect(meta?.source).toBe("models.dev");

    // ETag stored in catalog_meta, NOT as a provider sentinel row.
    const etag = await db!.pool.query(
      `SELECT value FROM catalog_meta WHERE key = 'etag'`,
    );
    expect(etag.rows[0]?.value).toBe('"abc123"');
    const fakeProvider = await db!.pool.query(
      `SELECT count(*) FROM catalog_providers WHERE id = '__etag'`,
    );
    expect(Number(fakeProvider.rows[0]?.count)).toBe(0);

    // Model was inserted with the qualified key "openai/gpt-4o"
    // (transformProviders qualifies bare IDs with the provider slug).
    const model = await db!.pool.query(
      `SELECT id, modalities, cost FROM catalog_models WHERE id = 'openai/gpt-4o'`,
    );
    expect(model.rows[0]?.id).toBe("openai/gpt-4o");
    expect(model.rows[0]?.modalities).toEqual({
      input: ["text", "image"],
      output: ["text"],
    });
    expect(model.rows[0]?.cost).toEqual({ input: 5, output: 15 });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("syncCatalog returns null and leaves DB intact on fetch failure", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (() =>
    Promise.reject(new Error("network down"))) as unknown as typeof fetch;
  try {
    // Seed data first so we can verify it survives a failed sync.
    await seedCatalog(db!.pool);
    const { syncCatalog } = await import("../src/models-dev/sync");
    const state = { lastSyncAt: 0, lastSyncOk: false };
    const meta = await syncCatalog(db!.pool, state);

    // Fail-open: no meta returned, DB untouched.
    expect(meta).toBeNull();
    const count = await db!.pool.query(
      `SELECT count(*) FROM catalog_models`,
    );
    expect(Number(count.rows[0]?.count)).toBe(3);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("syncCatalog skips when TTL has not elapsed", async () => {
  const originalFetch = globalThis.fetch;
  let fetchCalls = 0;
  globalThis.fetch = (() => {
    fetchCalls++;
    return Promise.resolve(
      new Response(JSON.stringify(FIXTURE_API_JSON), {
        status: 200,
        headers: { ETag: '"abc"' },
      }),
    );
  }) as unknown as typeof fetch;
  try {
    const { syncCatalog } = await import("../src/models-dev/sync");
    const state = { lastSyncAt: Date.now(), lastSyncOk: true };
    const meta = await syncCatalog(db!.pool, state);
    // TTL not elapsed → short-circuits, no network, null returned.
    expect(meta).toBeNull();
    expect(fetchCalls).toBe(0);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
