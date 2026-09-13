/**
 * Control-plane route wiring.
 *
 * API → service → repository → PostgreSQL. Routes are thin; the publish
 * service owns the pipeline. The only external caller is the frontend; the
 * only external receiver is the gateway admin API.
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
import { createWorkflowSchema, laneDtoSchema, providerDtoSchema, publishSchema } from "./schemas";
import type { CredentialRef } from "../secrets";

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

  app.get("/healthz", async () => ({ status: "ok" }));

  // Real system health for the frontend: probes the control plane itself and
  // the gateway's live admin endpoints (/healthz, /ready). Never fabricated —
  // each value is a real HTTP probe result.
  app.get("/system/health", async () => {
    const probe = async (path: string): Promise<{ status: string; detail?: string }> => {
      try {
        const r = await fetch(`${gateway.baseUrl}${path}`, { signal: AbortSignal.timeout(2000) });
        if (!r.ok) return { status: "degraded", detail: `HTTP ${r.status}` };
        const j = (await r.json().catch(() => null)) as { status?: string } | null;
        return { status: j?.status ?? "ok" };
      } catch (e) {
        return { status: "unreachable", detail: String(e) };
      }
    };
    return {
      control_plane: { status: "ok", service: "relayx-control-plane" },
      gateway: {
        healthz: await probe("/healthz"),
        ready: await probe("/ready"),
      },
    };
  });

  // ── Workflows ────────────────────────────────────────────────────────
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
    if (!(await repo.workflows.get(pool, id))) return reply.code(404).send({ error: "workflow not found" });
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
  const validateCompile = async (req: import("fastify").FastifyRequest, reply: import("fastify").FastifyReply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const body = (req.body ?? {}) as { workflow_json?: Record<string, unknown>; version?: number };
    const workflowJson = body.workflow_json ?? {};
    // Resolve the version whose stored JSON MATCHES the submitted content, so
    // the recorded plan_hash always belongs to the version it is stored under
    // (review #5). No match → refuse rather than hash mismatched content.
    const version = body.version ?? (await versionForJson(pool, id, workflowJson));
    if (version === null) {
      return reply.code(400).send({ error: "no stored version matches the submitted workflow_json (create a version first)" });
    }
    const wire = await publish.buildWire({ workflowId: id, workflowJson, version });
    if ("error" in wire) return reply.code(400).send({ error: wire.error });
    const result = await gateway.validate(wire);
    if (!result.ok) return reply.code(400).send({ error: result.error });
    const planHash = result.workflows.find((w) => w.workflow_id === id)?.plan_hash ?? null;
    await repo.workflows.updateVersionStatus(pool, id, version, "compiled", planHash);
    // A validate/compile must NOT degrade an active parent (review #9).
    if (wf.status !== "active") await repo.workflows.setStatus(pool, id, "compiled");
    return { status: "compiled", snapshot_version: result.snapshot_version, plan_hash: planHash, workflows: result.workflows };
  };

  app.post("/workflows/:id/validate", validateCompile);
  app.post("/workflows/:id/compile", validateCompile);

  app.post("/workflows/:id/publish", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const parsed = publishSchema.safeParse(req.body ?? {});
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const version = parsed.data.version ?? (await versionForJson(pool, id, parsed.data.workflow_json));
    if (version === null) {
      return reply.code(400).send({ error: "no stored version matches the submitted workflow_json (create a version first)" });
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
    if (!previous) return reply.code(409).send({ error: "no prior validated version to republish" });

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
      return reply.code(409).send({ error: "Workflow must be published before it can be run." });
    }

    const body = ((req.body ?? {}) as { body?: unknown }).body ?? null;
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

  // ── Providers ────────────────────────────────────────────────────────
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

  // ── Lanes ────────────────────────────────────────────────────────────
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
      policies: string[];
      credential_ref: CredentialRef | null;
    } = {
      project_id: parsed.data.project_id,
      provider_id: parsed.data.provider_id ?? null,
      endpoint: parsed.data.endpoint,
      base_url: parsed.data.base_url,
      egress: parsed.data.egress,
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
      policies?: string[];
      credential_ref?: CredentialRef | null;
    } = {};
    if (data.provider_id !== undefined) patch.provider_id = data.provider_id;
    if (data.endpoint !== undefined) patch.endpoint = data.endpoint;
    if (data.base_url !== undefined) patch.base_url = data.base_url;
    if (data.egress !== undefined) patch.egress = data.egress;
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

  return app;
}

// ── Helpers ────────────────────────────────────────────────────────────

function projectIdOf(req: import("fastify").FastifyRequest, fallback: string): string {
  return ((req.query as Record<string, string> | undefined)?.project_id ?? fallback);
}

/** The version whose stored graph matches `json` on the compilable parts
 *  (nodes + edges). Key order is canonicalized (recursively sorted) so a
 *  semantically-identical JSON matches regardless of insertion order; the
 *  serde `kind` tag inside configs and top-level identity fields are
 *  excluded (serialization artifacts, not editor content). Review #5. */
async function versionForJson(
  pool: Pool,
  workflowId: string,
  json: Record<string, unknown>,
): Promise<number | null> {
  const versions = await repo.workflows.listVersions(pool, workflowId);
  if (versions.length === 0) return null;
  const graphOf = (wj: Record<string, unknown>): string =>
    JSON.stringify(sortKeys([
      stripKind(wj.nodes),
      stripKind(wj.edges),
    ]));
  const needle = graphOf(json);
  for (const v of versions) {
    if (graphOf(v.workflow_json as Record<string, unknown>) === needle) return v.version;
  }
  return null;
}

/** Recursively sort object keys (stable across JSON representations). */
function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(value as Record<string, unknown>).sort()) {
      out[k] = sortKeys((value as Record<string, unknown>)[k]);
    }
    return out;
  }
  return value;
}

/** Strip the `kind` tag from config objects only.
 *
 * The graph layout is `node = {id, kind, config, inputs, outputs}` where
 * `config = {kind, …}`. The node-level `kind` is the semantic discriminator
 * ("llm" vs "router" — must be preserved); the config-level `kind` is a
 * serde artifact. We strip only the `kind` inside `config` (and recurse
 * into children) — never the node's own `kind`. Review D7. */
function stripKind(nodesOrValue: unknown): unknown {
  const isNodeList = Array.isArray(nodesOrValue) && nodesOrValue.every((n) => isNode(n));
  if (isNodeList) return nodesOrValue.map(stripNode);
  return nodesOrValue;
}

function isNode(v: unknown): boolean {
  return (
    v !== null &&
    typeof v === "object" &&
    typeof (v as Record<string, unknown>).kind === "string" &&
    typeof (v as Record<string, unknown>).id !== "undefined"
  );
}

function stripNode(node: unknown): unknown {
  const n = node as { id?: unknown; kind?: unknown; config?: unknown; inputs?: unknown; outputs?: unknown };
  const config = n.config === undefined ? n.config : stripConfig(n.config);
  return { ...n, config };
}

/** Drop `kind` from this config object, recursively (children keep kinds). */
function stripConfig(value: unknown): unknown {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(stripConfig);
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (k === "kind") continue;
    out[k] = k === "config" ? stripConfig(v) : v;
  }
  return out;
}