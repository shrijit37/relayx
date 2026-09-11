import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, Tag } from "@/components/relay/primitives";
import { workspace } from "@/lib/relay-data";

export const Route = createFileRoute("/settings")({
  head: () => ({
    meta: [
      { title: "Settings — relay-x" },
      { name: "description", content: "Workspace, environments, telemetry retention and keyboard shortcuts for the relay-x console." },
      { property: "og:title", content: "Settings — relay-x" },
      { property: "og:description", content: "Workspace, environments, retention and shortcuts." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: SettingsPage,
});

const shortcuts = [
  ["⌘K", "Command palette"],
  ["⌘[", "Toggle sidebar"],
  ["⌘↵", "Run test"],
  ["⌘S", "Save workflow"],
  ["⌘⇧V", "Validate workflow"],
  ["⌘⇧P", "Publish workflow"],
  ["⌘Z / ⌘⇧Z", "Undo / redo"],
  ["⌘C / ⌘V", "Copy / paste nodes"],
  ["Shift+drag", "Multi-select"],
  ["G", "Group selection"],
];

function SettingsPage() {
  return (
    <AppShell>
      <PageHeader title="Settings" subtitle="Workspace configuration and console preferences." />
      <div className="grid gap-3 p-4 xl:grid-cols-3">
        <Panel title="Workspace">
          <KV k="Name" v={workspace.name} />
          <KV k="Tenant id" v="tn_4f91c2" />
          <KV k="Region" v="us-east-1" />
          <KV k="Plan" v="Enterprise" />
        </Panel>
        <Panel title="Environments">
          <div className="flex flex-wrap gap-1">
            {workspace.environments.map((e) => (
              <Tag key={e} tone={e === "Production" ? "ok" : "neutral"}>
                {e}
              </Tag>
            ))}
          </div>
          <div className="mt-3 space-y-px">
            <KV k="Default" v="Production" />
            <KV k="Publish approval" v="required" />
            <KV k="Auto rollback" v="on error budget burn" />
          </div>
        </Panel>
        <Panel title="Telemetry">
          <KV k="Trace exporter" v="OTLP / gRPC" />
          <KV k="Prompt capture" v="disabled" tone="ok" />
          <KV k="Metric retention" v="30 days" />
          <KV k="Trace retention" v="7 days" />
          <KV k="Sampling" v="100% errors · 10% success" />
        </Panel>
        <Panel title="Keyboard shortcuts" className="xl:col-span-3">
          <div className="grid gap-x-6 gap-y-1 sm:grid-cols-2 lg:grid-cols-3">
            {shortcuts.map(([k, v]) => (
              <div key={k} className="flex items-center justify-between border-b border-border/60 py-1.5">
                <span className="text-xs text-muted-foreground">{v}</span>
                <kbd className="num rounded-[3px] border border-border bg-canvas px-1.5 py-0.5 text-[10px]">{k}</kbd>
              </div>
            ))}
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}
