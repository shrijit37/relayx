import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel } from "@/components/relay/primitives";

export const Route = createFileRoute("/settings")({
  head: () => ({
    meta: [
      { title: "Settings — relay-x" },
      { name: "description", content: "Local development environment and keyboard shortcuts for the relay-x console." },
      { property: "og:title", content: "Settings — relay-x" },
      { property: "og:description", content: "Local development environment and keyboard shortcuts." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: SettingsPage,
});

const shortcuts: Array<[string, string]> = [
  ["⌘K", "Command palette"],
  ["⌘[", "Toggle sidebar"],
  ["⌘S", "Save workflow"],
  ["⌘⇧V", "Validate workflow"],
  ["⌘⇧P", "Publish workflow"],
  ["Shift+drag", "Multi-select"],
];

function SettingsPage() {
  return (
    <AppShell>
      <PageHeader title="Settings" subtitle="Local development state — no workspace/organization backend exists yet." />
      <div className="grid gap-3 p-4 xl:grid-cols-3">
        <Panel title="Environment">
          <KV k="Mode" v="local development" tone="warn" />
          <KV k="Control plane" v="POSTS to http://127.0.0.1:9091" />
          <KV k="Gateway admin" v="http://127.0.0.1:9090" />
          <KV k="Auth / session" v="none — no auth backend exists yet" />
          <KV k="Workspace / tenant" v="unavailable — no workspace backend exists yet" />
        </Panel>
        <Panel title="Telemetry">
          <KV k="Prompt capture" v="disabled" tone="ok" />
          <KV k="Metrics backend" v="not available — no telemetry backend yet" />
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