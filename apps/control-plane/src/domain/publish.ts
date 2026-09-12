/**
 * Publication service — the operational heart of Phase 6.
 *
 * ```text
 * load workflow version
 *   → resolve lane records referenced by the workflow
 *   → resolve credential_ref → authorization (env: in-process, minimal)
 *   → build WireSnapshot (lanes have base_url + authorization, workflow JSON
 *     itself never carries credentials)
 *   → gateway /validate (compile-only, deterministic plan hash)
 *   → persist COMPILED + plan_hash
 *   → gateway /publish (atomic)
 *   → persist PUBLISHED + publication record + ACTIVE pointer
 * ```
 *
 * Any failure leaves the previously active runtime untouched — the gateway
 * swaps snapshot + lane pools only on a successful compile of the whole
 * bundle.
 */

import type { Pool } from "pg";
import type { GatewayResult } from "../gateway/client";
import type { WireSnapshot, WireWorkflow, WireLane } from "./wire";
import { resolveCredential, type CredentialRef } from "../secrets";
import * as repo from "../db/repositories";

type LaneRow = repo.LaneRow & { credential_ref: CredentialRef | null };

/** Lane (and credential) row type used by the publish service. */
export type LaneRowWithCred = LaneRow;

/**
 * Build the wire lane map for a workflow — only lanes the workflow actually
 * references (llm.lane_id, fallback providers, retry target) survive into
 * the publish. The compiler rejects lane-less LLM nodes, so an unresolved
 * referenced lane is a hard failure, never a silent "default".
 */
export function collectReferencedLanes(workflowJson: Record<string, unknown>): Set<string> {
  const ids = new Set<string>();
  const nodes = Array.isArray(workflowJson.nodes) ? workflowJson.nodes : [];
  for (const node of nodes as Array<Record<string, unknown>>) {
    const config = (node?.config ?? {}) as Record<string, unknown>;
    if (typeof config.lane_id === "string") ids.add(config.lane_id as string);
    if (Array.isArray(config.providers)) {
      for (const p of config.providers as Array<Record<string, unknown>>) {
        if (typeof p.lane_id === "string") ids.add(p.lane_id as string);
      }
    }
    const target = config.target as Record<string, unknown> | undefined;
    if (target && typeof target.lane_id === "string") ids.add(target.lane_id as string);
  }
  return ids;
}

/** Resolve a lane row to the wire lane config, credentials out-of-band. */
async function toWireLane(lane: LaneRowWithCred): Promise<WireLane> {
  return {
    base_url: lane.base_url,
    authorization: resolveCredential(lane.credential_ref),
  };
}

export type PublishResult = {
  status: "published" | "error";
  snapshot_version?: number;
  plan_hash?: string;
  error?: string;
};

export function createPublishService(deps: {
  pool: Pool;
  getLane: (id: string) => Promise<LaneRowWithCred | null>;
  gateway: { validate(p: unknown): Promise<GatewayResult>; publish(p: unknown): Promise<GatewayResult> };
}) {
  /** Build the wire bundle for one workflow: referenced lanes only, with
   *  credentials resolved out-of-band. This is the SINGLE lane-resolution
   *  path shared by /validate, /compile and /publish. */
  async function buildWire(opts: {
    workflowId: string;
    workflowJson: Record<string, unknown>;
    version: number;
  }): Promise<WireSnapshot | { error: string }> {
    const { workflowId, workflowJson, version } = opts;
    const lanes: Record<string, WireLane> = {};
    for (const id of collectReferencedLanes(workflowJson)) {
      const lane = await deps.getLane(id);
      if (!lane) return { error: `workflow references unknown lane '${id}'` };
      lanes[id] = await toWireLane(lane);
    }
    return {
      snapshot_version: version,
      workflows: [
        {
          id: workflowId,
          workflow: workflowJson,
          lanes,
        } satisfies WireWorkflow,
      ],
    };
  }

  return {
    /** Build the wire bundle (validate/compile proxy uses this). */
    buildWire,

    /**
     * Full publish pipeline for one workflow version.
     */
    async publish(opts: {
      workflowId: string;
      version: number;
      workflowJson: Record<string, unknown>;
    }): Promise<PublishResult> {
      const { workflowId, version, workflowJson } = opts;

      const wire = await buildWire({ workflowId, workflowJson, version });
      if ("error" in wire) {
        await recordFailure(deps.pool, workflowId, version, wire.error, "draft");
        return { status: "error", error: wire.error };
      }

      // Compile-only dry-run on the gateway: deterministic plan hash + validation.
      const validated = await deps.gateway.validate(wire);
      if (!validated.ok) {
        await recordFailure(deps.pool, workflowId, version, validated.error, "draft");
        return { status: "error", error: validated.error };
      }
      const planHash =
        validated.workflows.find((w) => w.workflow_id === workflowId)?.plan_hash ?? "";
      await recordCompiled(deps.pool, workflowId, version, planHash);

      // Atomic publish — the gateway swaps snapshot + lane pools in one store.
      const published = await deps.gateway.publish(wire);
      if (!published.ok) {
        await recordFailure(deps.pool, workflowId, version, published.error, "compiled");
        return { status: "error", error: published.error };
      }

      await recordPublished(deps.pool, workflowId, version, planHash, published.snapshot_version);
      return {
        status: "published",
        snapshot_version: published.snapshot_version,
        plan_hash: planHash,
      };
    },
  };
}

// ── Persistence helpers (raw SQL through the pool; repositories stay CRUD-only) ──

async function recordCompiled(
  pool: Pool,
  workflowId: string,
  version: number,
  planHash: string,
): Promise<void> {
  await pool.query(
    "UPDATE workflow_versions SET status = 'compiled', plan_hash = $3 WHERE workflow_id = $1 AND version = $2",
    [workflowId, version, planHash],
  );
  await pool.query("UPDATE workflows SET status = 'compiled', updated_at = now() WHERE id = $1", [workflowId]);
}

async function recordPublished(
  pool: Pool,
  workflowId: string,
  version: number,
  planHash: string,
  snapshotVersion: number,
): Promise<void> {
  await pool.query("UPDATE workflow_versions SET status = 'active' WHERE workflow_id = $1 AND version = $2", [workflowId, version]);
  await pool.query("UPDATE workflows SET status = 'active', updated_at = now() WHERE id = $1", [workflowId]);
  await pool.query(
    "INSERT INTO publications (id, workflow_id, workflow_version, plan_hash, snapshot_version, status) VALUES ($1,$2,$3,$4,$5,'succeeded')",
    [crypto.randomUUID(), workflowId, version, planHash, snapshotVersion],
  );
  await pool.query(
    `INSERT INTO workflow_active (workflow_id, workflow_version, plan_hash, snapshot_version, updated_at)
     VALUES ($1,$2,$3,$4,now())
     ON CONFLICT (workflow_id) DO UPDATE
     SET workflow_version = $2, plan_hash = $3, snapshot_version = $4, updated_at = now()`,
    [workflowId, version, planHash, snapshotVersion],
  );
}

async function recordFailure(
  pool: Pool,
  workflowId: string,
  version: number,
  error: string,
  revertTo: "draft" | "compiled" = "draft",
): Promise<void> {
  await pool.query(
    "INSERT INTO publications (id, workflow_id, workflow_version, plan_hash, snapshot_version, status, error) VALUES ($1,$2,$3,'',0,'failed',$4)",
    [crypto.randomUUID(), workflowId, version, error],
  );
  await pool.query("UPDATE workflow_versions SET status = $3 WHERE workflow_id = $1 AND version = $2", [workflowId, version, revertTo]);
}