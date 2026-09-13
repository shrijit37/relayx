import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, TableShell, Td } from "@/components/relay/primitives";
import { useCreateProviderMutation, useProviders } from "@/lib/use-workflow-publication";

export const Route = createFileRoute("/providers")({
  head: () => ({
    meta: [
      { title: "Providers — relay-x" },
      { name: "description", content: "Provider adapters persisted in the control plane." },
      { property: "og:title", content: "Providers — relay-x" },
      { property: "og:description", content: "Provider adapters persisted in the control plane." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: ProvidersPage,
});

const PROTOCOLS = ["openai_chat", "anthropic", "openai_responses"] as const;

function ProvidersPage() {
  const { data: providers, isPending, isError, error } = useProviders();
  const createProvider = useCreateProviderMutation();
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [protocol, setProtocol] = useState<string>(PROTOCOLS[0]);
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");

  const submit = () => {
    if (!name.trim() || !baseUrl.trim() || !model.trim()) return;
    createProvider.mutate(
      { name: name.trim(), protocol, base_url: baseUrl.trim(), model: model.trim() },
      {
        onSuccess: () => {
          setAdding(false);
          setName("");
          setBaseUrl("");
          setModel("");
        },
        onError: (err) => window.alert(`Create failed — ${err.message}`),
      },
    );
  };

  return (
    <AppShell>
      <PageHeader
        title="Providers"
        subtitle="Persisted provider config only. Health, latency and capability matrices are not reported — there is no provider-health backend yet."
        actions={
          <button
            onClick={() => setAdding((a) => !a)}
            className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90"
          >
            <Plus className="size-3.5" /> {adding ? "Cancel" : "Add provider"}
          </button>
        }
      />

      {adding && (
        <div className="border-b border-border bg-panel px-4 py-3">
          <div className="max-w-3xl space-y-2">
            <div className="grid gap-2 md:grid-cols-2">
              <label className="block">
                <span className="label-xs">Name</span>
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Protocol</span>
                <select
                  value={protocol}
                  onChange={(e) => setProtocol(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                >
                  {PROTOCOLS.map((p) => (
                    <option key={p} value={p}>
                      {p}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block">
                <span className="label-xs">Model</span>
                <input
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  placeholder="claude-sonnet-4-5"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Base URL</span>
                <input
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  placeholder="https://api.anthropic.com"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
            </div>
            <button
              onClick={submit}
              disabled={!name.trim() || !baseUrl.trim() || !model.trim()}
              className="focus-ring h-7 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90 disabled:opacity-40"
            >
              Create provider
            </button>
            {createProvider.isPending && <span className="num ml-2 text-[11px]">creating…</span>}
          </div>
        </div>
      )}

      <div className="space-y-3 p-4">
        <Panel title="Providers" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading providers…</div>
          ) : isError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(error)}</div>
          ) : providers && providers.length > 0 ? (
            <TableShell head={["Name", "Protocol", "Base URL", "Model", "Created"]}>
              {providers.map((p) => (
                <tr key={p.id} className="hover:bg-panel-raised/50">
                  <Td className="font-medium">{p.name}</Td>
                  <Td className="num text-muted-foreground">{p.protocol}</Td>
                  <Td className="num text-muted-foreground">{p.base_url}</Td>
                  <Td className="num">{p.model}</Td>
                  <Td className="num text-muted-foreground">{new Date(p.created_at).toLocaleDateString()}</Td>
                </tr>
              ))}
            </TableShell>
          ) : (
            <div className="p-4 text-xs text-muted-foreground">
              no providers persisted yet — add one to start routing.
            </div>
          )}
        </Panel>

        <Panel title="Capabilities" dense>
          <div className="p-3">
            <KV k="Protocol fidelity" v="handled in the Rust protocol adapters" />
            <KV k="Provider health" v="not available — no health-probe backend yet" />
            <KV k="Capability matrix" v="not available — no capability audit backend yet" />
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}