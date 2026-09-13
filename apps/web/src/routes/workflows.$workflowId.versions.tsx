import { createFileRoute, Link } from "@tanstack/react-router";
import { AlertTriangle, ArrowLeft, GitCompare, RotateCcw } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { useWorkflowVersions } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/workflows/$workflowId/versions")({
  head: () => ({
    meta: [
      { title: "Compilation & versions — relay-x" },
      { name: "description", content: "Versioned execution plans from a validated workflow graph." },
      { property: "og:title", content: "Compilation & versions — relay-x" },
      { property: "og:description", content: "Versioned, validated execution plans." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: VersionsPage,
});

function VersionsPage() {
  const { workflowId } = Route.useParams();
  const { data: liveVersions, isPending, isError, error } = useWorkflowVersions(workflowId);
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
        <Panel title="Lifecycle" className="xl:col-span-2">
          <div className="space-y-px">
            <KV k="Draft" v="editor state serialized" />
            <KV k="Validated / Compiled" v="gateway compile succeeds → deterministic plan hash" />
            <KV k="Active" v="published to the data plane (atomic snapshot swap)" />
          </div>
          <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground">
            The backend exposes one compile path for /validate and /compile; the version list below is
            backend truth. Detailed per-stage compilation timings are not retained by the backend.
          </p>
        </Panel>

        <div className="grid content-start gap-3">
          <Panel title="Plan artifact">
            <KV k="Plan hash" v={active?.plan_hash ?? "—"} />
            <KV k="Status" v={active?.status ?? "—"} />
            <KV k="Workflow" v={workflowId} />
          </Panel>
          <Panel title="Validation">
            {active ? (
              <div className="flex items-center gap-2 text-xs text-ok">
                <AlertTriangle className="size-3.5" />
                {active.plan_hash ? `Active version is compiled (plan ${shortHash(active.plan_hash)}).` : "Active version is published."}
              </div>
            ) : (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                No active version yet. When a version is compiled, its real plan hash is recorded here;
                per-issue validation details are not retained by the backend.
              </p>
            )}
          </Panel>
        </div>

        <Panel title="Version history" className="xl:col-span-3" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading versions…</div>
          ) : isError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(error)}</div>
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