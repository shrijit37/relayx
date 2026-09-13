import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader } from "@/components/relay/primitives";

export const Route = createFileRoute("/policies")({
  head: () => ({
    meta: [
      { title: "Policies — relay-x" },
      { name: "description", content: "Deterministic ALLOW/DENY rules." },
      { property: "og:title", content: "Policies — relay-x" },
      { property: "og:description", content: "Deterministic ALLOW/DENY rules." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: PoliciesPage,
});

function PoliciesPage() {
  return (
    <AppShell>
      <PageHeader title="Policies" subtitle="Policies compile into a deterministic matcher — evaluation is explicit, DENY always wins." />
      <div className="p-4">
        <EmptyState
          title="Not available yet."
          body={
            "The policy engine and its compiled matcher are Phase 8 work; there is no policy " +
            "backend yet.\n\nThis view will become available when policy management is implemented."
          }
        />
      </div>
    </AppShell>
  );
}