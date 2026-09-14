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
  createProvider,
  fetchLanes,
  fetchProviders,
  fetchSystemHealth,
  fetchWorkflowLatestVersion,
  fetchWorkflowVersions,
  fetchWorkflows,
  publishWorkflow,
  runWorkflow,
  saveWorkflowVersion,
  validateWorkflow,
  type ProviderRow,
  type RunResult,
  type VersionInfo,
  type VersionRow,
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

/** Run the workflow's ACTIVE (published) version through the real gateway.
 *  The mutation's status is the only execution-state source: pending →
 *  running, success → completed (real output), error → failed/cancelled (real
 *  backend envelope). No fabricated states. */
export function useRunWorkflowMutation() {
  return useMutation<
    RunResult,
    Error,
    { workflowId: string; body: unknown; signal?: AbortSignal }
  >({
    mutationFn: ({ workflowId, body, signal }) => runWorkflow(workflowId, body, signal),
  });
}

/** Real persisted provider rows (config only — no fabricated health). */
export function useProviders() {
  return useQuery({
    queryKey: ["providers"],
    queryFn: fetchProviders,
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

/** Real persisted lane rows. */
export function useLanes() {
  return useQuery({
    queryKey: ["lanes"],
    queryFn: () => fetchLanes(),
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

/** Latest persisted version row for the editor's workflow (durable truth). */
export function useWorkflowRow(workflowId: string) {
  return useQuery({
    queryKey: ["workflow-row", workflowId],
    queryFn: async () => {
      const rows = await fetchWorkflowVersions(workflowId);
      return rows.length > 0 ? rows[0] : null;
    },
    enabled: workflowId !== "new",
  });
}