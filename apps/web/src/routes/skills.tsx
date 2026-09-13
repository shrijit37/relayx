import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader } from "@/components/relay/primitives";

export const Route = createFileRoute("/skills")({
  head: () => ({
    meta: [
      { title: "Agent Skills — relay-x" },
      { name: "description", content: "Skill registry and progressive loading." },
      { property: "og:title", content: "Agent Skills — relay-x" },
      { property: "og:description", content: "Skill registry and progressive loading." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: SkillsPage,
});

function SkillsPage() {
  return (
    <AppShell>
      <PageHeader title="Skills" subtitle="Skills provide instructions and references. They are never auto-invoked." />
      <div className="p-4">
        <EmptyState
          title="Not available yet."
          body={
            "The Skill registry and progressive-loading runtime are Phase 7 work.\n\n" +
            "This view will become available when the Skill backend is implemented. Skill nodes " +
            "on the canvas remain display-only until then."
          }
        />
      </div>
    </AppShell>
  );
}