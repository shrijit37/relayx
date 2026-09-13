import { createFileRoute, Link } from "@tanstack/react-router";
import { ArrowUpRight } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import {
  EmptyState,
  KV,
  Panel,
  PageHeader,
  StatusText,
  TableShell,
  Td,
} from "@/components/relay/primitives";
import { useLanes, useProviders, useSystemHealth, useWorkflows } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/")({
  head: () => ({
    meta: [
      { title: "relay-x — Visual AI Gateway & Agent Orchestrator" },
      {
        name: "description",
        content:
          "relay-x is an ultra-low-latency visual AI gateway: workflow compilation, provider routing, network lanes, MCP discovery and measured latency observability.",
      },
      { property: "og:title", content: "relay-x — Visual AI Gateway & Agent Orchestrator" },
      {
        property: "og:description",
        content: "Design exactly how an AI request is routed, translated, authorized, networked, executed and streamed.",
      },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: Overview,
});

function Overview() {
  const { data: workflows, isPending: wfPending, isError: wfError, error: wfErr } = useWorkflows();
  const { data: lanes } = useLanes();
  const { data: providers } = useProviders();
  const { data: health } = useSystemHealth();

  return (
    <AppShell>
      <PageHeader
        title="Overview"
        subtitle="Control plane snapshot · data plane serving from compiled execution plans"
        meta={
          <>
            {health ? (
              <StatusText status={health.gateway.ready.status === "ready" ? "healthy" : health.gateway.ready.status} />
            ) : (
              <StatusText status="unknown" />
            )}
            <span className="num text-xs text-muted-foreground">
              {workflows ? `${workflows.length} workflows · ${lanes?.length ?? "—"} lanes · ${providers?.length ?? "—"} providers` : "loading…"}
            </span>
          </>
        }
        actions={
          <Link
            to="/workflows/$workflowId"
            params={{ workflowId: "new" }}
            className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90"
          >
            Open workflow editor
          </Link>
        }
      />

      <div className="space-y-3 p-4">
        <Panel title="Backend status" dense>
          <div className="space-y-px">
            <KV k="Control plane" v={health?.control_plane.status ?? "unreachable"} tone={health?.control_plane.status === "ok" ? "ok" : "warn"} />
            <KV k="Gateway /healthz" v={health?.gateway.healthz.status ?? "unreachable"} tone={health?.gateway.healthz.status === "ok" ? "ok" : "warn"} />
            <KV k="Gateway /ready" v={health?.gateway.ready.status ?? "unreachable"} tone={health?.gateway.ready.status === "ready" ? "ok" : "warn"} />
          </div>
          <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground">
            KPI telemetry (latency, throughput, error rates) is not available until a metrics/telemetry backend is
            implemented. This page shows real persisted infrastructure state only.
          </p>
        </Panel>

        <Panel title="Workflows" className="xl:col-span-2" dense>
          {wfPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading workflows…</div>
          ) : wfError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(wfErr)}</div>
          ) : workflows && workflows.length > 0 ? (
            <TableShell head={["Workflow", "Version", "State", "Created"]}>
              {workflows.map((w) => (
                <tr key={w.id} className="hover:bg-panel-raised/50">
                  <Td>
                    <Link
                      to="/workflows/$workflowId"
                      params={{ workflowId: w.id }}
                      className="flex items-center gap-1 font-medium hover:text-primary"
                    >
                      {w.name} <ArrowUpRight className="size-3 opacity-50" />
                    </Link>
                  </Td>
                  <Td className="num">{new Date(w.created_at).toLocaleDateString()}</Td>
                  <Td>
                    <StatusText status={w.status === "active" ? "Production" : w.status === "compiled" ? "Staging" : "Draft"} />
                  </Td>
                  <Td className="num text-muted-foreground">{w.id}</Td>
                </tr>
              ))}
            </TableShell>
          ) : (
            <div className="p-4 text-xs text-muted-foreground">
              no workflows persisted yet — create one to get started.
            </div>
          )}
        </Panel>

        <div className="grid gap-3 xl:grid-cols-3">
          <Panel title="Lanes">
            <div className="space-y-2">
              {lanes && lanes.length > 0 ? (
                lanes.map((l) => (
                  <div key={l.id} className="flex items-center gap-2">
                    <span className="num min-w-0 flex-1 truncate text-xs">{l.id}</span>
                    <span className="num text-xs text-muted-foreground">{l.egress}</span>
                  </div>
                ))
              ) : (
                <p className="text-[11px] text-muted-foreground">no lanes persisted yet.</p>
              )}
            </div>
          </Panel>
          <Panel title="Providers">
            <div className="space-y-2">
              {providers && providers.length > 0 ? (
                providers.map((p) => (
                  <div key={p.id} className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate text-xs">{p.name}</span>
                    <span className="num text-xs text-muted-foreground">{p.protocol}</span>
                  </div>
                ))
              ) : (
                <p className="text-[11px] text-muted-foreground">no providers persisted yet.</p>
              )}
            </div>
          </Panel>
        </div>

        <Panel title="Archived workspace" dense>
          <div className="p-3">
            <EmptyState
              title="No archived workflows."
              body={"Archived versions are retained for 90 days.\nRoll back from the version history of any workflow."}
              action={
                <Link
                  to="/workflows"
                  className="focus-ring inline-flex h-7 items-center rounded-sm border border-border-strong px-2.5 text-xs hover:border-primary hover:text-primary"
                >
                  Browse workflows
                </Link>
              }
            />
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}