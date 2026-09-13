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
import { buildCoherentWire, listActiveWorkflows, nextSnapshotVersion } from "./domain/publish";

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
// One coherent bundle over ALL active workflows (never single-workflow),
// credentials resolved, global snapshot version (review #1/#2/#9/#10).
// The gateway restarts empty; this republish is what restores service.
// Failures are logged, not fatal: the control plane still serves CRUD.
try {
  const activeFlows = await listActiveWorkflows(pool);
  if (activeFlows.length === 0) {
    console.log("[rehydrate] no active workflows to re-publish");
  } else {
    const wire = await buildCoherentWire(
      activeFlows,
      (id) => repo.lanes.get(pool, id),
      () => nextSnapshotVersion(pool),
    );
    if ("error" in wire) {
      console.warn(`[rehydrate] skipped: ${wire.error}`);
    } else {
      const res = await gateway.publish(wire);
      if (res.ok) {
        console.log(
          `[rehydrate] republished ${activeFlows.length} workflow(s) as snapshot v${res.snapshot_version}`,
        );
      } else {
        console.warn(`[rehydrate] publish failed: ${res.error}`);
      }
    }
  }
} catch (e) {
  console.warn("[rehydrate] skipped", e);
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