/**
 * Control-plane route wiring.
 *
 * API → service → repository → PostgreSQL. Routes are thin; the publish
 * service owns the pipeline. The only external caller is the frontend; the
 * only external receiver is the gateway admin API.
 *
 * Route registrations are split by resource into `routes/`:
 * - `routes/health.ts` — health probes
 * - `routes/workflows.ts` — workflow CRUD + validate/publish/rollback/run
 * - `routes/providers.ts` — provider CRUD
 * - `routes/lanes.ts` — lane CRUD
 */

import Fastify, { type FastifyInstance } from "fastify";
import cors from "@fastify/cors";
import type { Pool } from "pg";
import * as repo from "../db/repositories";
import {
  createPublishService,
  listActiveWorkflows,
  nextSnapshotVersion,
} from "../domain/publish";
import type { GatewayClient } from "../gateway/client";
import { registerHealthRoutes } from "./routes/health";
import { registerWorkflowRoutes } from "./routes/workflows";
import { registerProviderRoutes } from "./routes/providers";
import { registerLaneRoutes } from "./routes/lanes";
import { registerCatalogRoutes } from "../models-dev/routes";

export async function buildApp(opts: {
  pool: Pool;
  gateway: GatewayClient;
  defaultProjectId?: string;
}): Promise<FastifyInstance> {
  const { pool, gateway, defaultProjectId = "proj_default" } = opts;

  // Seed the default project once so the API is usable out of the box
  // (idempotent: ON CONFLICT DO NOTHING).
  await pool.query(
    "INSERT INTO projects (id, name) VALUES ($1, 'default') ON CONFLICT (id) DO NOTHING",
    [defaultProjectId],
  );

  const publish = createPublishService({
    pool,
    getLane: (id) => repo.lanes.get(pool, id),
    listActiveWorkflows: (except) => listActiveWorkflows(pool, except),
    nextSnapshotVersion: () => nextSnapshotVersion(pool),
    gateway,
  });

  const app = Fastify({ logger: true });
  await app.register(cors, { origin: true });

  registerHealthRoutes(app, gateway);
  registerWorkflowRoutes(app, { pool, gateway, publish });
  registerProviderRoutes(app, pool, defaultProjectId);
  registerLaneRoutes(app, pool, defaultProjectId);
  registerCatalogRoutes(app, pool);

  return app;
}
