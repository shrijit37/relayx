import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader, Panel } from "@/components/relay/primitives";
import { useSystemHealth } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/observability")({
  head: () => ({
    meta: [
      { title: "Observability — relay-x" },
      { name: "description", content: "Latency, throughput, streams and error rates." },
      { property: "og:title", content: "Observability — relay-x" },
      { property: "og:description", content: "Latency, throughput, streams and error rates." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: ObservabilityPage,
});

function ObservabilityPage() {
  const { data: health } = useSystemHealth();

  return (
    <AppShell>
      <PageHeader title="Observability" subtitle="Live gateway and control-plane signal." />
      <div className="space-y-3 p-4">
        <Panel title="System health" dense>
          <div className="p-3">
            <EmptyState
              title="No telemetry available."
              body={
                "Time-series metrics (latency percentiles, throughput, error rates) are not " +
                "ingested or exposed yet — there is no metrics/telemetry backend.\n\n" +
                "Gateway Prometheus metrics exist on the admin listener, but are not yet " +
                "proxied to this console."
              }
            />
          </div>
        </Panel>
        {health && (
          <Panel title="Live probes" dense>
            <div className="space-y-px p-3">
              <div className="flex items-center justify-between py-1">
                <span className="text-xs text-muted-foreground">Control plane /healthz</span>
                <span className="num text-xs">{health.control_plane.status}</span>
              </div>
              <div className="flex items-center justify-between py-1">
                <span className="text-xs text-muted-foreground">Gateway /healthz</span>
                <span className="num text-xs">{health.gateway.healthz.status}</span>
              </div>
              <div className="flex items-center justify-between py-1">
                <span className="text-xs text-muted-foreground">Gateway /ready</span>
                <span className="num text-xs">{health.gateway.ready.status}</span>
              </div>
            </div>
          </Panel>
        )}
      </div>
    </AppShell>
  );
}