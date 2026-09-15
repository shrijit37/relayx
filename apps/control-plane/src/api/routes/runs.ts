/**
 * Run-history routes: GET /runs (list, optional workflow filter) and
 * GET /runs/:id (single record). Runs are written by the workflow run
 * handler in `routes/workflows.ts` — this module only serves them.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import * as repo from "../../db/repositories";

export function registerRunRoutes(app: FastifyInstance, pool: Pool): void {
  app.get("/runs", async (req) => {
    const { workflow_id } = (req.query ?? {}) as { workflow_id?: string };
    return repo.runs.list(pool, workflow_id);
  });

  app.get("/runs/:id", async (req, reply) => {
    const run = await repo.runs.get(pool, (req.params as { id: string }).id);
    if (!run) return reply.code(404).send({ error: "run not found" });
    return run;
  });
}
