import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { KV, Metric, PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";

export const Route = createFileRoute("/health")({
  head: () => ({
    meta: [
      { title: "System health — relay-x" },
      { name: "description", content: "Control plane and Rust data plane health, plan propagation and regional node status." },
      { property: "og:title", content: "System health — relay-x" },
      { property: "og:description", content: "Control plane and data plane are separated; serving does not depend on authoring." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: HealthPage,
});

const nodes = [
  { node: "dp-use1-a", region: "us-east-1", plan: "v24", cpu: "38%", mem: "44%", streams: 62, status: "healthy" },
  { node: "dp-use1-b", region: "us-east-1", plan: "v24", cpu: "41%", mem: "46%", streams: 58, status: "healthy" },
  { node: "dp-euc1-a", region: "eu-central-1", plan: "v24", cpu: "29%", mem: "37%", streams: 18, status: "healthy" },
  { node: "dp-apse1-a", region: "ap-southeast-1", plan: "v23", cpu: "71%", mem: "68%", streams: 9, status: "degraded" },
];

function HealthPage() {
  return (
    <AppShell>
      <PageHeader title="Health" subtitle="Control plane (authoring, compilation) is isolated from the Rust data plane (serving)." />
      <div className="space-y-3 p-4">
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
          <Metric label="Data plane nodes" value="4" hint="3 healthy · 1 degraded" tone="warn" />
          <Metric label="Plan propagation" value="112" unit="ms" tone="ok" hint="last publish" />
          <Metric label="Hot-path DB reads" value="0" tone="ok" hint="per request" />
          <Metric label="Uptime" value="99.98" unit="%" tone="ok" hint="30 days" />
        </div>

        <div className="grid gap-3 xl:grid-cols-3">
          <Panel title="Data plane nodes" className="xl:col-span-2" dense>
            <TableShell head={["Node", "Region", "Plan", "CPU", "Memory", "Streams", "Status"]}>
              {nodes.map((n) => (
                <tr key={n.node} className="hover:bg-panel-raised/50">
                  <Td className="num">{n.node}</Td>
                  <Td className="num text-muted-foreground">{n.region}</Td>
                  <Td className="num">{n.plan}</Td>
                  <Td className="num">{n.cpu}</Td>
                  <Td className="num">{n.mem}</Td>
                  <Td className="num">{n.streams}</Td>
                  <Td>
                    <StatusText status={n.status} />
                  </Td>
                </tr>
              ))}
            </TableShell>
          </Panel>
          <Panel title="Control plane">
            <KV k="API" v="healthy" tone="ok" />
            <KV k="Compiler" v="healthy" tone="ok" />
            <KV k="Policy service" v="healthy" tone="ok" />
            <KV k="MCP indexer" v="degraded" tone="warn" />
            <KV k="Queue depth" v="3" />
            <KV k="Last compile" v="2 min ago" />
            <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground">
              A control plane outage does not interrupt serving: nodes continue on their last compiled plan.
            </p>
          </Panel>
        </div>
      </div>
    </AppShell>
  );
}
