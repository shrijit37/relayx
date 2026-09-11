import { createFileRoute } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, StatusText, Tag, TableShell, Td } from "@/components/relay/primitives";
import { capabilityMatrix, providers } from "@/lib/relay-data";

export const Route = createFileRoute("/providers")({
  head: () => ({
    meta: [
      { title: "Providers — relay-x" },
      { name: "description", content: "Provider adapters with machine-readable capabilities, endpoints, protocol fidelity and health." },
      { property: "og:title", content: "Providers — relay-x" },
      { property: "og:description", content: "Protocol features are exposed, never flattened to a lowest common denominator." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: ProvidersPage,
});

function ProvidersPage() {
  return (
    <AppShell>
      <PageHeader
        title="Providers"
        subtitle="Adapters expose machine-readable capabilities used during capability resolution."
        actions={
          <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90">
            <Plus className="size-3.5" /> Add provider
          </button>
        }
      />
      <div className="space-y-3 p-4">
        <div className="grid gap-2 md:grid-cols-2 2xl:grid-cols-4">
          {providers.map((p) => (
            <Panel key={p.id} title={p.name} actions={<StatusText status={p.status} />}>
              <div className="space-y-px">
                <KV k="Protocol" v={p.protocol} />
                <KV k="Endpoints" v={p.endpoints.length} />
                <KV k="Active lanes" v={p.lanes} />
                <KV k="Latency p50" v={p.latency} />
                <KV k="Error rate" v={p.error} />
                <KV k="Volume" v={p.tokens} />
              </div>
              <div className="mt-2 border-t border-border pt-2">
                <div className="label-xs">Models</div>
                <div className="mt-1 flex flex-wrap gap-1">
                  {p.models.map((m) => (
                    <Tag key={m}>{m}</Tag>
                  ))}
                </div>
              </div>
              <div className="mt-2 border-t border-border pt-2">
                <div className="label-xs">Endpoints</div>
                <div className="mt-1 space-y-0.5">
                  {p.endpoints.map((e) => (
                    <div key={e} className="num truncate text-[11px] text-muted-foreground">
                      {e}
                    </div>
                  ))}
                </div>
              </div>
            </Panel>
          ))}
        </div>

        <Panel title="Capability matrix" dense>
          <TableShell head={["Capability", ...capabilityMatrix.columns]}>
            {capabilityMatrix.rows.map((r) => (
              <tr key={r.name} className="hover:bg-panel-raised/50">
                <Td>{r.name}</Td>
                {r.values.map((v, i) => (
                  <Td key={i} className={v ? "num text-ok" : "num text-muted-foreground"}>
                    {v ? "✓" : "—"}
                  </Td>
                ))}
              </tr>
            ))}
          </TableShell>
        </Panel>
      </div>
    </AppShell>
  );
}
