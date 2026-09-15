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
const GATEWAY_API_KEY = Bun.env["RELAYX_GATEWAY_API_KEY"];

// Validate the poll interval: an empty/garbage RELAXY_HEALTH_POLL_MS turns
// into a ~1ms watchdog tight loop (Number('')===0 → setInterval(fn, 0)).
const healthPollRaw = Bun.env["RELAYX_HEALTH_POLL_MS"] ?? "2000";
const HEALTH_POLL_MS = Number(healthPollRaw);
if (!Number.isFinite(HEALTH_POLL_MS) || HEALTH_POLL_MS < 200) {
  console.warn(
    `[watchdog] RELAXY_HEALTH_POLL_MS='${healthPollRaw}' is invalid — falling back to 2000ms`,
  );
}
// GWateway health poll: clamps to a sane minimum; a poll interval faster than
// 200ms serves no purpose and only hammers the gateway.
const GW_HEALTH_POLL_MS = Number.isFinite(HEALTH_POLL_MS) && HEALTH_POLL_MS >= 200 ? HEALTH_POLL_MS : 2000;

const pool = createPool(dbConfigFromEnv());

// Migrations are part of startup: a clean DB is reproducible, an existing
// DB gains new versions non-destructively.
const applied = await migrate(pool);
if (applied.length > 0) console.log(`[migrate] applied: ${applied.join(", ")}`);

// Seed one default project so API calls work out of the box.
await pool.query(
  "INSERT INTO projects (id, name) VALUES ('proj_default','default') ON CONFLICT (id) DO NOTHING",
);

const gateway = new GatewayClient(GATEWAY_ADMIN, GATEWAY_API_KEY);
const app = await buildApp({ pool, gateway });

/**
 * Republish the ACTIVE version of every workflow as one coherent bundle.
 * Shared by boot and the watchdog; the gateway restarts empty (cargo-watch,
 * crash, manual), and this republish is what restores service. Failures are
 * logged, not fatal: the control plane still serves CRUD. Returns whether
 * the publish call itself succeeded, so a caller can distinguish "nothing to
 * do / compile failed" from "gateway rejected the bundle".
 */
async function rehydrateGateway(label: string): Promise<{ ok: boolean }> {
  try {
    const activeFlows = await listActiveWorkflows(pool);
    if (activeFlows.length === 0) {
      console.warn(`[${label}] no active workflows to re-publish`);
      return { ok: true };
    }
    const wire = await buildCoherentWire(
      activeFlows,
      (id) => repo.lanes.get(pool, id),
      () => nextSnapshotVersion(pool),
    );
    if ("error" in wire) {
      console.warn(`[${label}] skipped: ${wire.error}`);
      return { ok: false };
    }
    const res = await gateway.publish(wire);
    if (res.ok) {
      console.info(
        `[${label}] republished ${activeFlows.length} workflow(s) as snapshot v${res.snapshot_version}`,
      );
      return { ok: true };
    }
    console.warn(`[${label}] publish failed: ${res.error}`);
    return { ok: false };
  } catch (e) {
    console.warn(`[${label}] skipped`, e);
    return { ok: false };
  }
}

// Boot re-hydrate: publish the active bundle so a cold-started gateway has
// state immediately. The outcome seeds the watchdog's health flags — a
// failed boot publish while the gateway WAS reachable must not leave the
// data plane empty (a later healthy tick would otherwise skip rehydrate
// because no down-transition was ever observed).
const boot = await rehydrateGateway("rehydrate");

try {
  await app.listen({ port: PORT, host: "0.0.0.0" });
} catch (err) {
  app.log.error(err);
  process.exit(1);
}

// Gateway watchdog: poll /healthz (unauthenticated, read-only). When the
// gateway comes back after being down (restart/recompile/crash), republish
// the active bundle so the data plane recovers without a CP restart.
let gatewayUpAtBoot = boot.ok;
let gatewayHealthy = boot.ok;
let rehydrating = false;

setInterval(async () => {
  // No re-entrancy: a rehydrate that outlasts the poll interval must not
  // overlap a second one (two concurrent publishes interleave snapshot
  // versions and the gateway can hot-swap onto a stale bundle).
  if (rehydrating) return;
  try {
    const r = await fetch(`${GATEWAY_ADMIN}/healthz`, {
      signal: AbortSignal.timeout(2_000),
    });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    if (!gatewayHealthy) {
      console.warn("[watchdog] gateway recovered — rehydrating");
      rehydrating = true;
      try {
        const res = await rehydrateGateway("watchdog");
        // Recovery is only complete once the bundle actually publishes
        // (#6); a failed rehydrate keeps `gatewayHealthy` false so the next
        // tick retries instead of silently staying degraded.
        gatewayHealthy = res.ok;
        if (!res.ok) console.warn("[watchdog] rehydrate did not publish — will retry");
      } finally {
        rehydrating = false;
      }
    } else if (!gatewayUpAtBoot) {
      // The boot publish ran before `app.listen`, and its failure was never
      // reflected in the health state. A freshly-started gateway that
      // recovers between the boot rehydrate and the first watchdog tick
      // would otherwise stay empty; this tick repairs it.
      gatewayUpAtBoot = true;
    }
  } catch {
    gatewayHealthy = false;
  }
}, GW_HEALTH_POLL_MS);

// Control plane is durable + independent of the data plane: leaving this
// running keeps serving CRUD; gateway publish cadence is driven by calls.
process.on("SIGINT", async () => {
  await app.close();
  await pool.end();
  process.exit(0);
});