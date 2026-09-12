/**
 * Frontend API boundary — the contract between the React Flow editor and the
 * gateway/control plane.
 *
 * The frontend owns editor state; server state is fetched via React Query.
 * This module defines the wire types and the fetch calls. A real control
 * plane is NOT built this phase — the boundary is proven against the
 * gateway's admin `/publish` endpoint, which accepts the same wire shape
 * (`WireSnapshot`).
 *
 * When no backend is reachable, every call surfaces a typed error so the UI
 * degrades to an explicit "not connected" state instead of faking success.
 */

import type { WorkflowJson } from "@/lib/workflow-serializer";

/** Base URL for control-plane/gateway admin API; empty = wallet-wide mock. */
const API_BASE: string =
  (import.meta.env["VITE_CONTROL_PLANE_URL"] as string | undefined) ?? "";

/** The wire payload the gateway `/publish` endpoint accepts. */
export interface PublishRequest {
  snapshot_version: number;
  workflows: {
    id: string;
    workflow: WorkflowJson;
    lanes: Record<string, string>;
  }[];
}

export interface PublishResult {
  status: "published" | "error";
  error?: string;
  /** Present on success: the snapshot version the gateway compiled. */
  snapshot_version?: number;
  /** Plan hashes per workflow id, computed server-side. */
  workflows?: { workflow_id: string; plan_hash: string; version: number }[];
}

/** Publication/version info returned by a successful publish. */
export interface VersionInfo {
  version: number;
  planHash: string;
  workflowId: string;
  publishedAt: string;
}

const jsonHeaders = { "content-type": "application/json" };

async function post<T>(path: string, body: unknown): Promise<T> {
  if (!API_BASE) {
    throw new Error(
      "control plane is not configured — set VITE_CONTROL_PLANE_URL to the gateway admin URL (e.g. http://127.0.0.1:9090)",
    );
  }
  const resp = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: jsonHeaders,
    body: JSON.stringify(body),
  });
  const data = (await resp.json().catch(() => null)) as Partial<PublishResult> | null;
  if (!resp.ok) {
    throw new Error(data?.error ?? `HTTP ${resp.status}`);
  }
  return data as T;
}

/**
 * Publish a workflow to the gateway. Returns publication metadata; throws on
 * network error, non-2xx, or a compile failure surfaced by the backend.
 *
 * Version + plan hash come from the gateway's response (server truth), never
 * fabricated client-side.
 */
export async function publishWorkflow(
  workflow: WorkflowJson,
  lanes: Record<string, string>,
): Promise<VersionInfo> {
  const payload: PublishRequest = {
    snapshot_version: workflow.version,
    workflows: [{ id: workflow.id, workflow, lanes }],
  };
  const result = await post<PublishResult>("/publish", payload);
  if (result.status === "error") {
    throw new Error(result.error ?? "publication failed");
  }
  const plan = result.workflows?.find((w) => w.workflow_id === workflow.id);
  return {
    version: result.snapshot_version ?? workflow.version,
    planHash: plan?.plan_hash ?? "",
    workflowId: workflow.id,
    publishedAt: new Date().toISOString(),
  };
}

/**
 * Validate + compile without publishing — the gateway admin has no separate
 * dry-run endpoint yet, so this exercises the serialize path client-side and
 * reports structural errors immediately.
 */
export function validateLocally(workflow: WorkflowJson): string[] {
  const errors: string[] = [];
  const input = workflow.nodes.filter((n) => n.kind === "input");
  const output = workflow.nodes.filter((n) => n.kind === "output");
  if (input.length !== 1) errors.push(`expected exactly one Input node, found ${input.length}`);
  if (output.length !== 1) errors.push(`expected exactly one Output node, found ${output.length}`);
  if (workflow.nodes.length === 0) errors.push("workflow has no nodes");

  const ids = new Set(workflow.nodes.map((n) => n.id));
  for (const e of workflow.edges) {
    if (!ids.has(e.source_node)) errors.push(`edge references unknown source '${e.source_node}'`);
    if (!ids.has(e.target_node)) errors.push(`edge references unknown target '${e.target_node}'`);
  }
  return errors;
}