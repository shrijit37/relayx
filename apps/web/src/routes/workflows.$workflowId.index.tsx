import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { WorkflowBuilder } from "@/components/relay/workflow/WorkflowBuilder";

export const Route = createFileRoute("/workflows/$workflowId/")({
  head: () => ({
    meta: [
      { title: "Workflow editor — relay-x" },
      {
        name: "description",
        content: "Visual gateway workflow editor: routes, lanes, protocol translation, MCP discovery, streaming output and live debug.",
      },
      { property: "og:title", content: "Workflow editor — relay-x" },
      { property: "og:description", content: "Design routing, translation, authorization and streaming for every AI request." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: WorkflowEditorPage,
});

function WorkflowEditorPage() {
  return (
    <AppShell flush>
      <div className="h-full min-h-0">
        <WorkflowBuilder />
      </div>
    </AppShell>
  );
}
