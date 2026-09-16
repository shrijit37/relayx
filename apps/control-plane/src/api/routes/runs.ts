/**
 * Run-history routes: GET /runs (list, optional workflow/project filter) and
 * GET /runs/:id (single record). Runs are written by the workflow run
 * handler in `routes/workflows.ts` — this module only serves them.
 *
 * SECURITY: run-history data includes user prompts and model outputs.
 * The `project_id` filter scopes results to a project boundary. Full
 * control-plane auth (API key / session) is tracked as a follow-up.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import * as repo from "../../db/repositories";

export function registerRunRoutes(app: FastifyInstance, pool: Pool): void {
  app.get("/runs", async (req) => {
    const { workflow_id, project_id } = (req.query ?? {}) as {
      workflow_id?: string;
      project_id?: string;
    };
    return repo.runs.list(pool, workflow_id, project_id);
  });

  app.get("/runs/:id", async (req, reply) => {
    const run = await repo.runs.get(pool, (req.params as { id: string }).id);
    if (!run) return reply.code(404).send({ error: "run not found" });
    return run;
  });
}
