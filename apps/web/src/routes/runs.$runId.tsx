import { createFileRoute, Link } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader } from "@/components/relay/primitives";

export const Route = createFileRoute("/runs/$runId")({
  head: () => ({
    meta: [
      { title: "Run detail — relay-x" },
      { name: "description", content: "Run detail and latency waterfall." },
      { property: "og:title", content: "Run detail — relay-x" },
      { property: "og:description", content: "Run detail and latency waterfall." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: RunDetail,
});

function RunDetail() {
  const { runId } = Route.useParams();
  return (
    <AppShell>
      <PageHeader
        title={`RUN #${runId}`}
        subtitle="Run detail"
        meta={
          <Link to="/runs" className="flex items-center gap-1 text-xs text-muted-foreground hover:text-primary">
            <ArrowLeft className="size-3" /> all runs
          </Link>
        }
      />
      <div className="p-4">
        <EmptyState
          title="This run is not available."
          body={
            "There is no execution-history backend yet, so individual run records are " +
            "not persisted.\n\nThis view will become available when a run-history backend is implemented."
          }
        />
      </div>
    </AppShell>
  );
}