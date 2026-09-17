/**
 * Test helpers: a fresh Postgres database per suite (migrations applied),
 * an in-process mock gateway (fastify) acting as `/validate` + `/publish`.
 *
 * The real DB (127.0.0.1:5433) is used; tests never touch a production
 * database. Each test creates its own schema-namespace via a dedicated DB
 * (created once, migrations run once, then dropped) — per-test, because
 * tests assert empty-state and hard-coded row counts.
 *
 * Migration files are resolved from this file (never cwd), so the suite
 * runs from any working directory.
 */

import { createPool, dbConfigFromEnv, migrate } from "../src/db/db";
import { listActiveWorkflows, nextSnapshotVersion } from "../src/domain/publish";
import Fastify from "fastify";

/** The migrations dir resolved from this file, independent of cwd. */
const MIGRATIONS_DIR = new URL("../src/db/", import.meta.url).pathname;

/**
 * Create one throwaway database, run all migrations once, and hand back a
 * pool. `close()` ends the pool AND drops the database so repeated runs
 * don't pile up throwaway `test_*` databases (each CREATE DATABASE is
 * O(catalog), so the leak also slowed the suite over time). `WITH (FORCE)`
 * terminates straggler connections and drops immediately, so concurrent
 * suites don't serialize on lingering locks.
 */
export async function freshDb(name: string) {
  const admin = createPool({ ...dbConfigFromEnv(), database: "postgres" });
  const dbName = `test_${name}_${Date.now().toString(36)}`;
  try {
    await admin.query(`CREATE DATABASE "${dbName}"`);
  } catch (err) {
    await admin.end();
    throw err;
  }
  // Don't end `admin` here: the returned `close()` needs a live admin pool
  // to DROP the database. Ending it in a finally would make every drop
  // fail with "Cannot use a pool after calling end on the pool".
  const pool = createPool({ ...dbConfigFromEnv(), database: dbName });
  try {
    await migrate(pool, MIGRATIONS_DIR);
  } catch (err) {
    // A migration failure mid-setup must not leak the DB. End the pool
    // (releases its connections) then drop the half-migrated DB.
    await pool.end();
    await dropTestDb(admin, dbName);
    await admin.end();
    throw err;
  }
  return {
    pool,
    dbName,
    async close() {
      await pool.end();
      await dropTestDb(admin, dbName);
      await admin.end();
    },
  };
}

/** Best-effort drop of a throwaway `test_*` db; never fails the test run. */
async function dropTestDb(admin: import("pg").Pool, dbName: string) {
  try {
    // `WITH (FORCE)` terminates straggler connections and drops
    // immediately, so concurrent suites don't serialize on locks.
    await admin.query(`DROP DATABASE IF EXISTS "${dbName}" WITH (FORCE)`);
  } catch {
    // Last-resort best-effort cleanup must not fail the test run.
  }
}

/** Standard publish-service deps wired to the pool (routes use these too). */
export function publishDeps(pool: import("pg").Pool, gateway: { validate(p: unknown): any; publish(p: unknown): any }) {
  return {
    pool,
    getLane: async (id: string) => (await import("../src/db/repositories")).lanes.get(pool, id),
    listActiveWorkflows: (except?: string[]) => listActiveWorkflows(pool, except),
    nextSnapshotVersion: () => nextSnapshotVersion(pool),
    gateway,
  };
}

/** A deterministic, minimal in-process gateway admin. */
export async function mockGateway(opts: {
  mustValidate: boolean;
  failPublishWith?: string;
  failValidateWith?: string;
  failRunWith?: string;
  /** SSE chunks for `/run?stream=true` (written in order, with
   *  `streamChunkDelayMs` between). Terminal scenarios are expressed by the
   *  chunks themselves: include `event: done`/`event: error` for a terminal
   *  frame, or omit both to simulate a truncated stream. */
  streamChunks?: string[];
  streamChunkDelayMs?: number;
  /** When set, the mock `/run?stream=true` endpoint throws a 502-style
   *  response (gateway rejected the stream before any bytes), so the
   *  control-plane stream path sees `gateway.runStream` throw. */
  failStreamWith?: string;
}) {
  const app = Fastify();

  app.post("/validate", async (req, reply) => {
    if (opts.failValidateWith) return reply.code(400).send({ status: "error", error: opts.failValidateWith });
    const body = req.body as { snapshot_version?: number; workflows: Array<Record<string, unknown>> };
    return {
      status: "validated",
      snapshot_version: body.snapshot_version ?? 0,
      workflows: (body.workflows as Array<Record<string, unknown>>).map((w) => ({
        workflow_id: w.id,
        plan_hash: `sha256:${JSON.stringify(w.workflow).length}`,
        version: 1,
      })),
    };
  });

  app.post("/publish", async (req, reply) => {
    if (opts.failPublishWith) return reply.code(400).send({ status: "error", error: opts.failPublishWith });
    const body = req.body as { snapshot_version?: number; workflows: Array<Record<string, unknown>> };
    return {
      status: "published",
      snapshot_version: body.snapshot_version ?? 0,
      workflows: (body.workflows as Array<Record<string, unknown>>).map((w) => ({
        workflow_id: w.id,
        plan_hash: `sha256:${JSON.stringify(w.workflow).length}`,
        version: 1,
      })),
    };
  });

  app.post("/run", async (req, reply) => {
    if (opts.failRunWith) return reply.code(400).send({ status: "error", error: opts.failRunWith });
    const body = req.body as { workflow_id?: string; body?: unknown };
    // SSE streaming mode: pipe the configured chunks with an optional delay,
    // then end. The control-plane pump detects the terminal `event: done` /
    // `event: error` frames from the forwarded bytes.
    const query = (req.query ?? {}) as { stream?: string };
    if (query.stream === "true") {
      if (opts.failStreamWith) {
        return reply
          .code(502)
          .send({ error: opts.failStreamWith });
      }
      const chunks = opts.streamChunks ?? [
        "data: {\"delta\":\"hi\"}\n\n",
        "event: done\ndata: {}\n\n",
      ];
      const delay = opts.streamChunkDelayMs ?? 0;
      reply.raw.writeHead(200, { "content-type": "text/event-stream" });
      for (const chunk of chunks) {
        if (delay > 0) {
          // Sleep in 50 ms increments so the loop exits quickly when the
          // socket is destroyed (client abort, test teardown).
          let remaining = delay;
          while (remaining > 0 && !reply.raw.destroyed) {
            const step = Math.min(remaining, 50);
            await Bun.sleep(step);
            remaining -= step;
          }
        }
        if (reply.raw.destroyed) return;
        reply.raw.write(chunk);
      }
      reply.raw.end();
      return;
    }
    return {
      status: "ok",
      request_id: "req_run_1",
      workflow_id: body.workflow_id,
      snapshot_version: 7,
      plan_hash: "sha256:mock-run-plan",
      output: { ok: true, echoed: body.body },
    };
  });

  return app;
}