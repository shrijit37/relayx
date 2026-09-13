/**
 * Test helpers: a fresh Postgres database per suite (migrations applied),
 * an in-process mock gateway (fastify) acting as `/validate` + `/publish`.
 *
 * The real DB (127.0.0.1:5433) is used; tests never touch a production
 * database. Each test creates its own schema-namespace via a dedicated DB.
 */

import { createPool, dbConfigFromEnv, migrate } from "../src/db/db";
import { listActiveWorkflows, nextSnapshotVersion } from "../src/domain/publish";
import Fastify from "fastify";

export async function freshDb(name: string) {
  const admin = createPool({ ...dbConfigFromEnv(), database: "postgres" });
  const dbName = `test_${name}_${Date.now().toString(36)}`;
  await admin.query(`CREATE DATABASE "${dbName}"`);
  await admin.end();

  const pool = createPool({ ...dbConfigFromEnv(), database: dbName });
  await migrate(pool, "./src/db");
  return { pool, dbName, async close() { await pool.end(); } };
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