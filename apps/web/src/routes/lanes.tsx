import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, TableShell, Td } from "@/components/relay/primitives";
import { useLanes } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/lanes")({
  head: () => ({
    meta: [
      { title: "Lanes — relay-x" },
      { name: "description", content: "Network lanes persisted in the control plane." },
      { property: "og:title", content: "Lanes — relay-x" },
      { property: "og:description", content: "Network lanes persisted in the control plane." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: LanesPage,
});

function LanesPage() {
  const { data: lanes, isPending, isError, error } = useLanes();

  return (
    <AppShell>
      <PageHeader
        title="Lanes"
        subtitle="Persisted lane config only. Topology, health, latency and WireGuard state are not reported — there is no lane-health backend yet."
      />
      <div className="space-y-3 p-4">
        <Panel title="Lanes" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading lanes…</div>
          ) : isError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(error)}</div>
          ) : lanes && lanes.length > 0 ? (
            <TableShell head={["Lane", "Endpoint", "Base URL", "Egress", "Policies"]}>
              {lanes.map((l) => (
                <tr key={l.id} className="hover:bg-panel-raised/50">
                  <Td className="num font-medium">{l.id}</Td>
                  <Td className="num text-muted-foreground">{l.endpoint}</Td>
                  <Td className="num text-muted-foreground">{l.base_url}</Td>
                  <Td className="num text-muted-foreground">{l.egress}</Td>
                  <Td className="num text-muted-foreground">
                    {l.policies.length > 0 ? l.policies.join(", ") : "—"}
                  </Td>
                </tr>
              ))}
            </TableShell>
          ) : (
            <div className="p-4 text-xs text-muted-foreground">
              no lanes persisted yet — create one from the control plane or a workflow lane node.
            </div>
          )}
        </Panel>

        <Panel title="Runtime network state" dense>
          <div className="p-3">
            <KV k="Connection pools" v="per-lane, rebuilt on each publish (atomic)" />
            <KV k="WireGuard" v="not available — no lane-network backend yet" />
            <KV k="Health probes" v="not available — no lane-health backend yet" />
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}