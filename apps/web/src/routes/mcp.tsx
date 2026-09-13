import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader } from "@/components/relay/primitives";

export const Route = createFileRoute("/mcp")({
  head: () => ({
    meta: [
      { title: "MCP / Tools — relay-x" },
      { name: "description", content: "MCP server registry and tool discovery." },
      { property: "og:title", content: "MCP / Tools — relay-x" },
      { property: "og:description", content: "MCP server registry and tool discovery." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: McpPage,
});

function McpPage() {
  return (
    <AppShell>
      <PageHeader title="MCP / Tools" subtitle="Capability registry — registered tools are not automatically exposed to the model." />
      <div className="p-4">
        <EmptyState
          title="Not available yet."
          body={
            "The MCP registry, discovery index, and tool-activation runtime are Phase 7 work.\n\n" +
            "This view will become available when the MCP backend is implemented. Node kinds on " +
            "the canvas (MCP Discovery) remain display-only until then."
          }
        />
      </div>
    </AppShell>
  );
}