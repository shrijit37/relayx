import { useEffect, useState } from "react";
import { createFileRoute, Link } from "@tanstack/react-router";
import {
  Area,
  AreaChart,
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { AppShell } from "@/components/relay/AppShell";
import { Bar, Metric, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { kpis, lanes, latencySeries, providers, runs } from "@/lib/relay-data";

export const Route = createFileRoute("/observability")({
  head: () => ({
    meta: [
      { title: "Observability — relay-x" },
      { name: "description", content: "p50/p95/p99 latency, gateway vs upstream split, throughput, streams, error rate and a trace explorer." },
      { property: "og:title", content: "Observability — relay-x" },
      { property: "og:description", content: "Explains where latency and failures originate without logging prompts or completions." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: ObservabilityPage,
});

const axis = {
  stroke: "var(--color-muted-foreground)",
  fontSize: 10,
  tickLine: false,
  axisLine: false,
};

function ChartFrame({ children }: { children: React.ReactElement }) {
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  if (!mounted) return <div className="h-[168px]" />;
  return (
    <div className="h-[168px] w-full">
      <ResponsiveContainer width="100%" height="100%">
        {children}
      </ResponsiveContainer>
    </div>
  );
}

const tooltipProps = {
  contentStyle: {
    background: "var(--color-popover)",
    border: "1px solid var(--color-border)",
    borderRadius: 4,
    fontSize: 11,
    fontFamily: "var(--font-mono)",
  },
  labelStyle: { color: "var(--color-muted-foreground)" },
} as const;

function ObservabilityPage() {
  return (
    <AppShell>
      <PageHeader title="Observability" subtitle="Last 24 hours · protocol metadata only, no prompt or completion capture." />
      <div className="space-y-3 p-4">
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4 2xl:grid-cols-8">
          {kpis.map((k) => (
            <Metric key={k.label} {...k} />
          ))}
        </div>

        <div className="grid gap-3 lg:grid-cols-2">
          <Panel title="Latency percentiles (ms)">
            <ChartFrame>
              <LineChart data={latencySeries} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
                <CartesianGrid stroke="var(--color-border)" strokeDasharray="2 4" vertical={false} />
                <XAxis dataKey="t" {...axis} minTickGap={28} />
                <YAxis {...axis} width={34} />
                <Tooltip {...tooltipProps} />
                <Line type="monotone" dataKey="p50" stroke="var(--color-chart-2)" dot={false} strokeWidth={1.4} />
                <Line type="monotone" dataKey="p95" stroke="var(--color-chart-1)" dot={false} strokeWidth={1.4} />
                <Line type="monotone" dataKey="p99" stroke="var(--color-chart-4)" dot={false} strokeWidth={1.4} />
              </LineChart>
            </ChartFrame>
          </Panel>

          <Panel title="Gateway vs upstream (ms)">
            <ChartFrame>
              <AreaChart data={latencySeries} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
                <CartesianGrid stroke="var(--color-border)" strokeDasharray="2 4" vertical={false} />
                <XAxis dataKey="t" {...axis} minTickGap={28} />
                <YAxis {...axis} width={34} />
                <Tooltip {...tooltipProps} />
                <Area type="monotone" dataKey="upstream" stroke="var(--color-chart-4)" fill="var(--color-chart-4)" fillOpacity={0.12} strokeWidth={1.2} />
                <Area type="monotone" dataKey="gateway" stroke="var(--color-chart-1)" fill="var(--color-chart-1)" fillOpacity={0.2} strokeWidth={1.2} />
              </AreaChart>
            </ChartFrame>
            <p className="mt-1 text-[11px] text-muted-foreground">
              Gateway overhead is measured in-process (p95 3.4 ms) and never reported as zero.
            </p>
          </Panel>

          <Panel title="Throughput (rps) & active streams">
            <ChartFrame>
              <AreaChart data={latencySeries} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
                <CartesianGrid stroke="var(--color-border)" strokeDasharray="2 4" vertical={false} />
                <XAxis dataKey="t" {...axis} minTickGap={28} />
                <YAxis {...axis} width={34} />
                <Tooltip {...tooltipProps} />
                <Area type="monotone" dataKey="rps" stroke="var(--color-chart-2)" fill="var(--color-chart-2)" fillOpacity={0.14} strokeWidth={1.2} />
                <Area type="monotone" dataKey="streams" stroke="var(--color-chart-1)" fill="var(--color-chart-1)" fillOpacity={0.14} strokeWidth={1.2} />
              </AreaChart>
            </ChartFrame>
          </Panel>

          <Panel title="Error rate (%)">
            <ChartFrame>
              <AreaChart data={latencySeries} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
                <CartesianGrid stroke="var(--color-border)" strokeDasharray="2 4" vertical={false} />
                <XAxis dataKey="t" {...axis} minTickGap={28} />
                <YAxis {...axis} width={34} />
                <Tooltip {...tooltipProps} />
                <Area type="monotone" dataKey="errors" stroke="var(--color-chart-5)" fill="var(--color-chart-5)" fillOpacity={0.15} strokeWidth={1.2} />
              </AreaChart>
            </ChartFrame>
          </Panel>
        </div>

        <div className="grid gap-3 lg:grid-cols-2">
          <Panel title="Provider latency">
            <div className="space-y-2.5">
              {providers.map((p) => (
                <div key={p.id}>
                  <div className="flex items-center justify-between pb-1">
                    <span className="text-xs">{p.name}</span>
                    <span className="num text-[11px] text-muted-foreground">{p.latency}</span>
                  </div>
                  <Bar value={p.latency === "—" ? 0 : parseInt(p.latency) / 3} tone={p.status === "degraded" ? "warn" : "info"} />
                </div>
              ))}
            </div>
          </Panel>
          <Panel title="Lane health">
            <div className="space-y-2.5">
              {lanes.map((l) => (
                <div key={l.id}>
                  <div className="flex items-center justify-between pb-1">
                    <span className="num text-xs">{l.id}</span>
                    <StatusText status={l.health} />
                  </div>
                  <Bar value={parseFloat(l.reuse)} tone={l.health === "degraded" ? "warn" : "ok"} />
                </div>
              ))}
            </div>
          </Panel>
        </div>

        <Panel title="Trace explorer" dense>
          <TableShell head={["Request ID", "Workflow", "Version", "Route", "Lane", "Provider", "TTFB", "Total", "Gateway", "Status"]}>
            {runs.map((r) => (
              <tr key={r.id} className="hover:bg-panel-raised/50">
                <Td>
                  <Link to="/runs/$runId" params={{ runId: r.id }} className="num hover:text-primary">
                    #{r.id}
                  </Link>
                </Td>
                <Td className="text-muted-foreground">{r.workflow}</Td>
                <Td className="num">v{r.version}</Td>
                <Td className="num">{r.route}</Td>
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
      </div>
    </AppShell>
  );
}
