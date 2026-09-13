import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader } from "@/components/relay/primitives";

export const Route = createFileRoute("/runs/")({
  head: () => ({
    meta: [
      { title: "Runs — relay-x" },
      { name: "description", content: "Gateway run history and trace explorer." },
      { property: "og:title", content: "Runs — relay-x" },
      { property: "og:description", content: "Gateway run history and trace explorer." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: RunsPage,
});

function RunsPage() {
  return (
    <AppShell>
      <PageHeader title="Runs" subtitle="Execution history and per-run telemetry." />
      <div className="p-4">
        <EmptyState
          title="Run history is not available yet."
          body={
            "There is no execution-history backend yet: gateway runs are executed in-memory " +
            "and no durable run records are stored.\n\nThis view will become available when a " +
            "run-history backend is implemented."
          }
        />
      </div>
    </AppShell>
  );
}