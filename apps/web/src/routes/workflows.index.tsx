import { createFileRoute, Link } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";
import { workflows } from "@/lib/relay-data";

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
  return (
    <AppShell>
      <PageHeader
        title="Workflows"
        subtitle="Visual graphs are the authoring representation; runtime serves the compiled execution plan."
        actions={
          <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90">
            <Plus className="size-3.5" /> Create workflow
          </button>
        }
      />
      <div className="space-y-3 p-4">
        <Panel title="All workflows" dense>
          <TableShell head={["Workflow", "Version", "State", "Environment", "Owner", "Nodes", "p95", "rps", "Updated"]}>
            {workflows.map((w) => (
              <tr key={w.id} className="hover:bg-panel-raised/50">
                <Td>
                  <Link to="/workflows/$workflowId" params={{ workflowId: w.id }} className="font-medium hover:text-primary">
                    {w.name}
                  </Link>
                </Td>
                <Td className="num">v{w.version}</Td>
                <Td>
                  <StatusText status={w.state} />
                </Td>
                <Td className="num text-muted-foreground">{w.env}</Td>
                <Td className="num text-muted-foreground">{w.owner}</Td>
                <Td className="num">{w.nodes}</Td>
                <Td className="num">{w.p95}</Td>
                <Td className="num">{w.rps}</Td>
                <Td className="num text-muted-foreground">{w.updated}</Td>
              </tr>
            ))}
          </TableShell>
        </Panel>

        <Panel title="Development environment" dense>
          <div className="p-3">
            <EmptyState
              title="No workflows yet."
              body={"Build your first gateway workflow\nby connecting a route, lane and provider."}
              action={
                <button className="focus-ring inline-flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90">
                  <Plus className="size-3.5" /> Create workflow
                </button>
              }
            />
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}
