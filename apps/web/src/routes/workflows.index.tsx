import { useState } from "react";
import { createFileRoute, Link } from "@tanstack/react-router";
import { Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { useDeleteWorkflowMutation, useDeactivateWorkflowMutation, useRenameWorkflowMutation, useWorkflows } from "@/lib/use-workflow-publication";

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
  const deactivateWorkflow = useDeactivateWorkflowMutation();
  const renameWorkflow = useRenameWorkflowMutation();
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");

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

  const submitRename = (id: string) => {
    const name = renameDraft.trim();
    if (!name) return;
    renameWorkflow.mutate(
      { id, name },
      {
        onSuccess: () => {
          toast.success(`Workflow renamed`);
          setRenamingId(null);
        },
        onError: (err) => toast.error(`Rename failed — ${err.message}`),
      },
    );
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
                    {renamingId === w.id ? (
                      <span className="flex items-center gap-1">
                        <input
                          value={renameDraft}
                          onChange={(e) => setRenameDraft(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") submitRename(w.id);
                            if (e.key === "Escape") setRenamingId(null);
                          }}
                          autoFocus
                          className="focus-ring h-6 w-40 rounded-sm border border-border bg-canvas px-1.5 text-xs outline-none focus:border-primary"
                        />
                        <button
                          onClick={() => submitRename(w.id)}
                          disabled={renameWorkflow.isPending || !renameDraft.trim()}
                          className="focus-ring rounded-sm border border-border px-1.5 py-0.5 text-[10px] hover:border-border-strong disabled:opacity-40"
                        >
                          Save
                        </button>
                        <button
                          onClick={() => setRenamingId(null)}
                          className="focus-ring rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground hover:border-border-strong"
                        >
                          Cancel
                        </button>
                      </span>
                    ) : (
                      <>
                        <Link to="/workflows/$workflowId" params={{ workflowId: w.id }} className="font-medium hover:text-primary">
                          {w.name}
                        </Link>
                        <button
                          onClick={() => {
                            setRenameDraft(w.name);
                            setRenamingId(w.id);
                          }}
                          className="focus-ring ml-1.5 rounded-sm px-1 text-[10px] text-muted-foreground hover:text-foreground"
                          title="Rename workflow"
                        >
                          Rename
                        </button>
                      </>
                    )}
                    <span className="num ml-2 text-[10px] text-muted-foreground">{w.id}</span>
                  </Td>
                  <Td>
                    <StatusText status={w.is_active ? "Production" : w.status === "compiled" ? "Staging" : "Draft"} />
                  </Td>
                  <Td className="num text-muted-foreground">{new Date(w.created_at).toLocaleDateString()}</Td>
                  <Td className="text-right">
                    {
                      // Active (Production) workflows cannot be deleted — the
                      // gateway serves their published snapshot. Deactivate first.
                      w.is_active ? (
                        <button
                          onClick={() => {
                            deactivateWorkflow.mutate(w.id, {
                              onSuccess: () => toast.success(`Workflow ${w.id} deactivated`),
                              onError: (err) => toast.error(`Deactivate failed — ${err.message}`),
                            });
                          }}
                          disabled={deactivateWorkflow.isPending}
                          className="focus-ring rounded-sm border border-warn/40 bg-warn/10 px-1.5 py-0.5 text-[10px] text-warn hover:bg-warn/20"
                          title="Deactivate this workflow before deleting"
                        >
                          Deactivate
                        </button>
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
