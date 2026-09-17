/**
 * React Query hooks for the control-plane API boundary.
 *
 * React Query owns server state; the editor keeps local canvas state. Hooks
 * degrade to an explicit error when the control plane is unreachable (no
 * silent fake-success path). Publication metadata shown to the user always
 * comes from the backend.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  compileWorkflow,
  createLane,
  createProvider,
  deactivateWorkflow,
  deleteLane,
  deleteProvider,
  deleteWorkflow,
  fetchCatalogModels,
  fetchCatalogProviders,
  fetchCatalogStatus,
  fetchLanes,
  fetchProviders,
  fetchRun,
  fetchRuns,
  fetchSystemHealth,
  fetchWorkflow,
  fetchWorkflowLatestVersion,
  fetchWorkflowVersions,
  fetchWorkflows,
  publishWorkflow,
  renameWorkflow,
  rollbackWorkflow,
  runWorkflow,
  saveWorkflowVersion,
  updateLane,
  updateProvider,
  validateWorkflow,
  type CatalogModel,
  type CatalogProvider,
  type CatalogStatus,
  type ProviderRow,
  type RunRow,
  type VersionInfo,
  type VersionRow,
  type WorkflowRow,
} from "@/lib/api";
import type { WorkflowJson } from "@/lib/workflow";

export const publicationKeys = {
  all: ["publication"] as const,
  versions: (workflowId: string) => ["versions", workflowId] as const,
  latest: (workflowId: string) => ["latest-version", workflowId] as const,
  workflows: ["workflows"] as const,
};

/**
 * Publish a workflow through the control plane: persists as a new immutable
 * version → validate+compile → atomic publish → surfaces backend truth.
 * On success the versions + workflows lists refetch from the server (the
 * authoritative source) — no client-side optimistic write, since publication
 * metadata must come from the backend.
 */
export function usePublishWorkflow() {
  const queryClient = useQueryClient();

  return useMutation<VersionInfo, Error, { workflow: WorkflowJson; lanes: Record<string, string> }>({
    mutationFn: ({ workflow, lanes }) => publishWorkflow(workflow, lanes),
    onSuccess: (info) => {
      queryClient.invalidateQueries({ queryKey: publicationKeys.versions(info.workflowId) });
      queryClient.invalidateQueries({ queryKey: publicationKeys.workflows });
    },
  });
}

/** Fetch the backend's authoritative versions list for a workflow. */
export function useWorkflowVersions(workflowId: string) {
  return useQuery({
    queryKey: publicationKeys.versions(workflowId),
    queryFn: () => fetchWorkflowVersions(workflowId),
  });
}

/** Fetch all workflows from the control plane (backend-derived truth). */
export function useWorkflows() {
  return useQuery({
    queryKey: publicationKeys.workflows,
    queryFn: fetchWorkflows,
  });
}

/** Fetch the latest immutable version for the editor's workflow. */
export function useWorkflowLatestVersion(workflowId: string) {
  return useQuery({
    queryKey: publicationKeys.latest(workflowId),
    queryFn: () => fetchWorkflowLatestVersion(workflowId),
  });
}

/** Save a new immutable version of the editor state. */
export function useSaveWorkflowMutation(workflowId: string) {
  const queryClient = useQueryClient();
  return useMutation<VersionRow, Error, WorkflowJson>({
    mutationFn: (workflow) => saveWorkflowVersion(workflowId, workflow),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: publicationKeys.versions(workflowId) });
      queryClient.invalidateQueries({ queryKey: publicationKeys.latest(workflowId) });
      queryClient.invalidateQueries({ queryKey: publicationKeys.workflows });
    },
  });
}

/** Validate + compile the current editor state against the control plane.
 *  `validateWorkflow` resolves the backend row via `workflow.id`; the caller
 *  must set `workflow.id` to match the editor's workflowId before calling. */
export function useValidateMutation() {
  return useMutation<{ plan_hash: string | null; status: string }, Error, WorkflowJson>({
    mutationFn: (workflow) => validateWorkflow(workflow),
  });
}

/** Compile-only alias of validate (POST /workflows/:id/compile — same
 *  backend handler, same shape; see api.compileWorkflow). */
export function useCompileMutation() {
  return useMutation<{ plan_hash: string | null; status: string }, Error, WorkflowJson>({
    mutationFn: (workflow) => compileWorkflow(workflow),
  });
}

/** Non-stream run of the published ACTIVE version (SSE-empty fallback). */
export function useRunWorkflowMutation() {
  const queryClient = useQueryClient();
  return useMutation<
    Awaited<ReturnType<typeof runWorkflow>>,
    Error,
    { workflowId: string; body: unknown }
  >({
    mutationFn: ({ workflowId, body }) => runWorkflow(workflowId, body),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["runs"] });
    },
  });
}

/** Real persisted provider rows (config only — no fabricated health). */
export function useProviders(projectId?: string) {
  return useQuery({
    queryKey: ["providers", projectId ?? "all"],
    queryFn: () => fetchProviders(projectId),
  });
}

/** Create a real provider record. */
export function useCreateProviderMutation() {
  const queryClient = useQueryClient();
  return useMutation<ProviderRow, Error, Parameters<typeof createProvider>[0]>({
    mutationFn: createProvider,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
  });
}

/** Update a real provider record (partial patch). */
export function useUpdateProviderMutation() {
  const queryClient = useQueryClient();
  return useMutation<
    ProviderRow,
    Error,
    { id: string; input: Parameters<typeof updateProvider>[1] }
  >({
    mutationFn: ({ id, input }) => updateProvider(id, input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
  });
}

/** Delete a real provider record. */
export function useDeleteProviderMutation() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: deleteProvider,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
  });
}

