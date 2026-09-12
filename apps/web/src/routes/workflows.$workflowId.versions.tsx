import { createFileRoute, Link } from "@tanstack/react-router";
import { AlertTriangle, ArrowLeft, Check, GitCompare, RotateCcw } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { compileStages, validationIssues } from "@/lib/relay-data";
import { useWorkflowVersions } from "@/lib/use-workflow-publication";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/workflows/$workflowId/versions")({
  head: () => ({
    meta: [
      { title: "Compilation & versions — relay-x" },
      { name: "description", content: "Compile a React Flow graph into a validated, policy-compiled, versioned execution plan." },
      { property: "og:title", content: "Compilation & versions — relay-x" },
      { property: "og:description", content: "Schema, semantic, capability, policy and lane validation before publish." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: VersionsPage,
});

function VersionsPage() {
  const { workflowId } = Route.useParams();
  const { data: liveVersions, isPending } = useWorkflowVersions(workflowId);
  const active = liveVersions?.find((v) => v.status === "active");

  return (
    <AppShell>
      <PageHeader
        title="Compilation & versions"
        subtitle={`${active ? `ACTIVE v${active.version} · plan ${shortHash(active.plan_hash)}` : "no live version yet"} · the visual graph is authored, the execution plan is served.`}
        meta={
          <Link
            to="/workflows/$workflowId"
            params={{ workflowId }}
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-primary"
          >
            <ArrowLeft className="size-3" /> back to editor
          </Link>
        }
        actions={
          <>
            <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong">
              <GitCompare className="size-3.5" /> Compare
            </button>
            <Link
              to="/workflows/$workflowId"
              params={{ workflowId }}
              className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong"
            >
              <RotateCcw className="size-3.5" /> Edit
            </Link>
          </>
        }
      />

      <div className="grid gap-3 p-4 xl:grid-cols-3">
        <Panel title="Compilation pipeline" className="xl:col-span-2">
          <ol className="space-y-0">
            {compileStages.map((s, i) => (
              <li key={s.name} className="flex items-start gap-3">
                <div className="flex flex-col items-center">
                  <span
                    className={cn(
                      "mt-1 grid size-4 place-items-center rounded-full border",
                      s.state === "warn" ? "border-warn text-warn" : "border-ok text-ok",
                    )}
                  >
                    {s.state === "warn" ? <AlertTriangle className="size-2.5" /> : <Check className="size-2.5" />}
                  </span>
                  {i < compileStages.length - 1 && <span className="my-0.5 h-6 w-px bg-border" />}
                </div>
                <div className="flex min-w-0 flex-1 items-baseline justify-between gap-3 pb-1">
                  <div className="min-w-0">
                    <div className="text-xs font-medium">{s.name}</div>
                    <div className="num truncate text-[11px] text-muted-foreground">{s.detail}</div>
                  </div>
                  <span className="num text-[11px] text-muted-foreground">{s.ms}</span>
                </div>
              </li>
            ))}
          </ol>
        </Panel>

        <div className="grid content-start gap-3">
          <Panel title="Plan artifact">
            <KV k="Plan hash" v={active?.plan_hash ?? "—"} />
            <KV k="Snapshot version" v={String(active?.version ?? "—")} />
            <KV k="Workflow" v={workflowId} />
          </Panel>
          <Panel title="Validation">
            <ul className="space-y-2">
              {validationIssues.map((v) => (
                <li key={v.code} className="flex gap-2">
                  <AlertTriangle className={cn("mt-px size-3.5 shrink-0", v.level === "error" ? "text-fail" : "text-warn")} />
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs font-medium">{v.title}</span>
                      <span className="num text-[10px] text-muted-foreground">{v.code}</span>
                    </div>
                    <p className="text-[11px] leading-relaxed text-muted-foreground">{v.detail}</p>
                    <span className="num text-[10px] text-muted-foreground">on {v.node}</span>
                  </div>
                </li>
              ))}
            </ul>
          </Panel>
        </div>

        <Panel title="Version history" className="xl:col-span-3" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading versions…</div>
          ) : liveVersions && liveVersions.length > 0 ? (
            <TableShell head={["Version", "Status", "Plan", "When", ""]}>
              {liveVersions.map((v) => (
                <tr key={v.version} className="hover:bg-panel-raised/50">
                  <Td className="num font-medium">v{v.version}</Td>
                  <Td>
                    <StatusText status={v.status === "active" ? "Production" : v.status === "compiled" ? "Staging" : "Draft"} />
                  </Td>
                  <Td className="num text-muted-foreground">{shortHash(v.plan_hash)}</Td>
                  <Td className="num text-muted-foreground">{new Date(v.created_at).toLocaleString()}</Td>
                  <Td className="text-right">
                    <button className="focus-ring rounded-sm border border-border px-1.5 py-0.5 text-[10px] hover:border-primary hover:text-primary">
                      {v.status === "active" ? "Active" : "Draft"}
                    </button>
                  </Td>
                </tr>
              ))}
            </TableShell>
          ) : (
            <div className="p-4 text-xs text-muted-foreground">no versions persisted yet — open the editor and publish.</div>
          )}
        </Panel>
      </div>
    </AppShell>
  );
}

function shortHash(h: string | null | undefined): string {
  if (!h) return "—";
  return h.length > 12 ? `${h.slice(0, 12)}…` : h;
}
