import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { EmptyState, PageHeader } from "@/components/relay/primitives";

export const Route = createFileRoute("/secrets")({
  head: () => ({
    meta: [
      { title: "Secrets — relay-x" },
      { name: "description", content: "Provider credentials and network identities." },
      { property: "og:title", content: "Secrets — relay-x" },
      { property: "og:description", content: "Provider credentials and network identities." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: SecretsPage,
});

function SecretsPage() {
  return (
    <AppShell>
      <PageHeader title="Secrets" subtitle="Credentials are referenced by handle; raw values never reach the frontend." />
      <div className="p-4">
        <EmptyState
          title="Not available yet."
          body={
            "Credential resolution and rotation tracking are Phase 8 work (secret-manager " +
            "integration); credential_refs exist in solver/snapshot code but there is no " +
            "secret-management backend yet.\n\nThis view will become available when secret " +
            "management is implemented."
          }
        />
      </div>
    </AppShell>
  );
}