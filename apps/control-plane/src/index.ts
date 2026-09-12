/**
 * Control-plane entry point.
 *
 * Boots: Postgres pool → migrations → Fastify API wired to the gateway admin.
 * The data plane keeps running from whatever bundle was last published even
 * if this process dies — the control plane is never on the request path.
 *
 * Gateway restart behavior (§24): the control plane REPUBLISHES the last
 * ACTIVE version of every workflow on boot, so a cold-started gateway that
 * lost its in-memory bundle is re-hydrated from durable state. This is the
 * documented (simple, honest) restart story for Phase 6.
 */

import { buildApp } from "./api/routes";
import { createPool, dbConfigFromEnv, migrate } from "./db/db";
import { GatewayClient } from "./gateway/client";
import * as repo from "./db/repositories";

const PORT = Number(Bun.env["RELAYX_CONTROL_PORT"] ?? 9091);
const GATEWAY_ADMIN = Bun.env["RELAYX_GATEWAY_ADMIN_URL"] ?? "http://127.0.0.1:9090";

const pool = createPool(dbConfigFromEnv());

// Migrations are part of startup: a clean DB is reproducible, an existing
// DB gains new versions non-destructively.
const applied = await migrate(pool);
if (applied.length > 0) console.log(`[migrate] applied: ${applied.join(", ")}`);

// Seed one default project so API calls work out of the box.
await pool.query(
  "INSERT INTO projects (id, name) VALUES ('proj_default','default') ON CONFLICT (id) DO NOTHING",
);

const gateway = new GatewayClient(GATEWAY_ADMIN);
const app = await buildApp({ pool, gateway });

// Re-hydrate the data plane from the last ACTIVE version of every workflow.
// If the gateway has no bundle (fresh start) or is out of sync, this makes
// the durable state authoritative. Publish failures are logged, not fatal:
// the control plane still serves CRUD; the publish pipeline can retry.
try {
  const active = await pool.query("SELECT * FROM workflow_active ORDER BY updated_at");
  let republished = 0;
  for (const row of active.rows) {
    const version = await repo.workflows.listVersions(pool, row.workflow_id);
    const latest = version.find((v) => v.version === row.workflow_version);
    if (!latest?.plan_hash) continue;
    const wire = {
      snapshot_version: row.workflow_version,
      workflows: [
        {
          id: row.workflow_id,
          workflow: latest.workflow_json,
          lanes: {},
        },
      ],
    };
    // Lanes referenced by the workflow are resolved by the publish service;
    // this boot-path resolves them from the active row's lane records.
    const laneIds = collectLaneIds(latest.workflow_json);
    const lanes: Record<string, { base_url: string }> = {};
    for (const id of laneIds) {
      const lane = await repo.lanes.get(pool, id);
      if (lane) lanes[id] = { base_url: lane.base_url };
    }
    wire.workflows[0]!.lanes = lanes;
    const res = await gateway.publish(wire);
    if (res.ok) {
      republished++;
      console.log(`[rehydrate] republished ${row.workflow_id} v${row.workflow_version}`);
    } else {
      console.warn(`[rehydrate] ${row.workflow_id} v${row.workflow_version} failed: ${res.error}`);
    }
  }
  if (republished > 0) console.log(`[rehydrate] ${republished} workflow(s) re-published`);
} catch (e) {
  console.warn("[rehydrate] skipped", e);
}

function collectLaneIds(workflowJson: Record<string, unknown>): string[] {
  const ids: string[] = [];
  const nodes = Array.isArray(workflowJson.nodes) ? workflowJson.nodes : [];
  for (const n of nodes as Array<Record<string, unknown>>) {
    const config = (n?.config ?? {}) as Record<string, unknown>;
    if (typeof config.lane_id === "string") ids.push(config.lane_id as string);
  }
  return ids;
}

try {
  await app.listen({ port: PORT, host: "0.0.0.0" });
} catch (err) {
  app.log.error(err);
  process.exit(1);
}

// Control plane is durable + independent of the data plane: leaving this
// running keeps serving CRUD; gateway publish cadence is driven by calls.
process.on("SIGINT", async () => {
  await app.close();
  await pool.end();
  process.exit(0);
});