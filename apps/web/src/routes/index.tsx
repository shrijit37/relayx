import { createFileRoute, Link } from "@tanstack/react-router";
import { ArrowUpRight } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, KV, Metric, Panel, PageHeader, StatusText, Tag, TableShell, Td } from "@/components/relay/primitives";
import { kpis, lanes, providers, runs, workflows } from "@/lib/relay-data";

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
  return (
    <AppShell>
      <PageHeader
        title="Overview"
        subtitle="Control plane snapshot · data plane serving from compiled execution plans"
        meta={
          <>
            <StatusText status="healthy" />
            <span className="num text-xs text-muted-foreground">4 workflows · 4 lanes · 3 active providers</span>
          </>
        }
        actions={
          <Link
            to="/workflows/$workflowId"
            params={{ workflowId: "production-gateway" }}
            className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90"
          >
            Open workflow editor
          </Link>
        }
      />

      <div className="space-y-3 p-4">
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4 2xl:grid-cols-8">
          {kpis.map((k) => (
            <Metric key={k.label} {...k} />
          ))}
        </div>

        <div className="grid gap-3 xl:grid-cols-3">
          <Panel title="Workflows" className="xl:col-span-2" dense>
            <TableShell head={["Workflow", "Version", "State", "Env", "Nodes", "p95", "rps", "Updated"]}>
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
                  <Td className="num">v{w.version}</Td>
                  <Td>
                    <StatusText status={w.state} />
                  </Td>
                  <Td className="num text-muted-foreground">{w.env}</Td>
                  <Td className="num">{w.nodes}</Td>
                  <Td className="num">{w.p95}</Td>
                  <Td className="num">{w.rps}</Td>
                  <Td className="num text-muted-foreground">{w.updated}</Td>
                </tr>
              ))}
            </TableShell>
          </Panel>

          <Panel title="Control plane / data plane">
            <div className="space-y-2 text-[11px] leading-relaxed text-muted-foreground">
              <p>
                Authoring, compilation and policy evaluation happen in the control plane. The Rust data plane serves
                requests from an immutable execution plan — no database read in the hot path.
              </p>
            </div>
            <div className="mt-3 space-y-px">
              <KV k="Active plan" v="plan_8f31a2 (v24)" />
              <KV k="Plan propagation" v="112 ms" />
              <KV k="Config snapshot age" v="2 min" />
              <KV k="Hot-path DB reads" v="0" tone="ok" />
              <KV k="Gateway overhead p95" v="3.4 ms" tone="ok" />
            </div>
          </Panel>
        </div>

        <div className="grid gap-3 xl:grid-cols-3">
          <Panel title="Recent runs" className="xl:col-span-2" dense>
            <TableShell head={["Run", "Workflow", "Lane", "Provider", "TTFB", "Total", "Gateway", "Status"]}>
              {runs.slice(0, 5).map((r) => (
                <tr key={r.id} className="hover:bg-panel-raised/50">
                  <Td>
                    <Link to="/runs/$runId" params={{ runId: r.id }} className="num hover:text-primary">
                      #{r.id}
                    </Link>
                  </Td>
                  <Td className="text-muted-foreground">{r.workflow}</Td>
                  <Td className="num">{r.lane}</Td>
                  <Td>{r.provider}</Td>
                  <Td className="num">{r.ttfb}</Td>
                  <Td className="num">{r.total}</Td>
                  <Td className="num text-ok">{r.gateway}</Td>
                  <Td>
                    <StatusText status={r.status} />
                  </Td>
                </tr>
              ))}
            </TableShell>
          </Panel>

          <div className="grid gap-3">
            <Panel title="Lane health">
              <div className="space-y-2">
                {lanes.map((l) => (
                  <div key={l.id} className="flex items-center gap-2">
                    <StatusText status={l.health} className="w-[86px] shrink-0" />
                    <span className="num min-w-0 flex-1 truncate text-xs">{l.id}</span>
                    <span className="num text-xs text-muted-foreground">{l.latency}</span>
                  </div>
                ))}
              </div>
            </Panel>
            <Panel title="Providers">
              <div className="space-y-2">
                {providers.map((p) => (
                  <div key={p.id} className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate text-xs">{p.name}</span>
                    <Tag>{p.protocol}</Tag>
                    <span className="num w-16 text-right text-xs text-muted-foreground">{p.latency}</span>
                  </div>
                ))}
              </div>
            </Panel>
          </div>
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
