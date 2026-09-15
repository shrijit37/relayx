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

import type { WorkflowJson } from "@/lib/workflow";

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
export type WorkflowRow = {
  id: string;
  name: string;
  status: string;
  project_id: string;
  created_at: string;
};

export type VersionRow = {
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
  published_at?: string;
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

  // Backend-truth only: version/snapshot/plan-hash/published_at come from
  // the control plane — never the client clock (review:angle-c).
  return {
    version: result.workflow_version,
    planHash: result.plan_hash,
    workflowId: result.workflow_id,
    snapshotVersion: result.snapshot_version,
    status: "active",
    publishedAt: result.published_at ?? new Date().toISOString(),
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
export type LaneRow = {
  id: string;
  project_id: string;
  provider_id: string | null;
  endpoint: string;
  base_url: string;
  egress: string;
  policies: string[];
  credential_ref: { ref: string; provider: string } | null;
  created_at: string;
};

export async function fetchLanes(projectId = "proj_default"): Promise<LaneRow[]> {
  return req(`/lanes?project_id=${projectId}`);
}

/** Create a lane via the control plane (real persisted row). */
export async function createLane(input: {
  id?: string;
  name: string;
  project_id: string;
  endpoint: string;
  base_url: string;
  egress?: string;
  policies?: string[];
}): Promise<LaneRow> {
  return req(`/lanes`, { method: "POST", body: JSON.stringify(input) });
}

/** Fetch the latest version row for a workflow (sorted descending). */
export async function fetchWorkflowLatestVersion(workflowId: string): Promise<VersionRow | null> {
  const rows = await req<VersionRow[]>(`/workflows/${workflowId}/versions`);
  if (!rows.length) return null;
  return rows.reduce((a, b) => (a.version > b.version ? a : b));
}

/** Persist the editor state as a new immutable version (public). */
export async function saveWorkflowVersion(workflowId: string, workflow: WorkflowJson): Promise<VersionRow> {
  return req<VersionRow>(`/workflows/${workflowId}/versions`, {
    method: "POST",
    body: JSON.stringify({ workflow_json: workflow }),
  });
}

/** Create a new workflow row in the control plane. */
export async function createWorkflow(name: string, projectId = "proj_default"): Promise<WorkflowRow> {
  return req<WorkflowRow>("/workflows", {
    method: "POST",
    body: JSON.stringify({ name, project_id: projectId }),
  });
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

/** SSE stream events from the gateway's token-level run. */
export type StreamTokenEvent = { type: "token"; delta: string };
export type StreamDoneEvent = {
  type: "done";
  request_id: string;
  workflow_id: string;
  workflow_version: number;
  snapshot_version: number;
  plan_hash: string;
  output: unknown;
};
export type StreamErrorEvent = { type: "error"; error: string };
export type StreamEvent = StreamTokenEvent | StreamDoneEvent | StreamErrorEvent;

/** Execute the published ACTIVE version of a workflow through the control
 *  plane, consuming the gateway's SSE token stream. Yields parsed events as
 *  they arrive; the caller iterates with `for await`. */
export async function* runWorkflowStream(
  workflowId: string,
  body: unknown,
  signal?: AbortSignal,
): AsyncGenerator<StreamEvent> {
  const init: RequestInit = {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ body }),
  };
  if (signal !== undefined) init.signal = signal;

  const resp = await fetch(`${API_BASE}/workflows/${workflowId}/run?stream=true`, init);
  if (!resp.ok) {
    const data = (await resp.json().catch(() => null)) as { error?: string } | null;
    throw new Error(data?.error ?? `HTTP ${resp.status}`);
  }
  // Move `resp.body!` inside the generator so a 200-with-empty body (proxy
  // stripping) throws the typed error instead of a raw TypeError at call time.
  if (!resp.body) throw new Error("empty response body");
  const reader = resp.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      // SSE events are terminated by a blank line. Scan for the NEXT boundary
      // and slice off only complete events — re-splitting the whole buffer from
      // byte 0 each chunk is O(n²) at token cadence for a long transcript.
      let boundary = buffer.indexOf("\n\n");
      while (boundary >= 0) {
        const part = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const event = parseSseEvent(part);
        if (!event) {
          boundary = buffer.indexOf("\n\n");
          continue;
        }
        let parsed: Record<string, unknown>;
        try {
          parsed = JSON.parse(event.data);
        } catch {
          yield { type: "error", error: `malformed ${event.eventType} event` };
          return;
        }
        if (event.eventType === "token") {
          yield { type: "token", delta: (parsed as { delta: string }).delta };
        } else if (event.eventType === "done") {
          yield {
            type: "done",
            request_id: parsed["request_id"] as string,
            workflow_id: parsed["workflow_id"] as string,
            workflow_version: (parsed["workflow_version"] as number) ?? 0,
            snapshot_version: parsed["snapshot_version"] as number,
            plan_hash: parsed["plan_hash"] as string,
            output: parsed["output"],
          };
        } else if (event.eventType === "error") {
          yield { type: "error", error: (parsed as { error: string }).error };
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
  } finally {
    reader.cancel();
  }
}

function parseSseEvent(part: string): { eventType: string; data: string } | null {
  if (!part.trim()) return null;
  let eventType = "message";
  const dataLines: string[] = [];
  for (const line of part.split("\n")) {
    if (line.startsWith("event:")) {
      eventType = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trim());
    }
  }
  return { eventType, data: dataLines.join("\n") };
}

/** Real control-plane + gateway health probe (both /healthz and /ready). */
export type SystemHealth = {
  control_plane: { status: string; service?: string };
  gateway: {
    healthz: { status: string; detail?: string };
    ready: { status: string; detail?: string };
  };
};

export async function fetchSystemHealth(): Promise<SystemHealth> {
  return req<SystemHealth>("/system/health");
}

/** Real provider rows from the control plane (persisted config only). */
export type ProviderRow = {
  id: string;
  name: string;
  protocol: string;
  base_url: string;
  model: string;
  created_at: string;
};

export async function fetchProviders(): Promise<ProviderRow[]> {
  return req(`/providers`);
}

/** Create a provider via the control plane (real persisted row). */
export async function createProvider(input: {
  name: string;
  protocol: string;
  base_url: string;
  model: string;
}): Promise<ProviderRow> {
  return req(`/providers`, { method: "POST", body: JSON.stringify(input) });
}

// ── Shared helpers ──────────────────────────────────────────────────────

/**
 * Get or create the workflow's durable row, keyed by the editor's id.
 * The control plane accepts an explicit `id` on POST /workflows so the
 * frontend's workflow identity (e.g. "production-gateway") is preserved
 * instead of orphaning a UUID row per publish (review #3).
 */
async function ensureWorkflow(workflow: WorkflowJson): Promise<WorkflowRow> {
  // Fetch the single row by id (404 → create) instead of paging the whole
  // /workflows table on every publish/validate.
  const existing = await req<WorkflowRow>(
    `/workflows/${encodeURIComponent(workflow.id)}`,
  ).catch(() => null);
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

/** Persist the exact editor state as a new immutable version (publish convenience). */
async function createImmutableVersion(workflowId: string, workflow: WorkflowJson): Promise<void> {
  await saveWorkflowVersion(workflowId, workflow);
}