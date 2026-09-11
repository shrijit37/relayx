import { createFileRoute } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { Bar, KV, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { lanes } from "@/lib/relay-data";

export const Route = createFileRoute("/lanes")({
  head: () => ({
    meta: [
      { title: "Lanes — relay-x" },
      { name: "description", content: "Network lanes combine provider endpoint, network route, policy and connection pool as one routing primitive." },
      { property: "og:title", content: "Lanes — relay-x" },
      { property: "og:description", content: "Network path is a first-class concept: WireGuard, direct egress or proxy." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: LanesPage,
});

const topology = [
  { path: "US VPN", detail: "WireGuard US-01", target: "Anthropic", health: "healthy", latency: "82 ms" },
  { path: "EU VPN", detail: "WireGuard EU-03", target: "Anthropic", health: "healthy", latency: "119 ms" },
  { path: "Direct", detail: "Direct egress", target: "OpenAI", health: "healthy", latency: "104 ms" },
  { path: "Proxy", detail: "HTTP proxy APAC-02", target: "Mistral", health: "degraded", latency: "241 ms" },
];

function LanesPage() {
  return (
    <AppShell>
      <PageHeader
        title="Lanes"
        subtitle="A lane binds provider endpoint + network route + policy + connection pool."
        actions={
          <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90">
            <Plus className="size-3.5" /> Add lane
          </button>
        }
      />
      <div className="grid gap-3 p-4 xl:grid-cols-3">
        <div className="grid gap-2 md:grid-cols-2 xl:col-span-2">
          {lanes.map((l) => (
            <Panel key={l.id} title={l.id} actions={<StatusText status={l.health} />}>
              <div className="space-y-px">
                <KV k="Provider" v={l.provider} />
                <KV k="Endpoint" v={l.endpoint} />
                <KV k="Network" v={l.network} />
                <KV k="Region" v={l.region} />
                <KV k="Policy" v={l.policy} />
                <KV k="Pool" v={`${l.pool} connections`} />
                <KV k="Latency" v={l.latency} />
                <KV k="Error rate" v={l.errors} tone={l.health === "degraded" ? "warn" : "ok"} />
              </div>
              <div className="mt-2 border-t border-border pt-2">
                <div className="flex items-center justify-between pb-1">
                  <span className="label-xs">Connection reuse</span>
                  <span className="num text-[11px]">{l.reuse}</span>
                </div>
                <Bar value={parseFloat(l.reuse)} tone={l.health === "degraded" ? "warn" : "ok"} />
              </div>
            </Panel>
          ))}
        </div>

        <div className="grid content-start gap-3">
          <Panel title="Egress topology">
            <div className="num text-[11px] leading-6">
              <div>Gateway</div>
              {topology.map((t, i) => (
                <div key={t.path} className="flex items-center gap-2">
                  <span className="text-muted-foreground">
                    {i === topology.length - 1 ? "└──" : "├──"}
                  </span>
                  <span>{t.path}</span>
                  <span className="text-muted-foreground">───</span>
                  <span>{t.target}</span>
                  <span className="ml-auto flex items-center gap-1.5 text-muted-foreground">
                    {t.latency}
                    <StatusText status={t.health} className="[&>span:last-child]:hidden" />
                  </span>
                </div>
              ))}
            </div>
          </Panel>
          <Panel title="Pool summary" dense>
            <TableShell head={["Lane", "Pool", "Reuse", "Health"]}>
              {lanes.map((l) => (
                <tr key={l.id}>
                  <Td className="num">{l.id}</Td>
                  <Td className="num">{l.pool}</Td>
                  <Td className="num">{l.reuse}</Td>
                  <Td>
                    <StatusText status={l.health} />
                  </Td>
                </tr>
              ))}
            </TableShell>
          </Panel>
        </div>
      </div>
    </AppShell>
  );
}
