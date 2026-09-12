/**
 * React Query hooks for the workflow API boundary.
 *
 * React Query owns server state; the editor keeps local canvas state. Each
 * hook degrades to an explicit error when the control plane/gateway admin is
 * not reachable (no silent fake-success path).
 */

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { publishWorkflow, type VersionInfo } from "@/lib/api";
import type { WorkflowJson } from "@/lib/workflow-serializer";

export const publicationKeys = {
  all: ["publication"] as const,
  version: (workflowId: string) => ["publication", workflowId] as const,
};

/**
 * Publish a workflow: sends the serialized Workflow JSON + lanes to the
 * gateway admin `/publish`, and invalidates the cached publication version on
 * success so the versions UI reflects reality.
 */
export function usePublishWorkflow() {
  const queryClient = useQueryClient();

  return useMutation<VersionInfo, Error, { workflow: WorkflowJson; lanes: Record<string, string> }>({
    mutationFn: ({ workflow, lanes }) => publishWorkflow(workflow, lanes),
    onSuccess: (info) => {
      queryClient.setQueryData(publicationKeys.version(info.workflowId), info);
    },
  });
}