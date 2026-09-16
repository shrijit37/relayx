import { createFileRoute, Link } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, KV, PageHeader, Panel, StatusText } from "@/components/relay/primitives";
import { duration } from "@/lib/format";
import { useRun, useWorkflows } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/runs/$runId")({
  head: () => ({
    meta: [
      { title: "Run detail — relay-x" },
      { name: "description", content: "Run detail and latency waterfall." },
      { property: "og:title", content: "Run detail — relay-x" },
      { property: "og:description", content: "Run detail and latency waterfall." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: RunDetail,
});

function RunDetail() {
  const { runId } = Route.useParams();
  const { data: run, isPending, isError, error } = useRun(runId);
  const { data: workflows } = useWorkflows();

  const workflowName = run
    ? workflows?.find((w) => w.id === run.workflow_id)?.name ?? run.workflow_id
    : "";

  return (
    <AppShell>
      <PageHeader
        title={isPending ? "Loading…" : run ? `Run ${runId.slice(0, 8)}` : "Run not found"}
        subtitle={run ? `${workflowName} v${run.workflow_version}` : "Run detail"}
        meta={
          <Link
            to="/runs"
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-primary"
          >
            <ArrowLeft className="size-3" /> all runs
          </Link>
        }
      />

      <div className="p-4">
        {isPending ? (
          <div className="text-xs text-muted-foreground">loading run…</div>
        ) : isError ? (
          <div className="text-xs text-fail">control plane unreachable — {String(error)}</div>
        ) : !run ? (
          <EmptyState title="Run not found." body={`No run record exists for ID ${runId}.`} />
        ) : (
          <div className="grid gap-3 xl:grid-cols-3">
            <Panel title="Run info" className="xl:col-span-2">
              <KV k="Run ID" v={run.id} />
              <KV k="Workflow" v={<Link to="/workflows/$workflowId" params={{ workflowId: run.workflow_id }} className="hover:text-primary">{workflowName}</Link>} />
              <KV k="Version" v={`v${run.workflow_version}`} />
              <KV k="Snapshot version" v={String(run.snapshot_version)} />
              <KV k="Plan hash" v={run.plan_hash?.slice(0, 16) ?? "—"} />
              <KV k="Status" v={<StatusText status={run.status} />} />
              <KV
                k="Duration"
                v={
                  run.completed_at
                    ? duration(run.started_at, run.completed_at)
                    : run.status === "running"
                      ? "in progress…"
                      : "—"
                }
              />
              <KV k="Started" v={new Date(run.started_at).toLocaleString()} />
              <KV k="Completed" v={run.completed_at ? new Date(run.completed_at).toLocaleString() : "—"} />
            </Panel>

            <Panel title="Timing">
              <KV k="Started at" v={new Date(run.started_at).toLocaleTimeString()} />
              <KV
                k="Completed at"
                v={run.completed_at ? new Date(run.completed_at).toLocaleTimeString() : "—"}
              />
              <KV
                k="Wall time"
                v={
                  run.completed_at
                    ? duration(run.started_at, run.completed_at)
                    : "—"
                }
              />
            </Panel>

            <Panel title="Input body" className="xl:col-span-2">
              <pre className="num max-h-[200px] overflow-auto whitespace-pre-wrap rounded-sm border border-border bg-canvas px-3 py-2 font-mono text-[11px]">
                {run.input_body ? JSON.stringify(run.input_body, null, 2) : "—"}
              </pre>
            </Panel>

            <Panel title="Output">
              <pre className="num max-h-[200px] overflow-auto whitespace-pre-wrap rounded-sm border border-border bg-canvas px-3 py-2 font-mono text-[11px]">
                {run.output ? JSON.stringify(run.output, null, 2) : "—"}
              </pre>
            </Panel>

            {run.error && (
              <Panel title="Error" className="xl:col-span-3">
                <div className="rounded-sm border border-fail/40 bg-fail/8 px-3 py-2 text-xs text-fail">
                  {run.error}
                </div>
              </Panel>
            )}
          </div>
        )}
      </div>
    </AppShell>
  );
}
