import { createFileRoute } from "@tanstack/react-router";
import { Check, Minus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, StatusText, Tag } from "@/components/relay/primitives";
import { skills } from "@/lib/relay-data";

export const Route = createFileRoute("/skills")({
  head: () => ({
    meta: [
      { title: "Agent Skills — relay-x" },
      { name: "description", content: "Skills load progressively: metadata, then instructions, with large references kept as deferred resources." },
      { property: "og:title", content: "Agent Skills — relay-x" },
      { property: "og:description", content: "Skills are instructional context, not executable tools." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: SkillsPage,
});

function Loaded({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="flex items-center justify-between py-[5px]">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className={ok ? "flex items-center gap-1 text-xs text-ok" : "flex items-center gap-1 text-xs text-muted-foreground"}>
        {ok ? <Check className="size-3" /> : <Minus className="size-3" />}
        {ok ? "loaded" : "not loaded"}
      </span>
    </div>
  );
}

function SkillsPage() {
  return (
    <AppShell>
      <PageHeader
        title="Skills"
        subtitle="Skills provide instructions and references. They are not executable tools and are never auto-invoked."
      />
      <div className="grid gap-2 p-4 md:grid-cols-2 2xl:grid-cols-4">
        {skills.map((s) => (
          <Panel key={s.name} title={s.name} actions={<StatusText status={s.status} />}>
            <div className="flex items-center gap-1.5">
              <Tag>v{s.version}</Tag>
              <Tag tone="info">skill</Tag>
            </div>
            <div className="mt-2 space-y-px border-t border-border pt-2">
              <Loaded ok={s.metadata} label="Metadata" />
              <Loaded ok={s.instructions} label="Instructions" />
              <KV k="References" v={`${s.references} available`} />
              <KV k="Scripts" v={`${s.scripts} available`} />
            </div>
            <div className="mt-2 border-t border-border pt-2">
              <KV k="Context cost" v={s.bytes} />
            </div>
            <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
              References are fetched on demand rather than dumped into the main context window.
            </p>
          </Panel>
        ))}
      </div>
    </AppShell>
  );
}
