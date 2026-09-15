import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, TableShell, Td } from "@/components/relay/primitives";
import {
  useCreateProviderMutation,
  useDeleteProviderMutation,
  useProviders,
  useUpdateProviderMutation,
} from "@/lib/use-workflow-publication";

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
  const updateProvider = useUpdateProviderMutation();
  const deleteProvider = useDeleteProviderMutation();

  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [protocol, setProtocol] = useState<string>(PROTOCOLS[0]);
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");

  const resetForm = () => {
    setName("");
    setProtocol(PROTOCOLS[0]);
    setBaseUrl("");
    setModel("");
  };

  const openAdd = () => {
    resetForm();
    setEditingId(null);
    setAdding(true);
  };

  const openEdit = (p: { id: string; name: string; protocol: string; base_url: string; model: string }) => {
    setName(p.name);
    setProtocol(p.protocol);
    setBaseUrl(p.base_url);
    setModel(p.model);
    setEditingId(p.id);
    setAdding(false);
  };

  const submitCreate = () => {
    if (!name.trim() || !baseUrl.trim() || !model.trim()) return;
    createProvider.mutate(
      { name: name.trim(), protocol, base_url: baseUrl.trim(), model: model.trim() },
      {
        onSuccess: () => {
          toast.success(`Provider ${name.trim()} created`);
          setAdding(false);
          resetForm();
        },
        onError: (err) => toast.error(`Create failed — ${err.message}`),
      },
    );
  };

  const submitEdit = () => {
    if (!editingId || !name.trim() || !baseUrl.trim() || !model.trim()) return;
    updateProvider.mutate(
      {
        id: editingId,
        input: {
          name: name.trim(),
          protocol,
          base_url: baseUrl.trim(),
          model: model.trim(),
        },
      },
      {
        onSuccess: () => {
          toast.success(`Provider ${editingId} updated`);
          setEditingId(null);
          resetForm();
        },
        onError: (err) => toast.error(`Update failed — ${err.message}`),
      },
    );
  };

  const handleDelete = (id: string) => {
    deleteProvider.mutate(id, {
      onSuccess: () => {
        toast.success(`Provider ${id} deleted`);
        setConfirmDelete(null);
      },
      onError: (err) => toast.error(`Delete failed — ${err.message}`),
    });
  };

  const isWorking = createProvider.isPending || updateProvider.isPending || deleteProvider.isPending;

  return (
    <AppShell>
      <PageHeader
        title="Providers"
        subtitle="Persisted provider config only. Health, latency and capability matrices are not reported — there is no provider-health backend yet."
        actions={
          <button
            onClick={() => (adding ? setAdding(false) : openAdd())}
            disabled={isWorking}
            className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90 disabled:opacity-40"
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
              onClick={submitCreate}
              disabled={!name.trim() || !baseUrl.trim() || !model.trim() || isWorking}
              className="focus-ring h-7 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90 disabled:opacity-40"
            >
              Create provider
            </button>
            {createProvider.isPending && <span className="num ml-2 text-[11px]">creating…</span>}
          </div>
        </div>
      )}

      {editingId && (
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
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Base URL</span>
                <input
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={submitEdit}
                disabled={!name.trim() || !baseUrl.trim() || !model.trim() || isWorking}
                className="focus-ring h-7 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90 disabled:opacity-40"
              >
                Save changes
              </button>
              <button
                onClick={() => { setEditingId(null); resetForm(); }}
                className="focus-ring h-7 rounded-sm border border-border px-2.5 text-xs text-muted-foreground hover:border-border-strong"
              >
                Cancel
              </button>
              {updateProvider.isPending && <span className="num text-[11px]">saving…</span>}
            </div>
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
            <TableShell head={["Name", "Protocol", "Base URL", "Model", "Created", ""]}>
              {providers.map((p) => (
                <tr key={p.id} className="hover:bg-panel-raised/50">
                  <Td className="font-medium">{p.name}</Td>
                  <Td className="num text-muted-foreground">{p.protocol}</Td>
                  <Td className="num text-muted-foreground">{p.base_url}</Td>
                  <Td className="num">{p.model}</Td>
                  <Td className="num text-muted-foreground">{new Date(p.created_at).toLocaleDateString()}</Td>
                  <Td className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      <button
                        onClick={() => openEdit(p)}
                        disabled={isWorking}
                        className="focus-ring rounded-sm border border-border p-1 hover:border-primary hover:text-primary disabled:opacity-40"
                      >
                        <Pencil className="size-3" />
                      </button>
                      {confirmDelete === p.id ? (
                        <div className="flex items-center gap-1">
                          <button
                            onClick={() => handleDelete(p.id)}
                            disabled={isWorking}
                            className="focus-ring rounded-sm border border-fail/40 bg-fail/10 px-1.5 py-0.5 text-[10px] text-fail hover:bg-fail/20"
                          >
                            Confirm
                          </button>
                          <button
                            onClick={() => setConfirmDelete(null)}
                            className="focus-ring rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground hover:border-border-strong"
                          >
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <button
                          onClick={() => setConfirmDelete(p.id)}
                          disabled={isWorking}
                          className="focus-ring rounded-sm border border-border p-1 hover:border-fail hover:text-fail disabled:opacity-40"
                        >
                          <Trash2 className="size-3" />
                        </button>
                      )}
                    </div>
                  </Td>
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
