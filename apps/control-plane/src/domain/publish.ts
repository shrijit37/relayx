/**
 * Publication service — the operational heart of Phase 6.
 *
 * ```text
 * load workflow version
 *   → gather ALL active workflows + the one being published
 *   → resolve every referenced lane record (credential_ref → authorization)
 *   → build ONE coherent WireSnapshot (global monotonic snapshot_version)
 *   → gateway /validate (compile-only, deterministic plan hash)
 *   → persist COMPILED + plan_hash
 *   → gateway /publish (atomic — replaces the whole runtime bundle)
 *   → persist PUBLISHED + publication record + ACTIVE pointer (one txn)
 * ```
 *
 * The wire bundle always carries EVERY active workflow, not just the one being
 * published: the gateway swap is all-or-nothing for the whole runtime, so a
 * single-workflow publish would silently drop every other workflow from the
 * data plane (review finding #2). Publishing B therefore preserves A.
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
 * Lane ids a workflow references: llm.lane_id, fallback providers, retry
 * target. The compiler rejects lane-less LLM nodes, so an unresolved
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

/**
 * Distinct custom extension kinds referenced by a workflow's Custom nodes.
 *
 * Walks `workflowJson.nodes` looking for `config.kind === "custom"` and
 * reading `config.ext_kind` (the extension kind identifier after the
 * serde `ext_kind` rename). Returns the set of distinct kinds so the
 * wire snapshot can carry extension metadata for observability.
 */
export function collectCustomExtensionKinds(workflowJson: Record<string, unknown>): Set<string> {
  const kinds = new Set<string>();
  const nodes = Array.isArray(workflowJson.nodes) ? workflowJson.nodes : [];
  for (const node of nodes as Array<Record<string, unknown>>) {
    const config = (node?.config ?? {}) as Record<string, unknown>;
    if (config.kind === "custom" && typeof config.ext_kind === "string" && config.ext_kind) {
      kinds.add(config.ext_kind as string);
    }
  }
  return kinds;
}

/** Resolve a lane row to the wire lane config, credentials out-of-band. */
async function toWireLane(
  lane: LaneRowWithCred,
  workflowId: string,
): Promise<WireLane | { error: string }> {
  // Fail-closed egress: a masked lane MUST ship with a proxy URL. Surface
  // this as an actionable bundle error — the gateway's compile-time
  // rejection is the backstop, but the control plane must never emit a
  // masked wired lane without a tunnel.
  if (lane.egress === "masked" && !lane.proxy_url) {
    return {
      error: `workflow '${workflowId}' lane '${lane.id}' has egress=masked but no proxy_url`,
    };
  }
  return {
    base_url: lane.base_url,
    authorization: resolveCredential(lane.credential_ref),
    egress: lane.egress,
    proxy_url: lane.proxy_url,
  };
}

/**
 * The SINGLE coherent-bundle builder. Given an explicit set of workflows +
 * a lane resolver + a global snapshot-version allocator, builds ONE
 * `WireSnapshot` where every referenced lane has its credential resolved
 * and the snapshot version is global (monotonic across restarts and
 * workflows). Used by `createPublishService` and by the rehydrate boot
 * path (review #1, #2, #9, #10) — no caller re-implements lane/bundle
 * resolution.
 */
/**
 * Build the wire bundle WITHOUT allocating a snapshot version.
 * The snapshot version is a placeholder (0) that must be patched via
 * `allocateSnapshotVersion` before sending to the gateway. This avoids
 * burning a version number when validation or lane resolution fails.
 *
 * `getExtVersions` is an optional callback that maps a set of extension
 * kind strings to a `{ kind → version }` record for snapshot metadata.
 * When omitted, all collected kinds get version 0 (the placeholder).
 */
