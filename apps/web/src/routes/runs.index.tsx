import { createFileRoute, Link } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { Metric, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { errorCategories, runs } from "@/lib/relay-data";

export const Route = createFileRoute("/runs/")({
  head: () => ({
    meta: [
      { title: "Runs — relay-x" },
      { name: "description", content: "Inspect gateway runs with measured gateway overhead, upstream TTFB and stable error categories." },
      { property: "og:title", content: "Runs — relay-x" },
      { property: "og:description", content: "Every run separates gateway latency from upstream latency." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: RunsPage,
});

function RunsPage() {
  return (
    <AppShell>
      <PageHeader title="Runs" subtitle="Last 15 minutes · 412 rps · sampling 100% of failures" />
      <div className="space-y-3 p-4">
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
          <Metric label="Completed" value="36,412" delta="+6.1%" tone="ok" hint="15 min" />
          <Metric label="Failed" value="152" delta="+0.08%" tone="fail" hint="stable categories" />
          <Metric label="Cancelled" value="64" tone="neutral" hint="client aborts" />
          <Metric label="Gateway p95" value="3.4" unit="ms" tone="ok" hint="measured, not claimed" />
        </div>

        <div className="grid gap-3 xl:grid-cols-4">
          <Panel title="Trace explorer" className="xl:col-span-3" dense>
            <TableShell head={["Request ID", "Workflow", "Version", "Route", "Lane", "Provider", "TTFB", "Total", "Gateway", "Status", "When"]}>
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
                  <Td className="num text-muted-foreground">{r.when}</Td>
                </tr>
              ))}
            </TableShell>
          </Panel>

          <Panel title="Error categories">
            <div className="space-y-1.5">
              {errorCategories.map((e) => (
                <div key={e.code} className="flex items-center gap-2">
                  <span className="num min-w-0 flex-1 truncate text-[11px] text-muted-foreground">{e.code}</span>
                  <span className="num text-[11px]">{e.count}</span>
                </div>
              ))}
            </div>
            <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground">
              Failures are classified deterministically. No generic “something went wrong”.
            </p>
          </Panel>
        </div>
      </div>
    </AppShell>
  );
}