/** Real persisted lane rows. */
export function useLanes(projectId?: string) {
  return useQuery({
    queryKey: ["lanes", projectId ?? "all"],
    queryFn: () => fetchLanes(projectId),
  });
}

/** Create a real lane record. */
export function useCreateLaneMutation() {
  const queryClient = useQueryClient();
  return useMutation<Awaited<ReturnType<typeof createLane>>, Error, Parameters<typeof createLane>[0]>({
    mutationFn: createLane,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["lanes"] });
    },
  });
}

/** Update a real lane record (partial patch). */
export function useUpdateLaneMutation() {
  const queryClient = useQueryClient();
  return useMutation<
    Awaited<ReturnType<typeof updateLane>>,
    Error,
    { id: string; input: Parameters<typeof updateLane>[1] }
  >({
    mutationFn: ({ id, input }) => updateLane(id, input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["lanes"] });
    },
  });
}

/** Delete a real lane record. */
export function useDeleteLaneMutation() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: deleteLane,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["lanes"] });
    },
  });
}

/** Roll back a workflow to its previous validated version (backend republish). */
export function useRollbackWorkflowMutation() {
  const queryClient = useQueryClient();
  return useMutation<
    Awaited<ReturnType<typeof rollbackWorkflow>>,
    Error,
    string
  >({
    mutationFn: rollbackWorkflow,
    // Invalidate on settled, not just success: a failed rollback must still
    // refresh the cached versions/workflows so the UI shows backend truth
    // (a stuck or partial republish shouldn't leave stale state visible).
    onSettled: (_result, _error, workflowId) => {
      queryClient.invalidateQueries({ queryKey: publicationKeys.versions(workflowId) });
      queryClient.invalidateQueries({ queryKey: publicationKeys.workflows });
    },
  });
}

/** Deactivate a workflow (removes the active pointer, gateway stops serving it). */
export function useDeactivateWorkflowMutation() {
  const queryClient = useQueryClient();
  return useMutation<
    Awaited<ReturnType<typeof deactivateWorkflow>>,
    Error,
    string
  >({
    mutationFn: deactivateWorkflow,
    onSettled: (_result, _error, workflowId) => {
      queryClient.invalidateQueries({ queryKey: publicationKeys.versions(workflowId) });
      queryClient.invalidateQueries({ queryKey: publicationKeys.workflows });
    },
  });
}

/** Delete a workflow row (cascades versions + runs). */
export function useDeleteWorkflowMutation() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: deleteWorkflow,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: publicationKeys.workflows });
      // Runs for the deleted workflow cascade in the DB; drop the stale
      // cached rows too, or they keep rendering and 404 on click.
      queryClient.invalidateQueries({ queryKey: ["runs"] });
    },
  });
}

/** Rename a workflow row. */
export function useRenameWorkflowMutation() {
  const queryClient = useQueryClient();
  return useMutation<WorkflowRow, Error, { id: string; name: string }>({
    mutationFn: ({ id, name }) => renameWorkflow(id, name),
    onSuccess: (_row, vars) => {
      queryClient.invalidateQueries({ queryKey: publicationKeys.workflows });
      queryClient.invalidateQueries({ queryKey: ["workflow-row", vars.id] });
    },
  });
}

/** Run-history rows (optionally filtered by workflow and/or project). */
export function useRuns(workflowId?: string, projectId?: string) {
  return useQuery<RunRow[]>({
    queryKey: ["runs", workflowId ?? "all", projectId ?? "all"],
    queryFn: () => fetchRuns(workflowId, projectId),
    refetchInterval: (query) =>
      (query.state.data ?? []).some((r) => r.status === "running") ? 10_000 : false,
  });
}

/** Single run record for the detail page. */
export function useRun(runId: string) {
  return useQuery<RunRow>({
    queryKey: ["runs", "detail", runId],
    queryFn: () => fetchRun(runId),
    // Keep polling while the run is still live so the detail page actually
    // leaves `running` — the list polls, the detail page must too.
    refetchInterval: (query) =>
      query.state.data?.status === "running" ? 10_000 : false,
  });
}

/** Real control-plane + gateway health probe. */
export function useSystemHealth() {
  return useQuery({
    queryKey: ["system-health"],
    queryFn: fetchSystemHealth,
    refetchInterval: 15_000,
  });
}

/** Durable workflow row (A3 — real GET /workflows/:id, not versions[0]). */
export function useWorkflowRow(workflowId: string) {
  return useQuery({
    queryKey: ["workflow-row", workflowId],
    queryFn: () => fetchWorkflow(workflowId),
    enabled: workflowId !== "new",
  });
}

/** Catalog sync metadata (GET /catalog/status). */
export function useCatalogStatus() {
  return useQuery<CatalogStatus>({
    queryKey: ["catalog-status"],
    queryFn: fetchCatalogStatus,
    staleTime: 60_000,
  });
}

/** Catalog provider list (GET /catalog/providers). */
export function useCatalogProviders() {
  return useQuery<CatalogProvider[]>({
    queryKey: ["catalog-providers"],
    queryFn: fetchCatalogProviders,
    staleTime: 60_000,
  });
}

/** Catalog models from the models.dev sync (background-populated by control-plane).
 *  `enabled` lets callers avoid fetching until a relevant provider is selected
 *  (React Query dedupes identical keys, so stray calls are merely wasteful). */
export function useCatalogModels(
  params?: { provider?: string; capability?: string },
  options?: { enabled?: boolean },
) {
  return useQuery<CatalogModel[]>({
    queryKey: ["catalog-models", params],
    queryFn: () => fetchCatalogModels(params),
    staleTime: 60_000, // catalog refreshes every 24h; no need to re-fetch rapidly
    enabled: options?.enabled ?? true,
  });
}