export async function buildCoherentWireNoVersion(
  flows: BundleWorkflow[],
  getLane: (id: string) => Promise<LaneRowWithCred | null>,
  getExtVersions?: (kinds: Set<string>) => Record<string, number>,
): Promise<WireSnapshot | { error: string }> {
  const workflows: WireWorkflow[] = [];
  const allKinds = new Set<string>();

  for (const flow of flows) {
    const referenced = collectReferencedLanes(flow.workflowJson);
    const flowLanes: Record<string, WireLane> = {};
    for (const id of referenced) {
      const lane = await getLane(id);
      if (!lane) return { error: `workflow '${flow.id}' references unknown lane '${id}'` };
      const wired = await toWireLane(lane, flow.id);
      if ("error" in wired) return wired;
      flowLanes[id] = wired;
    }

    // Collect custom extension kinds from this workflow's Custom nodes.
    for (const kind of collectCustomExtensionKinds(flow.workflowJson)) {
      allKinds.add(kind);
    }

    workflows.push({ id: flow.id, workflow: flow.workflowJson, lanes: flowLanes, version: flow.version });
  }

  // Build extension metadata: kind + version for each distinct extension
  // referenced by any workflow in the bundle. The actual validator/executor
  // trait objects live in the runtime registry, not here.
  const versionMap = getExtVersions ? getExtVersions(allKinds) : {};
  const extensions = [...allKinds].map((kind) => ({
    kind,
    version: versionMap[kind] ?? 0,
  }));

  // Placeholder — patched by allocateSnapshotVersion before gateway publish.
  return { snapshot_version: 0, workflows, extensions };
}

/**
 * Allocate a real snapshot version and patch it into a wire bundle.
 * Call this ONLY when the wire has been validated and is about to be
 * published — this is the only point where a Postgres row is burned.
 */
export async function allocateSnapshotVersion(
  wire: WireSnapshot,
  nextSnapshotVersion: () => Promise<number>,
): Promise<WireSnapshot> {
  const snapshot_version = await nextSnapshotVersion();
  return { ...wire, snapshot_version };
}

export type PublishResult = {
  status: "published" | "error";
  snapshot_version?: number;
  plan_hash?: string;
  published_at?: string;
  error?: string;
};

/** A workflow that will be part of the coherent bundle. */
export type BundleWorkflow = {
  id: string;
  version: number;
  workflowJson: Record<string, unknown>;
  revision: "active" | "target";
};

