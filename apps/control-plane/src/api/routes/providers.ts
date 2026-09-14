/**
 * Provider CRUD routes.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import * as repo from "../../db/repositories";
import { providerDtoSchema } from "../schemas";
import { projectIdOf } from "../utils/json-helpers";

export function registerProviderRoutes(
  app: FastifyInstance,
  pool: Pool,
  defaultProjectId: string,
): void {
  app.get("/providers", async (req) =>
    repo.providers.list(pool, projectIdOf(req, defaultProjectId)),
  );

  app.post("/providers", async (req, reply) => {
    const parsed = providerDtoSchema.safeParse(req.body);
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const created = await repo.providers.create(pool, {
      project_id: defaultProjectId,
      ...parsed.data,
    });
    return reply.code(201).send(created);
  });

  app.put("/providers/:id", async (req, reply) => {
    const parsed = providerDtoSchema.partial().safeParse(req.body);
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const patch: { name?: string; protocol?: string; base_url?: string; model?: string } = {};
    if (parsed.data.name !== undefined) patch.name = parsed.data.name;
    if (parsed.data.protocol !== undefined) patch.protocol = parsed.data.protocol;
    if (parsed.data.base_url !== undefined) patch.base_url = parsed.data.base_url;
    if (parsed.data.model !== undefined) patch.model = parsed.data.model;
    const updated = await repo.providers.update(pool, (req.params as { id: string }).id, patch);
    if (!updated) return reply.code(404).send({ error: "provider not found" });
    return updated;
  });

  app.delete("/providers/:id", async (req, reply) => {
    await repo.providers.remove(pool, (req.params as { id: string }).id);
    return reply.code(204).send();
  });
}
