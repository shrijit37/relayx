import { createFileRoute, Link } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { duration } from "@/lib/format";
import { useRuns, useWorkflows } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/runs/")({
  head: () => ({
    meta: [
      { title: "Runs — relay-x" },
      { name: "description", content: "Gateway run history and trace explorer." },
      { property: "og:title", content: "Runs — relay-x" },
      { property: "og:description", content: "Gateway run history and trace explorer." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: RunsPage,
});

function RunsPage() {
  const { data: runs, isPending, isError, error } = useRuns();
  const { data: workflows } = useWorkflows();

  const workflowName = (id: string) =>
    workflows?.find((w) => w.id === id)?.name ?? id;

  return (
    <AppShell>
      <PageHeader title="Runs" subtitle="Execution history from durable run records (populated by real workflow executions)." />
      <div className="p-4">
        {isPending ? (
          <div className="text-xs text-muted-foreground">loading runs…</div>
        ) : isError ? (
          <div className="text-xs text-fail">control plane unreachable — {String(error)}</div>
        ) : !runs || runs.length === 0 ? (
          <EmptyState
            title="No run history yet."
            body={
              "Run history is populated by real workflow executions. Execute a published workflow to create the first record."
            }
          />
        ) : (
          <div className="space-y-3">
            <TableShell head={["Run", "Workflow", "Version", "Status", "Started", "Duration"]}>
              {runs.map((r) => (
                <tr key={r.id} className="hover:bg-panel-raised/50">
                  <Td>
                    <Link
                      to="/runs/$runId"
                      params={{ runId: r.id }}
                      className="num font-medium hover:text-primary"
                    >
                      {r.id.slice(0, 8)}
                    </Link>
                  </Td>
                  <Td>
                    <Link
                      to="/workflows/$workflowId"
                      params={{ workflowId: r.workflow_id }}
                      className="hover:text-primary"
                    >
                      {workflowName(r.workflow_id)}
                    </Link>
                  </Td>
                  <Td className="num">v{r.workflow_version}</Td>
                  <Td>
                    <StatusText status={r.status} />
                  </Td>
                  <Td className="num text-muted-foreground">
                    {new Date(r.started_at).toLocaleString()}
                  </Td>
                  <Td className="num text-muted-foreground">
                    {r.completed_at
                      ? duration(r.started_at, r.completed_at)
                      : r.status === "running"
                        ? "in progress…"
                        : "—"}
                  </Td>
                </tr>
              ))}
            </TableShell>
          </div>
        )}
      </div>
    </AppShell>
  );
}