export function createPublishService(deps: {
  pool: Pool;
  getLane: (id: string) => Promise<LaneRowWithCred | null>;
  /** Load every ACTIVE workflow (id + latest version) so a publish preserves
   *  the rest of the runtime. Repository seam. */
  listActiveWorkflows: (exceptWorkflowIds?: string[]) => Promise<BundleWorkflow[]>;
  /** Allocate the next global snapshot version (monotonic across restarts). */
  nextSnapshotVersion: () => Promise<number>;
  gateway: { validate(p: unknown): Promise<GatewayResult>; publish(p: unknown): Promise<GatewayResult> };
}) {
  /** Build the coherent wire bundle WITHOUT allocating a snapshot version.
   * The version is a placeholder (0) that must be patched via
   * `allocateSnapshotVersion` before sending to the gateway for publish.
   * Validation does not need a real version — the plan hash is deterministic
   * from the workflow definition.
   */
  async function buildWire(opts: {
    workflowId: string;
    workflowJson: Record<string, unknown>;
    version: number;
  }): Promise<WireSnapshot | { error: string }> {
    const others = await deps.listActiveWorkflows([opts.workflowId]);
    return buildCoherentWireNoVersion(
      [
        { id: opts.workflowId, version: opts.version, workflowJson: opts.workflowJson, revision: "target" as const },
        ...others,
      ],
      async (id) => deps.getLane(id),
    );
  }

  return {
    /** Build the coherent wire bundle (validate/compile proxy uses this). */
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
      // Version is still a placeholder (0) here — allocated only after validation
      // succeeds so failed validates don't burn snapshot version numbers.
      const validated = await deps.gateway.validate(wire);
      if (!validated.ok) {
        await recordFailure(deps.pool, workflowId, version, validated.error, "draft", "", wire.snapshot_version);
        return { status: "error", error: validated.error };
      }
      const planHash =
        validated.workflows.find((w) => w.workflow_id === workflowId)?.plan_hash ?? "";
      await recordCompiled(deps.pool, workflowId, version, planHash);

      // Allocate a real snapshot version NOW — only after validation passed.
      const publishedWire = await allocateSnapshotVersion(wire, async () => deps.nextSnapshotVersion());

      // Atomic publish — the gateway swaps snapshot + lane pools in one store.
      const published = await deps.gateway.publish(publishedWire);
      if (!published.ok) {
        await recordFailure(deps.pool, workflowId, version, published.error, "compiled", planHash, publishedWire.snapshot_version);
        return { status: "error", error: published.error };
      }

      // Post-publish metadata as ONE transaction: if control-plane state and
      // the gateway swap ever disagree, a crash here leaves ACTIVE pointer
      // consistent with the gateway (no silent rollback on next reboot).
      const publishedAt = await recordPublished(
        deps.pool,
        workflowId,
        version,
        planHash,
        published.snapshot_version,
      );
      return {
        status: "published",
        snapshot_version: published.snapshot_version,
        plan_hash: planHash,
        published_at: publishedAt,
      };
    },

    /**
     * Deactivate a workflow: republish the coherent bundle WITHOUT this
     * workflow, then remove the `workflow_active` pointer. The gateway
     // stops serving this workflow; the control-plane status reverts to
     * `draft`; the DELETE guard no longer blocks deletion.
     */
    async deactivate(opts: {
      workflowId: string;
    }): Promise<{ status: "deactivated" | "error"; error?: string }> {
      const { workflowId } = opts;

      // Build the coherent wire bundle EXCLUDING the target workflow.
      const others = await deps.listActiveWorkflows([workflowId]);
      if (others.length === 0) {
        // No other active workflows — just remove the pointer and skip
        // a gateway publish (nothing left to serve).
        await removeActivePointer(deps.pool, workflowId);
        return { status: "deactivated" };
      }

      const wire = await buildCoherentWireNoVersion(
        others.map((w) => ({ ...w, revision: "active" as const })),
        async (id) => deps.getLane(id),
      );
      if ("error" in wire) {
        return { status: "error", error: wire.error };
      }

      const publishedWire = await allocateSnapshotVersion(wire, async () => deps.nextSnapshotVersion());
      const published = await deps.gateway.publish(publishedWire);
      if (!published.ok) {
        return { status: "error", error: published.error };
      }

      // Remove the active pointer and revert status to draft.
      await removeActivePointer(deps.pool, workflowId);
      return { status: "deactivated" };
    },
  };
}

// ── Persistence helpers (raw SQL through the pool; repositories stay CRUD-only) ──
// NOTE: the ACTIVE-pointer write here is the ONLY copy — `workflows.upsertActive`
// in repositories.ts is dead and was removed to avoid a divergent duplicate.

export async function recordCompiled(
  pool: Pool,
  workflowId: string,
  version: number,
  planHash: string,
): Promise<void> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(
      "UPDATE workflow_versions SET status = 'compiled', plan_hash = $3 WHERE workflow_id = $1 AND version = $2",
      [workflowId, version, planHash],
    );
    await client.query(
      "UPDATE workflows SET status = 'compiled', updated_at = now() WHERE id = $1",
      [workflowId],
    );
    await client.query("COMMIT");
  } catch (e) {
    await client.query("ROLLBACK");
    throw e;
  } finally {
    client.release();
  }
}

export async function recordPublished(
  pool: Pool,
  workflowId: string,
  version: number,
  planHash: string,
  snapshotVersion: number,
): Promise<string> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(
      "UPDATE workflow_versions SET status = 'active' WHERE workflow_id = $1 AND version = $2",
      [workflowId, version],
    );
    await client.query(
      "UPDATE workflows SET status = 'active', updated_at = now() WHERE id = $1",
      [workflowId],
    );
    const pub = await client.query<{ published_at: string }>(
      "INSERT INTO publications (id, workflow_id, workflow_version, plan_hash, snapshot_version, status) VALUES ($1,$2,$3,$4,$5,'succeeded') RETURNING published_at",
      [crypto.randomUUID(), workflowId, version, planHash, snapshotVersion],
    );
    await client.query(
      `INSERT INTO workflow_active (workflow_id, workflow_version, plan_hash, snapshot_version, updated_at)
       VALUES ($1,$2,$3,$4,now())
       ON CONFLICT (workflow_id) DO UPDATE
       SET workflow_version = $2, plan_hash = $3, snapshot_version = $4, updated_at = now()`,
      [workflowId, version, planHash, snapshotVersion],
    );
    await client.query("UPDATE runtime_meta SET snapshot_version = $1, updated_at = now() WHERE id = 'global'", [
      snapshotVersion,
    ]);
    await client.query("COMMIT");
    return pub.rows[0]?.published_at ?? new Date().toISOString();
  } catch (e) {
    await client.query("ROLLBACK");
    throw e;
  } finally {
    client.release();
  }
}

