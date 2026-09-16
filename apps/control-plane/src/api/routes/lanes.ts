/**
 * Lane CRUD routes.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import * as repo from "../../db/repositories";
import { laneDtoSchema } from "../schemas";
import { projectIdOf } from "../utils/json-helpers";
import type { CredentialRef } from "../../secrets";

export function registerLaneRoutes(
  app: FastifyInstance,
  pool: Pool,
  defaultProjectId: string,
): void {
  app.get("/lanes", async (req) => repo.lanes.list(pool, projectIdOf(req, defaultProjectId)));

  app.post("/lanes", async (req, reply) => {
    const parsed = laneDtoSchema.safeParse(req.body);
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const laneData: {
      id?: string;
      project_id: string;
      provider_id: string | null;
      endpoint: string;
      base_url: string;
      egress: string;
      proxy_url: string | null;
      policies: string[];
      credential_ref: CredentialRef | null;
    } = {
      project_id: parsed.data.project_id,
      provider_id: parsed.data.provider_id ?? null,
      endpoint: parsed.data.endpoint,
      base_url: parsed.data.base_url,
      egress: parsed.data.egress,
      proxy_url: parsed.data.proxy_url ?? null,
      policies: parsed.data.policies,
      credential_ref: parsed.data.credential_ref ?? null,
    };
    if (parsed.data.id) laneData.id = parsed.data.id;
    const created = await repo.lanes.create(pool, laneData);
    return reply.code(201).send(created);
  });

  app.put("/lanes/:id", async (req, reply) => {
    const parsed = laneDtoSchema.partial().safeParse(req.body);
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const data = parsed.data;
    const patch: {
      provider_id?: string | null;
      endpoint?: string;
      base_url?: string;
      egress?: string;
      proxy_url?: string | null;
      policies?: string[];
      credential_ref?: CredentialRef | null;
    } = {};
    if (data.provider_id !== undefined) patch.provider_id = data.provider_id;
    if (data.endpoint !== undefined) patch.endpoint = data.endpoint;
    if (data.base_url !== undefined) patch.base_url = data.base_url;
    if (data.egress !== undefined) patch.egress = data.egress;
    if (data.proxy_url !== undefined) patch.proxy_url = data.proxy_url;
    if (data.policies !== undefined) patch.policies = data.policies;
    if (data.credential_ref !== undefined) patch.credential_ref = data.credential_ref;
    const updated = await repo.lanes.update(pool, (req.params as { id: string }).id, patch);
    if (!updated) return reply.code(404).send({ error: "lane not found" });
    return updated;
  });

  app.delete("/lanes/:id", async (req, reply) => {
    await repo.lanes.remove(pool, (req.params as { id: string }).id);
    return reply.code(204).send();
  });
}
