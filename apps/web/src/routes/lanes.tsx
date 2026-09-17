import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell } from "@/components/relay/AppShell";
import { KV, PageHeader, Panel, TableShell, Td } from "@/components/relay/primitives";
import {
  useCreateLaneMutation,
  useDeleteLaneMutation,
  useLanes,
  useProviders,
  useUpdateLaneMutation,
} from "@/lib/use-workflow-publication";
import { DEFAULT_PROJECT, type LaneRow } from "@/lib/api";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/lanes")({
  head: () => ({
    meta: [
      { title: "Lanes — relay-x" },
      { name: "description", content: "Network lanes persisted in the control plane." },
      { property: "og:title", content: "Lanes — relay-x" },
      { property: "og:description", content: "Network lanes persisted in the control plane." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: LanesPage,
});

function LanesPage() {
  const { data: lanes, isPending, isError, error } = useLanes();
  const { data: providers } = useProviders();
  const createLane = useCreateLaneMutation();
  const updateLane = useUpdateLaneMutation();
  const deleteLane = useDeleteLaneMutation();

  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  // Form state for create + edit (shared shape).
  const [formId, setFormId] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [egress, setEgress] = useState<"direct" | "masked">("direct");
  const [proxyUrl, setProxyUrl] = useState("");
  const [providerId, setProviderId] = useState("");
  const [credentialRef, setCredentialRef] = useState("");
  const [policies, setPolicies] = useState("");

  const resetForm = () => {
    setFormId("");
    setEndpoint("");
    setBaseUrl("");
    setEgress("direct");
    setProxyUrl("");
    setProviderId("");
    setCredentialRef("");
    setPolicies("");
  };

  const openAdd = () => {
    resetForm();
    setEditingId(null);
    setAdding(true);
  };

  const openEdit = (l: LaneRow) => {
    setFormId(l.id);
    setEndpoint(l.endpoint);
    setBaseUrl(l.base_url);
    setEgress(l.egress === "masked" ? "masked" : "direct");
    setProxyUrl(l.proxy_url ?? "");
    setProviderId(l.provider_id ?? "");
    setCredentialRef(l.credential_ref?.ref ?? "");
    setPolicies(l.policies.join(", "));
    setEditingId(l.id);
    setAdding(false);
  };

  const submitCreate = () => {
    if (!baseUrl.trim()) return;
    // Masked without a proxy is rejected client-side (mirrors the backend 400).
    if (egress === "masked" && !proxyUrl.trim()) return;
    const input: {
      id?: string;
      name: string;
      project_id: string;
      endpoint?: string;
      base_url: string;
      egress: "direct" | "masked";
      proxy_url?: string | null;
      policies?: string[];
      provider_id?: string | null;
      credential_ref?: { ref: string; provider: string } | null;
    } = {
      name: formId.trim() || (endpoint.trim() || baseUrl.trim()),
      project_id: DEFAULT_PROJECT,
      base_url: baseUrl.trim(),
      egress: egress === "masked" ? "masked" : "direct",
    };
    if (formId.trim()) input.id = formId.trim();
    if (endpoint.trim()) input.endpoint = endpoint.trim();
    if (egress === "masked") input.proxy_url = proxyUrl.trim();
    if (providerId.trim()) input.provider_id = providerId.trim();
    if (credentialRef.trim()) input.credential_ref = { ref: credentialRef.trim(), provider: "env" };
    const parsed = policies.split(",").map((p) => p.trim()).filter(Boolean);
    if (parsed.length > 0) input.policies = parsed;
    if (input.egress !== "masked") input.proxy_url = null;
    createLane.mutate(input, {
      onSuccess: () => {
        toast.success(`Lane ${formId || endpoint || baseUrl} created`);
        setAdding(false);
        resetForm();
      },
      onError: (err) => toast.error(`Create failed — ${err.message}`),
    });
  };

  const submitEdit = () => {
    if (!editingId || !baseUrl.trim()) return;
    if (egress === "masked" && !proxyUrl.trim()) return;
    updateLane.mutate(
      {
        id: editingId,
        input: {
          base_url: baseUrl.trim(),
          egress: egress === "masked" ? "masked" : "direct",
          ...(endpoint.trim() ? { endpoint: endpoint.trim() } : {}),
          ...(egress === "masked" ? { proxy_url: proxyUrl.trim() } : { proxy_url: null }),
          ...(providerId.trim() ? { provider_id: providerId.trim() } : { provider_id: null }),
          ...(credentialRef.trim()
            ? { credential_ref: { ref: credentialRef.trim(), provider: "env" as const } }
            : { credential_ref: null }),
          policies: policies.split(",").map((p) => p.trim()).filter(Boolean),
        },
      },
      {
        onSuccess: () => {
          toast.success(`Lane ${editingId} updated`);
          setEditingId(null);
          resetForm();
        },
        onError: (err) => toast.error(`Update failed — ${err.message}`),
      },
    );
  };

  const handleDelete = (id: string) => {
    deleteLane.mutate(id, {
      onSuccess: () => {
        toast.success(`Lane ${id} deleted`);
        setConfirmDelete(null);
      },
      onError: (err) => toast.error(`Delete failed — ${err.message}`),
    });
  };

  const isWorking = createLane.isPending || updateLane.isPending || deleteLane.isPending;

  return (
    <AppShell>
      <PageHeader
        title="Lanes"
        subtitle="Persisted lane config only. Topology, health, latency and WireGuard state are not reported — there is no lane-health backend yet."
        actions={
          <button
            onClick={() => (adding ? setAdding(false) : openAdd())}
            disabled={isWorking}
            className="focus-ring flex h-7 items-center gap-1.5 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90 disabled:opacity-40"
          >
            <Plus className="size-3.5" /> {adding ? "Cancel" : "Add lane"}
          </button>
        }
      />

      {adding && (
        <div className="border-b border-border bg-panel px-4 py-3">
          <div className="max-w-3xl space-y-2">
            <div className="grid gap-2 md:grid-cols-2">
              <label className="block">
                <span className="label-xs">Lane ID (optional)</span>
                <input
                  value={formId}
                  onChange={(e) => setFormId(e.target.value)}
                  placeholder="auto-generated if empty"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Egress</span>
                <select
                  value={egress}
                  onChange={(e) => setEgress(e.target.value as "direct" | "masked")}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                >
                  <option value="direct">direct (gateway IP)</option>
                  <option value="masked">masked (via proxy)</option>
                </select>
              </label>
              {egress === "masked" && (
                <label className="block">
                  <span className="label-xs">Proxy URL (required for masked)</span>
                  <input
                    value={proxyUrl}
                    onChange={(e) => setProxyUrl(e.target.value)}
                    placeholder="http://proxy:8080 or socks5://proxy:1080"
                    className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                  />
                </label>
              )}
              <label className="block md:col-span-2">
                <span className="label-xs">Endpoint (optional — informational)</span>
                <input
                  value={endpoint}
                  onChange={(e) => setEndpoint(e.target.value)}
                  placeholder="api.anthropic.com"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
                <span className="mt-0.5 block text-[10px] text-muted-foreground">
                  Display only — the runtime forwards to the base URL.
                </span>
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
              <label className="block">
                <span className="label-xs">Provider</span>
                <select
                  value={providerId}
                  onChange={(e) => setProviderId(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                >
                  <option value="">none</option>
                  {providers?.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.protocol})
                    </option>
                  ))}
                </select>
              </label>
              <label className="block">
                <span className="label-xs">Credential ref (env var name)</span>
                <input
                  value={credentialRef}
                  onChange={(e) => setCredentialRef(e.target.value)}
                  placeholder="RELAYX_ANTHROPIC_KEY"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
                <span className="mt-0.5 block text-[10px] text-muted-foreground">
                  Resolved to an Authorization header at publish; raw values never
                  reach the frontend (see Secrets).
                </span>
              </label>
              <label className="block md:col-span-2">
                <span className="label-xs">Policies (comma-separated)</span>
                <input
                  value={policies}
                  onChange={(e) => setPolicies(e.target.value)}
                  placeholder="e.g. retry,timeout"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
                <span className="mt-0.5 block text-[10px] text-muted-foreground">
                  Stored but not enforced by the runtime yet.
                </span>
              </label>
            </div>
            <button
              onClick={submitCreate}
              disabled={
                !baseUrl.trim() ||
                (egress === "masked" && !proxyUrl.trim()) ||
                isWorking
              }
              className="focus-ring h-7 rounded-sm bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:opacity-90 disabled:opacity-40"
            >
              Create lane
            </button>
            {createLane.isPending && <span className="num ml-2 text-[11px]">creating…</span>}
          </div>
        </div>
      )}

      {editingId && (
        <div className="border-b border-border bg-panel px-4 py-3">
          <div className="max-w-3xl space-y-2">
            <div className="grid gap-2 md:grid-cols-2">
              <label className="block">
                <span className="label-xs">Base URL</span>
                <input
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Egress</span>
                <select
                  value={egress}
                  onChange={(e) => setEgress(e.target.value as "direct" | "masked")}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                >
                  <option value="direct">direct (gateway IP)</option>
                  <option value="masked">masked (via proxy)</option>
                </select>
              </label>
              {egress === "masked" && (
                <label className="block">
                  <span className="label-xs">Proxy URL (required for masked)</span>
                  <input
                    value={proxyUrl}
                    onChange={(e) => setProxyUrl(e.target.value)}
                    placeholder="http://proxy:8080 or socks5://proxy:1080"
                    className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                  />
                </label>
              )}
              <label className="block">
                <span className="label-xs">Endpoint (optional — informational)</span>
                <input
                  value={endpoint}
                  onChange={(e) => setEndpoint(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Provider</span>
                <select
                  value={providerId}
                  onChange={(e) => setProviderId(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                >
                  <option value="">none</option>
                  {providers?.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.protocol})
                    </option>
                  ))}
                </select>
              </label>
              <label className="block">
                <span className="label-xs">Credential ref (env var name)</span>
                <input
                  value={credentialRef}
                  onChange={(e) => setCredentialRef(e.target.value)}
                  placeholder="RELAYX_ANTHROPIC_KEY"
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
              <label className="block">
                <span className="label-xs">Policies (comma-separated)</span>
                <input
                  value={policies}
                  onChange={(e) => setPolicies(e.target.value)}
                  className="mt-1 h-8 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none focus:border-primary"
                />
              </label>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={submitEdit}
                disabled={
                  !baseUrl.trim() ||
                  (egress === "masked" && !proxyUrl.trim()) ||
                  isWorking
                }
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
              {updateLane.isPending && <span className="num text-[11px]">saving…</span>}
            </div>
          </div>
        </div>
      )}

      <div className="space-y-3 p-4">
        <Panel title="Lanes" dense>
          {isPending ? (
            <div className="p-4 text-xs text-muted-foreground">loading lanes…</div>
          ) : isError ? (
            <div className="p-4 text-xs text-fail">control plane unreachable — {String(error)}</div>
          ) : lanes && lanes.length > 0 ? (
            <TableShell head={["Lane", "Base URL", "Egress", "Proxy", "Credentials", "Policies", ""]}>
              {lanes.map((l) => (
                <tr key={l.id} className="hover:bg-panel-raised/50">
                  <Td className="num font-medium">{l.id}</Td>
                  <Td className="num text-muted-foreground">{l.base_url}</Td>
                  <Td className="num text-muted-foreground">{l.egress}</Td>
                  <Td className="num text-muted-foreground">{l.proxy_url ?? "—"}</Td>
                  <Td className="num text-muted-foreground">
                    {l.credential_ref ? l.credential_ref.ref : "—"}
                  </Td>
                  <Td className="num text-muted-foreground">
                    {l.policies.length > 0 ? l.policies.join(", ") : "—"}
                  </Td>
                  <Td className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      <button
                        onClick={() => openEdit(l)}
                        disabled={isWorking}
                        className="focus-ring rounded-sm border border-border p-1 hover:border-primary hover:text-primary disabled:opacity-40"
                      >
                        <Pencil className="size-3" />
                      </button>
                      {confirmDelete === l.id ? (
                        <div className="flex items-center gap-1">
                          <button
                            onClick={() => handleDelete(l.id)}
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
                          onClick={() => setConfirmDelete(l.id)}
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
              no lanes persisted yet — create one to start routing.
            </div>
          )}
        </Panel>

        <Panel title="Runtime network state" dense>
          <div className="p-3">
            <KV k="Connection pools" v="per-lane, rebuilt on each publish (atomic)" />
            <KV k="WireGuard" v="not available — no lane-network backend yet" />
            <KV k="Health probes" v="not available — no lane-health backend yet" />
          </div>
        </Panel>
      </div>
    </AppShell>
  );
}
