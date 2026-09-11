import { createFileRoute } from "@tanstack/react-router";
import { KeyRound, Plus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { PageHeader, Panel, StatusText, TableShell, Td } from "@/components/relay/primitives";

export const Route = createFileRoute("/secrets")({
  head: () => ({
    meta: [
      { title: "Secrets — relay-x" },
      { name: "description", content: "Provider credentials and network identities, referenced by lanes and never materialised in the hot path." },
      { property: "og:title", content: "Secrets — relay-x" },
      { property: "og:description", content: "Scoped credentials with rotation status and last use." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: SecretsPage,
});

const secrets = [
  { name: "ANTHROPIC_API_KEY", scope: "workspace", used: "12s ago", rotated: "14 days ago", status: "healthy" },
  { name: "OPENAI_API_KEY", scope: "workspace", used: "18s ago", rotated: "9 days ago", status: "healthy" },
  { name: "MISTRAL_API_KEY", scope: "project", used: "3m ago", rotated: "96 days ago", status: "degraded" },
  { name: "WG_US01_PRIVATE_KEY", scope: "lane", used: "12s ago", rotated: "30 days ago", status: "healthy" },
  { name: "WG_EU03_PRIVATE_KEY", scope: "lane", used: "34s ago", rotated: "30 days ago", status: "healthy" },
];

function SecretsPage() {
  return (
    <AppShell>
      <PageHeader
        title="Secrets"
        subtitle="Referenced by handle at compile time; resolved once into the data plane, never read per request."
        actions={
          <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90">
            <Plus className="size-3.5" /> Add secret
          </button>
        }
      />
      <div className="p-4">
        <Panel title="Credentials" dense>
          <TableShell head={["Name", "Scope", "Last used", "Rotated", "Status"]}>
            {secrets.map((s) => (
              <tr key={s.name} className="hover:bg-panel-raised/50">
                <Td>
                  <span className="flex items-center gap-1.5">
                    <KeyRound className="size-3 text-muted-foreground" />
                    <span className="num">{s.name}</span>
                  </span>
                </Td>
                <Td className="num text-muted-foreground">{s.scope}</Td>
                <Td className="num text-muted-foreground">{s.used}</Td>
                <Td className="num text-muted-foreground">{s.rotated}</Td>
                <Td>
                  <StatusText status={s.status} />
                </Td>
              </tr>
            ))}
          </TableShell>
        </Panel>
      </div>
    </AppShell>
  );
}
