import { createFileRoute } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, Tag } from "@/components/relay/primitives";
import { policies } from "@/lib/relay-data";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/policies")({
  head: () => ({
    meta: [
      { title: "Policies — relay-x" },
      { name: "description", content: "Deterministic ALLOW/DENY rules scoped by tenant, project, workflow, environment, MCP server, tool and endpoint." },
      { property: "og:title", content: "Policies — relay-x" },
      { property: "og:description", content: "Policies compile into a deterministic matcher evaluated on the hot path." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: PoliciesPage,
});

const scopes = ["tenant", "project", "workflow", "environment", "MCP server", "tool", "endpoint"];

function PoliciesPage() {
  return (
    <AppShell>
      <PageHeader
        title="Policies"
        subtitle="Compiled into a deterministic matcher — evaluation order is explicit, DENY always wins."
        actions={
          <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90">
            <Plus className="size-3.5" /> New rule
          </button>
        }
      />
      <div className="grid gap-3 p-4 xl:grid-cols-3">
        <div className="space-y-2 xl:col-span-2">
          {policies.map((p) => (
            <Panel
              key={p.id}
              title={p.name}
              actions={
                <>
                  <Tag tone={p.effect === "ALLOW" ? "ok" : "fail"}>{p.effect}</Tag>
                  <Tag>{p.scope}</Tag>
                </>
              }
            >
              <div className="rounded-sm border border-border bg-canvas p-2">
                <div className={cn("num text-[11px] font-medium", p.effect === "ALLOW" ? "text-ok" : "text-fail")}>
                  {p.effect}
                </div>
                <ul className="mt-1 space-y-0.5">
                  {p.conditions.map((c, i) => (
                    <li key={c} className="num text-[11px]">
                      {i > 0 && <span className="mr-2 text-muted-foreground">AND</span>}
                      {c}
                    </li>
                  ))}
                </ul>
              </div>
              <div className="mt-2 flex items-center justify-between">
                <span className="num text-[11px] text-muted-foreground">{p.id}</span>
                <span className="num text-[11px] text-muted-foreground">
                  {p.hits} · updated {p.updated}
                </span>
              </div>
            </Panel>
          ))}
        </div>

        <div className="grid content-start gap-3">
          <Panel title="Rule builder">
            <div className="space-y-2">
              <div className="flex gap-1.5">
                <button className="focus-ring h-7 flex-1 rounded-sm border border-ok/40 bg-ok/10 text-xs text-ok">ALLOW</button>
                <button className="focus-ring h-7 flex-1 rounded-sm border border-border text-xs text-muted-foreground hover:border-fail/40 hover:text-fail">
                  DENY
                </button>
              </div>
              {[
                ["Environment", "= production"],
                ["Workflow", "= customer-support"],
                ["Tool", "= github.read_pr"],
              ].map(([field, op]) => (
                <div key={field} className="grid grid-cols-[104px_1fr] gap-1.5">
                  <div className="num flex h-7 items-center rounded-sm border border-border bg-canvas px-2 text-[11px]">
                    {field}
                  </div>
                  <div className="num flex h-7 items-center rounded-sm border border-border bg-canvas px-2 text-[11px]">
                    {op}
                  </div>
                </div>
              ))}
              <button className="focus-ring h-7 w-full rounded-sm border border-dashed border-border text-[11px] text-muted-foreground hover:border-primary hover:text-primary">
                + Add condition
              </button>
            </div>
          </Panel>
          <Panel title="Scopes">
            <div className="flex flex-wrap gap-1">
              {scopes.map((s) => (
                <Tag key={s}>{s}</Tag>
              ))}
            </div>
          </Panel>
          <Panel title="Evaluation">
            <KV k="Compiled rules" v="9" />
            <KV k="Matcher" v="deterministic" />
            <KV k="Evaluation cost" v="0.1 ms" tone="ok" />
            <KV k="Denials / 24h" v="339" />
            <KV k="Conflicts" v="0" tone="ok" />
          </Panel>
        </div>
      </div>
    </AppShell>
  );
}
