import { useState } from "react";
import { createFileRoute, Link } from "@tanstack/react-router";
import { Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { useDeleteWorkflowMutation, useWorkflows } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/workflows/")({
  head: () => ({
    meta: [
      { title: "Workflows — relay-x" },
      { name: "description", content: "Author, validate and deploy gateway workflows compiled into versioned execution plans." },
      { property: "og:title", content: "Workflows — relay-x" },
      { property: "og:description", content: "Author, validate and deploy gateway workflows compiled into versioned execution plans." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: WorkflowsPage,
});

function WorkflowsPage() {
  const { data: live, isPending, isError, error } = useWorkflows();
  const deleteWorkflow = useDeleteWorkflowMutation();
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const rows = live ?? [];

  const handleDelete = (id: string) => {
    deleteWorkflow.mutate(id, {
      onSuccess: () => {
        toast.success(`Workflow ${id} deleted`);
        setConfirmDelete(null);
      },
      onError: (err) => toast.error(`Delete failed — ${err.message}`),
    });
  };

  return (
    <AppShell>
      <PageHeader
        title="Workflows"
        subtitle="Visual graphs are the authoring representation; runtime serves the compiled execution plan."
        actions={
          <Link
            to="/workflows/$workflowId"
            params={{ workflowId: "new" }}
            className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90"
          >
            <Plus className="size-3.5" /> Create workflow
          </Link>
        }
      />
      <div className="space-y-3 p-4">
        <Panel title="All workflows" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading workflows…</div>
          ) : isError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(error)}</div>
          ) : rows.length === 0 ? (
            <div className="p-4 text-xs text-muted-foreground">
              no workflows persisted yet — create one to get started.
            </div>
          ) : (
            <TableShell head={["Workflow", "State", "Created", ""]}>
              {rows.map((w) => (
                <tr key={w.id} className="hover:bg-panel-raised/50">
                  <Td>
                    <Link to="/workflows/$workflowId" params={{ workflowId: w.id }} className="font-medium hover:text-primary">
                      {w.name}
                    </Link>
                    <span className="num ml-2 text-[10px] text-muted-foreground">{w.id}</span>
                  </Td>
                  <Td>
                    <StatusText status={w.status === "active" ? "Production" : w.status === "compiled" ? "Staging" : "Draft"} />
                  </Td>
                  <Td className="num text-muted-foreground">{new Date(w.created_at).toLocaleDateString()}</Td>
                  <Td className="text-right">
                    {
                      // Active (Production) workflows cannot be deleted — the
                      // gateway serves their published snapshot. No trash
                      // button (and the backend rejects with 409 anyway).
                      w.status === "active" ? (
                        <span className="cursor-not-allowed text-[10px] text-muted-foreground" title="Roll back to deactivate before deleting">
                          Locked
                        </span>
                      ) : confirmDelete === w.id ? (
                        <div className="flex items-center justify-end gap-1">
                          <button
                            onClick={() => handleDelete(w.id)}
                            disabled={deleteWorkflow.isPending}
                            className="focus-ring rounded-sm border border-fail/40 bg-fail/10 px-1.5 py-0.5 text-[10px] text-fail hover:bg-fail/20"
                          >
                            Confirm
                          </button>
                          <button
                            onClick={() => setConfirmDelete(null)}
                            className="focus-ring rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground hover:border-border-strong"
                          >
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <button
                          onClick={() => setConfirmDelete(w.id)}
                          disabled={deleteWorkflow.isPending}
                          className="focus-ring rounded-sm border border-border p-1 hover:border-fail hover:text-fail disabled:opacity-40"
                        >
                          <Trash2 className="size-3" />
                        </button>
                      )
                    }
                  </Td>
                </tr>
              ))}
            </TableShell>
          )}
        </Panel>

        <Panel title="Development environment" dense>
          <div className="p-3">
            <EmptyState
              title="Durable configuration, compiled once."
              body={"Workflows persist to PostgreSQL as immutable versions.\nValidate → compile → publish atomically."}
              action={
                <Link
                  to="/workflows/$workflowId"
                  params={{ workflowId: "new" }}
                  className="focus-ring inline-flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90"
                >
                  <Plus className="size-3.5" /> Create workflow
                </Link>
              }
            />
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}
