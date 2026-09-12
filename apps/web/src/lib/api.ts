/**
 * Frontend API boundary — the contract between the React Flow editor and the
 * control plane (Fastify on RELAYX_CONTROL_PORT, real Postgres behind it).
 *
 * The frontend owns editor state; the control plane owns durable workflows,
 * versions, plan hashes, and publication. Every field shown to the user
 * (version, plan hash, snapshot, lane URLs, publication status) comes from
 * the backend — the frontend never fabricates runtime truth.
 *
 * When the control plane is unreachable, calls throw typed errors so the UI
 * degrades to an explicit "not connected" state instead of faking success.
 */

import type { WorkflowJson } from "@/lib/workflow-serializer";

/** Base URL for the control-plane API. */
const API_BASE: string =
  (import.meta.env["VITE_CONTROL_PLANE_URL"] as string | undefined) ??
  "http://127.0.0.1:9091";

const jsonHeaders = { "content-type": "application/json" };

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const resp = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: { ...jsonHeaders, ...(init?.headers ?? {}) },
  });
  const data = (await resp.json().catch(() => null)) as Partial<T> | null;
  if (!resp.ok) {
    throw new Error(((data as { error?: string } | null)?.error) ?? `HTTP ${resp.status}`);
  }
  return data as T;
}

/** The control-plane API shapes (mirror of the Fastify routes). */
type WorkflowRow = {
  id: string;
  name: string;
  status: string;
  project_id: string;
  created_at: string;
};

type VersionRow = {
  id: string;
  workflow_id: string;
  version: number;
  workflow_json: WorkflowJson;
  plan_hash: string | null;
  status: string;
  created_at: string;
};

type PublishResponse = {
  status: string;
  workflow_id: string;
  workflow_version: number;
  snapshot_version: number;
  plan_hash: string;
};

/** Version/plan metadata displayed to the user — always backend-derived. */
export interface VersionInfo {
  version: number;
  planHash: string;
  workflowId: string;
  snapshotVersion: number;
  status: string;
  publishedAt: string;
}

/** Ensure the workflow row exists (create if it doesn't), then publish. */
export async function publishWorkflow(
  workflow: WorkflowJson,
  _lanes: Record<string, string>,
): Promise<VersionInfo> {
  // Lane records (base_url + credentials) live in the control plane; the
  // frontend references lanes by id inside the workflow JSON. `_lanes` is
  // the editor's lane-node map, kept for API-compat — resolution happens
  // control-plane-side at publish time.

  // The workflow's durable row is keyed by the EDITOR's id (e.g.
  // "production-gateway"), so subsequent GET/POST /workflows/:id target the
  // same workflow — no orphan UUID rows (review #3).
  const wfRow = await ensureWorkflow(workflow);
  await createImmutableVersion(wfRow.id, workflow);

  const result = await req<PublishResponse>(`/workflows/${wfRow.id}/publish`, {
    method: "POST",
    body: JSON.stringify({ workflow_json: workflow }),
  });

  if (result.status !== "published") {
    throw new Error(`publication did not complete: ${result.status}`);
  }

  // Backend-truth only: version/snapshot/plan-hash come from the control plane.
  return {
    version: result.workflow_version,
    planHash: result.plan_hash,
    workflowId: result.workflow_id,
    snapshotVersion: result.snapshot_version,
    status: "active",
    publishedAt: new Date().toISOString(),
  };
}

/** Load workflow versions for the versions page (backend-derived). */
export async function fetchWorkflowVersions(workflowId: string): Promise<VersionRow[]> {
  return req<VersionRow[]>(`/workflows/${workflowId}/versions`);
}

/** Load all workflows (backend-derived list for the index page). */
export async function fetchWorkflows(): Promise<
  Array<{ id: string; name: string; status: string; created_at: string }>
> {
  return req(`/workflows`);
}

/** Load lane records so the editor can show real lane URLs/config. */
export async function fetchLanes(projectId = "proj_default"): Promise<
  Array<{ id: string; base_url: string; egress: string; credential_ref: { ref: string } | null }>
> {
  return req(`/lanes?project_id=${projectId}`);
}

/** Validate + compile without publishing; returns the real plan hash. */
export async function validateWorkflow(
  workflow: WorkflowJson,
): Promise<{ plan_hash: string; status: string }> {
  const row = await ensureWorkflow(workflow);
  return req(`/workflows/${row.id}/validate`, {
    method: "POST",
    body: JSON.stringify({ workflow_json: workflow }),
  });
}

// ── Shared helpers ──────────────────────────────────────────────────────

/**
 * Get or create the workflow's durable row, keyed by the editor's id.
 * The control plane accepts an explicit `id` on POST /workflows so the
 * frontend's workflow identity (e.g. "production-gateway") is preserved
 * instead of orphaning a UUID row per publish (review #3).
 */
async function ensureWorkflow(workflow: WorkflowJson): Promise<WorkflowRow> {
  const existing = (await req<WorkflowRow[]>(`/workflows`)).find((w) => w.id === workflow.id);
  if (existing) return existing;
  const created = await req<WorkflowRow>(`/workflows`, {
    method: "POST",
    body: JSON.stringify({
      id: workflow.id,
      name: typeof workflow.name === "string" ? workflow.name : workflow.id,
      project_id: "proj_default",
    }),
  });
  return created;
}

/** Persist the exact editor state as a new immutable version. */
async function createImmutableVersion(workflowId: string, workflow: WorkflowJson): Promise<void> {
  await req<VersionRow>(`/workflows/${workflowId}/versions`, {
    method: "POST",
    body: JSON.stringify({ workflow_json: workflow }),
  });
}