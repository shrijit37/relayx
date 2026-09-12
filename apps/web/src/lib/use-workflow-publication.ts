/**
 * React Query hooks for the control-plane API boundary.
 *
 * React Query owns server state; the editor keeps local canvas state. Hooks
 * degrade to an explicit error when the control plane is unreachable (no
 * silent fake-success path). Publication metadata shown to the user always
 * comes from the backend.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { fetchWorkflowVersions, fetchWorkflows, publishWorkflow, type VersionInfo } from "@/lib/api";
import type { WorkflowJson } from "@/lib/workflow-serializer";

export const publicationKeys = {
  all: ["publication"] as const,
  version: (workflowId: string) => ["publication", workflowId] as const,
  versions: (workflowId: string) => ["versions", workflowId] as const,
  workflows: ["workflows"] as const,
};

/**
 * Publish a workflow through the control plane: persists as a new immutable
 * version → validate+compile → atomic publish → surfaces backend truth.
 */
export function usePublishWorkflow() {
  const queryClient = useQueryClient();

  return useMutation<VersionInfo, Error, { workflow: WorkflowJson; lanes: Record<string, string> }>({
    mutationFn: ({ workflow, lanes }) => publishWorkflow(workflow, lanes),
    onSuccess: (info) => {
      queryClient.setQueryData(publicationKeys.version(info.workflowId), info);
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