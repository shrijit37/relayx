import { createFileRoute, Link } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, Metric, PageHeader, Panel, StatusText, Tag } from "@/components/relay/primitives";
import { waterfall } from "@/lib/relay-data";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/runs/$runId")({
  head: () => ({
    meta: [
      { title: "Run detail — relay-x" },
      { name: "description", content: "Phase-by-phase waterfall separating gateway overhead from upstream provider latency." },
      { property: "og:title", content: "Run detail — relay-x" },
      { property: "og:description", content: "Authentication, routing, translation, TTFB and streaming, measured in milliseconds." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: RunDetail,
});

function RunDetail() {
  const { runId } = Route.useParams();
  const total = waterfall.reduce((a, b) => a + b.ms, 0);
  let cursor = 0;
  const executionPath = [
    { id: "input", label: "Input", ms: "0.2 ms" },
    { id: "route", label: "Route", ms: "0.1 ms" },
    { id: "lane-us", label: "Lane", ms: "0.3 ms" },
    { id: "transform", label: "Protocol translation", ms: "1.2 ms" },
    { id: "provider", label: "Anthropic", ms: "420 ms TTFB" },
    { id: "output", label: "Streaming", ms: "1.39 s" },
  ];

  return (
    <AppShell>
      <PageHeader
        title={`RUN #${runId}`}
        subtitle="Production · Workflow v24 · plan_8f31a2"
        meta={
          <>
            <StatusText status="Completed" />
            <Link to="/runs" className="flex items-center gap-1 text-xs text-muted-foreground hover:text-primary">
              <ArrowLeft className="size-3" /> all runs
            </Link>
          </>
        }
        actions={
          <button className="focus-ring h-7 rounded-sm border border-border px-2.5 text-xs hover:border-border-strong">
            Export trace
          </button>
        }
      />

      <div className="space-y-3 p-4">
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
          <Metric label="Total" value="1.82" unit="s" hint="end to end" />
          <Metric label="Gateway" value="3.4" unit="ms" tone="ok" delta="0.19% of total" hint="in-process overhead" />
          <Metric label="Upstream" value="1.79" unit="s" hint="provider time" />
          <Metric label="TTFB" value="420" unit="ms" hint="first streamed token" />
        </div>

        <div className="grid gap-3 xl:grid-cols-3">
          <Panel title="Latency waterfall" className="xl:col-span-2">
            <div className="space-y-1.5">
              {waterfall.map((p) => {
                const left = (cursor / total) * 100;
                const width = Math.max(0.6, (p.ms / total) * 100);
                cursor += p.ms;
                return (
                  <div key={p.phase} className="grid grid-cols-[150px_1fr_78px] items-center gap-3">
                    <span className="truncate text-[11px] text-muted-foreground">{p.phase}</span>
                    <div className="relative h-3 rounded-sm bg-canvas">
                      <div
                        className={cn(
                          "absolute top-0 h-3 rounded-sm",
                          p.kind === "gateway" ? "bg-primary" : "bg-violet/70",
                        )}
                        style={{ left: `${left}%`, width: `${width}%` }}
                      />
                    </div>
                    <span className="num text-right text-[11px]">
                      {p.ms < 10 ? `${p.ms.toFixed(1)} ms` : p.ms >= 1000 ? `${(p.ms / 1000).toFixed(2)} s` : `${p.ms} ms`}
                    </span>
                  </div>
                );
              })}
            </div>
            <div className="mt-3 flex items-center gap-4 border-t border-border pt-2">
              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <span className="size-2 rounded-[2px] bg-primary" /> gateway (3.4 ms)
              </span>
              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <span className="size-2 rounded-[2px] bg-violet/70" /> upstream (1.79 s)
              </span>
            </div>
          </Panel>

          <div className="grid content-start gap-3">
            <Panel title="Request">
              <KV k="Route" v="claude-*" />
              <KV k="Lane" v="anthropic-us-vpn" />
              <KV k="Network" v="WireGuard US-01" />
              <KV k="Provider" v="Anthropic" />
              <KV k="Model" v="claude-sonnet-4.5" />
              <KV k="Translation" v="OpenAI → Anthropic" />
              <KV k="Connection" v="reused (pool)" tone="ok" />
              <KV k="Retries" v="0" />
            </Panel>
            <Panel title="Capability resolution">
              <div className="flex flex-wrap gap-1">
                <Tag tone="ok">streaming</Tag>
                <Tag tone="ok">tools</Tag>
                <Tag tone="ok">reasoning</Tag>
                <Tag tone="ok">structured output</Tag>
                <Tag tone="ok">deferred tools</Tag>
              </div>
              <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
                No prompt or completion content is captured. Only protocol-level metadata is retained.
              </p>
            </Panel>
          </div>
        </div>

        <Panel title="Execution path">
          <ol className="flex flex-wrap items-center gap-x-2 gap-y-3">
            {executionPath.map((s, i) => (
              <li key={s.id} className="flex items-center gap-2">
                <span className="rounded-sm border border-border bg-canvas px-2 py-1 text-[11px]">{s.label}</span>
                {i < executionPath.length - 1 && (
                  <span className="num text-[10px] text-muted-foreground">↓ {executionPath[i + 1]?.ms}</span>
                )}
              </li>
            ))}
          </ol>
        </Panel>
      </div>
    </AppShell>
  );
}
