/**
 * Workflow CRUD, validation, publish, rollback, and run routes.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import * as repo from "../../db/repositories";
import { createPublishService } from "../../domain/publish";
import type { GatewayClient } from "../../gateway/client";
import { createWorkflowSchema, publishSchema } from "../schemas";
import { versionForJson } from "../utils/json-helpers";

export type WorkflowDeps = {
  pool: Pool;
  gateway: GatewayClient;
  publish: ReturnType<typeof createPublishService>;
};

export function registerWorkflowRoutes(app: FastifyInstance, deps: WorkflowDeps): void {
  const { pool, gateway, publish } = deps;

  // ── Workflows CRUD ──────────────────────────────────────────────────
  app.get("/workflows", async () =>
    (await pool.query("SELECT * FROM workflows ORDER BY created_at DESC")).rows,
  );

  app.post("/workflows", async (req, reply) => {
    const parsed = createWorkflowSchema.safeParse(req.body);
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    // The frontend may create its durable row UNDER the editor's own id
    // (e.g. "production-gateway") so later GET/POST /workflows/:id match.
    // Without this, every publish creates an orphan UUID row (review #3).
    const id = parsed.data.id !== undefined ? parsed.data.id : undefined;
    const wf = await repo.workflows.create(pool, parsed.data.project_id, parsed.data.name, id);
    return reply.code(201).send(wf);
  });

  app.get("/workflows/:id", async (req, reply) => {
    const wf = await repo.workflows.get(pool, (req.params as { id: string }).id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    return wf;
  });

  app.put("/workflows/:id", async (req, reply) => {
    const { id } = req.params as { id: string };
    const body = req.body as { name?: string };
    if (!body.name) return reply.code(400).send({ error: "name required" });
    const wf = await repo.workflows.update(pool, id, body.name);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    return wf;
  });

  app.get("/workflows/:id/versions", async (req, reply) => {
    const { id } = req.params as { id: string };
    if (!(await repo.workflows.get(pool, id)))
      return reply.code(404).send({ error: "workflow not found" });
    return repo.workflows.listVersions(pool, id);
  });

  app.post("/workflows/:id/versions", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const body = (req.body ?? {}) as { workflow_json?: Record<string, unknown> };
    // Atomic version allocation avoids the concurrent-publish race (D2).
    const v = await repo.workflows.createNextVersion(pool, id, body.workflow_json ?? {});
    if (wf.status !== "active") await repo.workflows.setStatus(pool, id, "draft");
    return reply.code(201).send(v);
  });

  /** Validate + compile against the gateway without publishing. */
  const validateCompile = async (
    req: import("fastify").FastifyRequest,
    reply: import("fastify").FastifyReply,
  ) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const body = (req.body ?? {}) as {
      workflow_json?: Record<string, unknown>;
      version?: number;
    };
    const workflowJson = body.workflow_json ?? {};
    // Resolve the version whose stored JSON MATCHES the submitted content, so
    // the recorded plan_hash always belongs to the version it is stored under
    // (review #5). No match → refuse rather than hash mismatched content.
    const version = body.version ?? (await versionForJson(pool, id, workflowJson));
    if (version === null) {
      return reply.code(400).send({
        error: "no stored version matches the submitted workflow_json (create a version first)",
      });
    }
    const wire = await publish.buildWire({ workflowId: id, workflowJson, version });
    if ("error" in wire) return reply.code(400).send({ error: wire.error });
    const result = await gateway.validate(wire);
    if (!result.ok) return reply.code(400).send({ error: result.error });
    const planHash =
      result.workflows.find((w) => w.workflow_id === id)?.plan_hash ?? null;
    await repo.workflows.updateVersionStatus(pool, id, version, "compiled", planHash);
    // A validate/compile must NOT degrade an active parent (review #9).
    if (wf.status !== "active") await repo.workflows.setStatus(pool, id, "compiled");
    return {
      status: "compiled",
      snapshot_version: result.snapshot_version,
      plan_hash: planHash,
      workflows: result.workflows,
    };
  };

  app.post("/workflows/:id/validate", validateCompile);
  app.post("/workflows/:id/compile", validateCompile);

  app.post("/workflows/:id/publish", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const parsed = publishSchema.safeParse(req.body ?? {});
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const version =
      parsed.data.version ?? (await versionForJson(pool, id, parsed.data.workflow_json));
    if (version === null) {
      return reply.code(400).send({
        error: "no stored version matches the submitted workflow_json (create a version first)",
      });
    }
    const result = await publish.publish({
      workflowId: id,
      version,
      workflowJson: parsed.data.workflow_json,
    });
    if (result.status === "error") return reply.code(400).send({ error: result.error });
    return {
      status: "published",
      workflow_id: id,
      workflow_version: version,
      snapshot_version: result.snapshot_version,
      plan_hash: result.plan_hash,
      published_at: result.published_at,
    };
  });

  app.post("/workflows/:id/rollback", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const active = await repo.workflows.getActiveVersion(pool, id);
    if (!active) return reply.code(409).send({ error: "no active version to roll back from" });
    // Roll back only to a version that was genuinely validated/compiled
    // (status), never to a raw draft that may carry a stale plan_hash from
    // the COALESCE in updateVersionStatus (review D1).
    const previous = (await repo.workflows.listVersions(pool, id))
      .filter(
        (v) =>
          v.version < active.workflow_version &&
          (v.status === "compiled" || v.status === "active" || v.status === "published") &&
          Boolean(v.plan_hash),
      )
      .sort((a, b) => b.version - a.version)[0];
    if (!previous)
      return reply.code(409).send({ error: "no prior validated version to republish" });

    // Rollback = publish a previous valid version (spec §18), never a mutation.
    const result = await publish.publish({
      workflowId: id,
      version: previous.version,
      workflowJson: previous.workflow_json,
    });
    if (result.status === "error") return reply.code(500).send({ error: result.error });
    return {
      status: "rolled_back",
      to_version: previous.version,
      snapshot_version: result.snapshot_version,
      plan_hash: result.plan_hash,
    };
  });

  app.post("/workflows/:id/run", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });

    // Run only ever executes the ACTIVE (published) version. A draft/dirty
    // canvas is never silently published or exec'd (§11). The ACTIVE check is
    // the authoritative backend gate, not a frontend invention.
    const active = await repo.workflows.getActiveVersion(pool, id);
    if (!active) {
      return reply
        .code(409)
        .send({ error: "Workflow must be published before it can be run." });
    }

    const body = ((req.body ?? {}) as { body?: unknown }).body ?? null;

    // ?stream=true pipes the gateway's SSE token stream through to the
    // browser without buffering.
    const { stream } = (req.query ?? {}) as { stream?: string };
    if (stream === "true") {
      const gatewayResp = await gateway.runStream({ workflow_id: id, body });
      const upstream = gatewayResp.body;
      if (!upstream) {
        return reply
          .code(502)
          .send({ error: "gateway returned an empty stream" });
      }
      reply.hijack();
      reply.raw.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      });
      const reader = upstream.getReader();
      const pump = async () => {
        try {
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            if (!reply.raw.destroyed) reply.raw.write(value);
          }
        } finally {
          if (!reply.raw.destroyed) reply.raw.end();
        }
      };
      pump().catch(() => reader.cancel());
      return reply;
    }

    const result = await gateway.run({ workflow_id: id, body });
    if (!result.ok) return reply.code(400).send({ error: result.error });
    return {
      status: "ok",
      request_id: result.request_id,
      workflow_id: result.workflow_id,
      workflow_version: active.workflow_version,
      snapshot_version: result.snapshot_version,
      plan_hash: result.plan_hash,
      output: result.output,
    };
  });
}