async function recordFailure(
  pool: Pool,
  workflowId: string,
  version: number,
  error: string,
  revertTo: "draft" | "compiled" = "draft",
  planHash: string = "",
  snapshotVersion: number = 0,
): Promise<void> {
  // Never fabricate 0/empty for a failure that DID reach a valid plan:
  // callers pass the real plan_hash + snapshot_version when known so a
  // downstream monotonicity consumer sees the actual counter, not a
  // regression to 0 (review).
  await pool.query(
    "INSERT INTO publications (id, workflow_id, workflow_version, plan_hash, snapshot_version, status, error) VALUES ($1,$2,$3,$4,$5,'failed',$6)",
    [crypto.randomUUID(), workflowId, version, planHash, snapshotVersion, error],
  );
  await pool.query("UPDATE workflow_versions SET status = $3 WHERE workflow_id = $1 AND version = $2", [workflowId, version, revertTo]);
}

/** Remove the active pointer and revert workflow status to draft. */
async function removeActivePointer(pool: Pool, workflowId: string): Promise<void> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    await client.query("DELETE FROM workflow_active WHERE workflow_id = $1", [workflowId]);
    await client.query(
      "UPDATE workflows SET status = 'draft', updated_at = now() WHERE id = $1",
      [workflowId],
    );
    await client.query("COMMIT");
  } catch (e) {
    await client.query("ROLLBACK");
    throw e;
  } finally {
    client.release();
  }
}

/** Allocate the next global snapshot version (monotonic across restarts).
 *  Postgres BIGINT arrives as a string via node-pg. The value is safe as a
 *  JS Number as long as it stays below 2^53 — at that point the deploy is
 *  already long-lived enough to warrant a BigInt migration; here we fail
 *  loudly instead of silently wrapping (review D3). */
export async function nextSnapshotVersion(pool: Pool): Promise<number> {
  const { rows } = await pool.query<{ snapshot_version: string }>(
    "UPDATE runtime_meta SET snapshot_version = snapshot_version + 1, updated_at = now() WHERE id = 'global' RETURNING snapshot_version",
  );
  const raw = rows[0]?.snapshot_version;
  if (!raw) return 1;
  const v = Number(raw);
  if (!Number.isSafeInteger(v)) {
    throw new Error(
      `runtime_meta snapshot_version ${raw} exceeds Number.MAX_SAFE_INTEGER — time for BigInt migration`,
    );
  }
  return v;
}

/** All ACTIVE workflows (id + version + JSON), optionally excluding some. */
export async function listActiveWorkflows(
  pool: Pool,
  exceptWorkflowIds: string[] = [],
): Promise<BundleWorkflow[]> {
  const exclusions = exceptWorkflowIds.length > 0 ? "WHERE wa.workflow_id != ANY($1)" : "";
  const params: unknown[] = exceptWorkflowIds.length > 0 ? [exceptWorkflowIds] : [];
  const { rows } = await pool.query<{
    workflow_id: string;
    workflow_version: number;
    workflow_json: Record<string, unknown>;
  }>(
    `SELECT wa.workflow_id, wa.workflow_version, wv.workflow_json
     FROM workflow_active wa
     JOIN workflow_versions wv
       ON wv.workflow_id = wa.workflow_id AND wv.version = wa.workflow_version
     ${exclusions}`,
    params,
  );
  return rows.map((r) => ({
    id: r.workflow_id,
    version: r.workflow_version,
    workflowJson: r.workflow_json,
    revision: "active" as const,
  }));
}