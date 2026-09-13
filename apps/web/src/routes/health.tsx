import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, StatusText } from "@/components/relay/primitives";
import { useSystemHealth } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/health")({
  head: () => ({
    meta: [
      { title: "System health — relay-x" },
      { name: "description", content: "Control plane and Rust data plane live health." },
      { property: "og:title", content: "System health — relay-x" },
      { property: "og:description", content: "Control plane and data plane are separated; serving does not depend on authoring." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: HealthPage,
});

function HealthPage() {
  const { data: health, isPending, isError, error } = useSystemHealth();

  return (
    <AppShell>
      <PageHeader title="Health" subtitle="Control plane (authoring, compilation) is isolated from the Rust data plane (serving)." />
      <div className="grid gap-3 p-4 xl:grid-cols-3">
        <Panel title="Control plane" className="xl:col-span-2" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">probing…</div>
          ) : isError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(error)}</div>
          ) : health ? (
            <div className="space-y-px p-3">
              <KV k="API /healthz" v={health.control_plane.status} />
              <KV k="Service" v={health.control_plane.service ?? "relayx-control-plane"} />
            </div>
          ) : null}
        </Panel>
        <Panel title="Gateway (Rust data plane)">
          {isPending ? (
            <div className="p-3 text-[11px] text-muted-foreground">probing…</div>
          ) : isError || !health ? (
            <div className="p-3 text-[11px] text-fail">gateway unreachable</div>
          ) : (
            <div className="space-y-px">
              <KV k="/healthz" v={health.gateway.healthz.status} />
              <KV k="/ready" v={health.gateway.ready.status} />
              <div className="mt-2 flex items-center gap-2">
                <StatusText
                  status={health.gateway.ready.status === "ready" ? "ready" : health.gateway.ready.status}
                />
              </div>
            </div>
          )}
        </Panel>
      </div>
    </AppShell>
  );
}