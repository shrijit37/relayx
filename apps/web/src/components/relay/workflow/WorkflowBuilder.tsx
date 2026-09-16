/**
 * Workflow editor canvas (Phase 6.6).
 *
 * The canvas is a VIEW over the canonical model: nodes/edges are serialized
 * losslessly (explicit positions, ports, config), the Inspector edits real
 * configuration, and validation is live (local schema + debounced backend
 * compile). Backend remains authoritative for publishability.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
    addEdge,
    Background,
    BackgroundVariant,
    Controls,
    MiniMap,
    ReactFlow,
    ReactFlowProvider,
    useEdgesState,
    useNodesState,
    useReactFlow,
    type Connection,
    type Edge,
    type OnSelectionChangeParams,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Link, useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";
import {
    AlertTriangle,
    Check,
    CircleSlash,
    History,
    PanelLeftClose,
    PanelRightClose,
    Play,
    Redo2,
    Rocket,
    Save,
    ShieldCheck,
    Square,
    Undo2,
    X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { KV, StatusDot } from "../primitives";
import { Inspector } from "./Inspector";
import { NodeLibrary } from "./NodeLibrary";
import { relayNodeTypes, type RelayNode, nodeMeta } from "./nodes";
import { edgeTypes } from "./edges";
import { defaultEdges, defaultNodes } from "./graph";
import { useUndoRedo } from "./use-undo-redo";
import {
    usePublishWorkflow,
    useSaveWorkflowMutation,
    useValidateMutation,
    useWorkflowLatestVersion,
    useLanes,
    useProviders,
    useCatalogModels,
} from "@/lib/use-workflow-publication";
import { createWorkflow, saveWorkflowVersion } from "@/lib/api";
import {
    deserializeWorkflow,
    serializeWorkflow,
    fromViewNode,
    toCanonicalEdges,
} from "@/lib/workflow";
import type { WorkflowJson } from "@/lib/workflow/serializer";
import type { CanonicalConfig, CanonicalNode, CanonicalWorkflow } from "@/lib/workflow/nodes";
import { validateWorkflow } from "@/lib/workflow/validation";
import { runReducer, type RunAction, type RunState } from "@/lib/run-state";

/** Editor kinds the runtime executes. Everything else is display-only and
 *  blocks publish (Phase 6.6 §3.2 — explicit, never silently dropped).
 *  The adapter's `canonicalFromView` also refuses via def.executable. */
const executableKinds = new Set([
    "input",
    "output",
    "provider",
    "route",
    "transform",
    "condition",
    "fallback",
    "retry",
    "mcp",
    "skill",
]);

let nodeSeq = 0;

function Toolbar({
    running,
    workflowId,
    workflowName,
    version,
    onRun,
    onStop,
    onSave,
    onValidate,
    onPublish,
    onUndo,
    onRedo,
    canUndo,
    canRedo,
    libraryOpen,
    inspectorOpen,
    toggleLibrary,
    toggleInspector,
    status,
}: {
    running: boolean;
    workflowId: string;
    workflowName: string;
    version: number | null;
    onRun: () => void;
    onStop: () => void;
    onSave: () => void;
    onValidate: () => void;
    onPublish: () => void;
    onUndo: () => void;
    onRedo: () => void;
    canUndo: boolean;
    canRedo: boolean;
    libraryOpen: boolean;
    inspectorOpen: boolean;
    toggleLibrary: () => void;
    toggleInspector: () => void;
    status: { label: string; tone: "ok" | "warn" | "fail" | "neutral" };
}) {
    return (
        <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-panel px-2">
            <div className="flex min-w-0 items-center gap-2">
                <span className="truncate text-xs font-semibold">
                    {workflowId === "new" ? "New workflow" : workflowName}
                </span>
                {version !== null && (
                    <span className="num rounded-sm border border-border bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                        v{version}
                    </span>
                )}
                <span
                    className={cn(
                        "num rounded-sm border px-1.5 py-0.5 text-[10px]",
                        status.tone === "ok" && "border-ok/30 bg-ok/10 text-ok",
                        status.tone === "warn" && "border-warn/40 bg-warn/10 text-warn",
                        status.tone === "fail" && "border-fail/40 bg-fail/10 text-fail",
                        status.tone === "neutral" && "border-border bg-muted text-muted-foreground",
                    )}
                >
                    {status.label}
                </span>
                {workflowId !== "new" && (
                    <span className="num hidden rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground lg:inline">
                        {workflowId}
                    </span>
                )}
            </div>

            <div className="ml-auto flex items-center gap-1">
                <button
                    onClick={onUndo}
                    disabled={!canUndo}
                    className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs text-muted-foreground hover:border-border-strong disabled:opacity-40"
                >
                    <Undo2 className="size-3.5" /> <span className="hidden lg:inline">Undo</span>
                </button>
                <button
                    onClick={onRedo}
                    disabled={!canRedo}
                    className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs text-muted-foreground hover:border-border-strong disabled:opacity-40"
                >
                    <Redo2 className="size-3.5" /> <span className="hidden lg:inline">Redo</span>
                </button>
                <span className="mx-1 h-5 w-px bg-border" />
                <button
                    onClick={onValidate}
                    className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong"
                >
                    <ShieldCheck className="size-3.5 text-muted-foreground" /> Validate
                </button>
                <button
                    onClick={onSave}
                    className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong"
                >
                    <Save className="size-3.5 text-muted-foreground" /> Save
                </button>
                {workflowId !== "new" && (
                    <Link
                        to="/workflows/$workflowId/versions"
                        params={{ workflowId }}
                        className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong"
                    >
                        <History className="size-3.5 text-muted-foreground" />
                        <span className="hidden lg:inline">Versions</span>
                    </Link>
                )}
                <button
                    onClick={running ? onStop : onRun}
                    className={cn(
                        "focus-ring flex h-7 items-center gap-1.5 rounded-sm px-2.5 text-xs font-medium",
                        running
                            ? "bg-fail/15 text-fail hover:bg-fail/25"
                            : "bg-primary text-primary-foreground hover:opacity-90",
                    )}
                >
                    {running ? <Square className="size-3" /> : <Play className="size-3" />}
                    {running ? "Stop" : "Run test"}
                </button>
                <button
                    onClick={onPublish}
                    className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border-strong px-2 text-xs font-medium hover:border-primary hover:text-primary"
                >
                    <Rocket className="size-3.5" /> Publish
                </button>
                <span className="mx-1 h-5 w-px bg-border" />
                <button
                    onClick={toggleLibrary}
                    className={cn(
                        "focus-ring hidden size-7 place-items-center rounded-sm hover:bg-panel-raised md:grid",
                        libraryOpen && "text-primary",
                    )}
                    aria-label="Toggle node library"
                >
                    <PanelLeftClose className="size-3.5" />
                </button>
                <button
                    onClick={toggleInspector}
                    className={cn(
                        "focus-ring hidden size-7 place-items-center rounded-sm hover:bg-panel-raised md:grid",
                        inspectorOpen && "text-primary",
                    )}
                    aria-label="Toggle inspector"
                >
                    <PanelRightClose className="size-3.5" />
                </button>
            </div>
        </header>
    );
}

export function WorkflowBuilder({ workflowId = "production-gateway" }: { workflowId?: string }) {
    const [mounted, setMounted] = useState(false);
    useEffect(() => setMounted(true), []);
    if (!mounted) {
        return (
            <div className="flex h-full items-center justify-center bg-canvas">
                <span className="num text-[11px] text-muted-foreground">
                    loading execution graph…
                </span>
            </div>
        );
    }
    return (
        <ReactFlowProvider>
            <div className="flex h-full min-h-0 flex-col">
                <Canvas workflowId={workflowId} />
            </div>
        </ReactFlowProvider>
    );
}

function Canvas({ workflowId }: { workflowId: string }) {
    const [nodes, setNodes, onNodesChange] = useNodesState<RelayNode>([]);
    const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
    const [selected, setSelected] = useState<RelayNode | null>(null);
    const [libraryOpen, setLibraryOpen] = useState(true);
    const [inspectorOpen, setInspectorOpen] = useState(true);
    const [dirty, setDirty] = useState(false);
    const [workflowName, setWorkflowName] = useState("Unnamed workflow");
    const [version, setVersion] = useState<number | null>(null);
    const [planHash, setPlanHash] = useState<string | null>(null);
    const [noSavedVersions, setNoSavedVersions] = useState(false);
    const { screenToFlowPosition, fitView, getNodes: getNodesRaw, getEdges } = useReactFlow();
    // useNodesState<RelayNode> actually stores RelayNode[]; cast getNodes to
    // match so useUndoRedo's generic infers the concrete type.
    const getNodes = getNodesRaw as unknown as () => RelayNode[];
    const undo = useUndoRedo({ getNodes, getEdges, setNodes, setEdges });

    const { data: latest, isPending: latestPending } = useWorkflowLatestVersion(workflowId);

    useEffect(() => {
        if (workflowId === "new") {
            setNodes(defaultNodes);
            setEdges(defaultEdges);
            setVersion(1);
            setWorkflowName("New workflow");
            setNoSavedVersions(true);
            return;
        }
        if (!latest) return;
        const migrated = (latest.workflow_json as { schema_version?: number }).schema_version;
        if (migrated !== undefined && migrated < 2) {
            toast.info("Loaded a legacy workflow — canonical fields were migrated on load.");
        }
        // Deserialize through the canonical serializer: persisted schema kinds map
        // to editor kinds (llm→provider, router→route, …), unsupported kinds stay
        // visible, and unmappable kinds surface as load-time errors (never a crash
        // or a silently-dropped node).
        const view = deserializeWorkflow(
            latest.workflow_json as Parameters<typeof deserializeWorkflow>[0],
        );
        if (view.errors.length > 0) {
            toast.error(`Cannot fully load — ${view.errors.join("; ")}`);
        }
        for (const w of view.warnings) toast.warning(w);
        setNodes(view.nodes);
        setEdges(view.edges);
        setVersion(latest.version);
        setWorkflowName((latest.workflow_json as { name?: string }).name ?? "Unnamed workflow");
        setNoSavedVersions(false);
    }, [latest, workflowId, setNodes, setEdges]);

    // Fit view once after initial nodes load.
    const fittedRef = useRef(false);
    useEffect(() => {
        if (!fittedRef.current && nodes.length > 0) {
            fittedRef.current = true;
            requestAnimationFrame(() => fitView({ padding: 0.15, duration: 0 }));
        }
    }, [nodes.length, fitView]);

    // ── Serialization through the canonical adapter. The serializer
    //    deterministically maps the canvas view → canonical → persisted JSON
    //    and reports every issue (unsupported nodes, missing lanes, fabricated
    //    config guards) — it never silently drops nodes or infers semantics
    //    from titles. The view carries `canonicalConfig` from the Inspector.
    //
    //    Debounced: `serializeWorkflow` walks the whole canvas and is
    //    re-invoked on every keystroke/drag via `nodes`/`edges` identity
    //    churn. Running it ~150ms after input settles keeps the editor
    //    responsive without staleness (the toolbar status and Save path both
    //    consume the debounced result).
    const [serializeResult, setSerializeResult] = useState<{
        json: WorkflowJson | null;
        unsupported: { data: { kind: string } }[];
        errors: string[];
        warnings: string[];
    }>({ json: null, unsupported: [], errors: [], warnings: [] });
    useEffect(() => {
        const handle = setTimeout(() => {
            const serialized = serializeWorkflow(nodes, edges, {
                id: workflowId === "new" ? "workflow" : workflowId,
                name: workflowName,
                version: version ?? 1,
            });
            setSerializeResult({
                json: serialized.workflow,
                unsupported: nodes.filter((n) => !executableKinds.has(n.data.kind)),
                errors: serialized.errors,
                warnings: serialized.warnings,
            });
        }, 150);
        return () => clearTimeout(handle);
    }, [nodes, edges, workflowId, workflowName, version]);

    // ── Live local validation (Phase 6.6 §15). The editor shows schema +
    //    semantic issues immediately (missing lane, unknown lane ref, missing
    //    required fields) from the canonical model + the control-plane lane
    //    table; publishability stays backend-authoritative (compile gate).
    const { data: lanes = [] } = useLanes();
    const canonical: CanonicalWorkflow = useMemo(
        () => ({
            id: workflowId === "new" ? "workflow" : workflowId,
            name: workflowName,
            version: version ?? 1,
            schemaVersion: 2,
            nodes: nodes.map(fromViewNode),
            edges: toCanonicalEdges(edges),
        }),
        [nodes, edges, workflowId, workflowName, version],
    );
    const validation = useMemo(
        () =>
            validateWorkflow(
                canonical,
                lanes.map((l) => ({ id: l.id, baseUrl: l.base_url })),
                [],
            ),
        [canonical, lanes],
    );

    const onConfigChange = useCallback(
        (nodeId: string, config: CanonicalConfig) => {
            // Config lives on the canonical model; carry it on the view so the
            // canvas and the next serialize round-trip see the real value.
            setNodes((ns) =>
                ns.map((n) =>
                    n.id === nodeId ? { ...n, data: { ...n.data, canonicalConfig: config } } : n,
                ),
            );
            markDirty();
        },
        [setNodes],
    );

    const onTitleChange = useCallback(
        (nodeId: string, title: string) => {
            setNodes((ns) =>
                ns.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, title } } : n)),
            );
            markDirty();
        },
        [setNodes],
    );

    const markDirty = useCallback(() => {
        setDirty(true);
        setPlanHash(null);
    }, []);

    const status = useMemo(() => {
        if (planHash && dirty) return { label: "Modified", tone: "warn" as const };
        if (planHash) return { label: "Valid · compiled", tone: "ok" as const };
        if (!serializeResult.json) {
            const unsupported = serializeResult.unsupported.map((n) => n.data.kind);
            const label =
                unsupported.length > 0
                    ? `Invalid — ${[...new Set(unsupported)].join(", ")} blocks publish`
                    : "Invalid — resolve errors to publish";
            return { label, tone: "fail" as const };
        }
        return { label: "Modified", tone: "warn" as const };
    }, [planHash, dirty, serializeResult.json, serializeResult.unsupported]);

    const onConnect = useCallback(
        (c: Connection) => setEdges((eds) => addEdge({ ...c, type: "deletable" }, eds)),
        [setEdges],
    );

    const isValidConnection = useCallback(
        (c: Connection | Edge) => {
            // No self-loops.
            if (c.source === c.target) return false;
            // Cycle detection: DFS from target through existing edges back to source.
            const adj = new Map<string, string[]>();
            for (const e of edges) {
                const list = adj.get(e.source) ?? [];
                list.push(e.target);
                adj.set(e.source, list);
            }
            const visited = new Set<string>();
            const stack = [c.target];
            while (stack.length) {
                const n = stack.pop()!;
                if (n === c.source) return false;
                if (visited.has(n)) continue;
                visited.add(n);
                for (const next of adj.get(n) ?? []) stack.push(next);
            }
            return true;
        },
        [edges],
    );

    const onSelectionChange = useCallback((p: OnSelectionChangeParams) => {
        setSelected((p.nodes[0] as RelayNode) ?? null);
    }, []);

    const inspectorNode = useMemo(
        () => nodes.find((n) => n.id === selected?.id),
        [nodes, selected],
    );

    const { data: providers = [] } = useProviders();
    const providerOptions = providers.map((p) => ({ value: p.name, label: p.name }));
    const canonicalNode = useMemo(
        () => inspectorCanonical(inspectorNode),
        [inspectorNode],
    );
    // Model picker backed by the models.dev catalog (live, searchable).
    // Falls back to the provider's stored default model if the catalog is
    // unreachable/empty (sync hasn't run yet).
    const catalogProvider = canonicalNode?.config.kind === "llm"
        ? canonicalNode.config.config.provider
        : undefined;
    // Only fetch catalog models when a provider is actually selected — the
    // model picker is useless without one, and the fallback is the provider
    // row's stored default model (no catalog needed).
    const { data: catalogModels = [] } = useCatalogModels(
        catalogProvider
            ? { provider: catalogProvider, capability: "tool_call" }
            : undefined,
        { enabled: Boolean(catalogProvider) },
    );
    const modelOptions = useMemo(() => {
        if (!canonicalNode) return [];
        const cfg = canonicalNode.config;
        if (cfg.kind !== "llm" || !cfg.config.provider) return [];
        const match = providers.find((p) => p.name === cfg.config.provider);
        const opts: { value: string; label: string }[] = [];
        // Catalog ids are "provider/model" keys (models.dev shape). The
        // picker stores the bare provider-native id ("gpt-4o") — the wire
        // model — not the prefixed catalog key, which the upstream APIs
        // reject. The label keeps the rich "Name · Provider · ctx" text.
        const bareModel = (catalogId: string) =>
            catalogId.split("/").slice(1).join("/");
        if (
            cfg.config.model &&
            !catalogModels.some((m) => bareModel(m.id) === cfg.config.model)
        ) {
            opts.push({ value: cfg.config.model, label: cfg.config.model });
        }
        // Prefer catalog models for the selected provider, then the provider
        // row's stored default model as a genuine fallback (only when the
        // catalog has no rows for this provider — e.g. sync hasn't run yet).
        for (const m of catalogModels) {
            opts.push({
                value: bareModel(m.id),
                label: `${m.name} · ${m.provider_name}${m.limits ? ` · ${m.limits.context.toLocaleString()} ctx` : ""}`,
            });
        }
        if (catalogModels.length === 0 && match?.model)
            opts.push({ value: match.model, label: match.model });
        return opts;
    }, [catalogModels, providers, canonicalNode]);

    const navigate = useNavigate();

    // ── Real Save / Validate / Publish (control-plane mutations) ──────────
    // Every action is driven by the real mutation outcome: pending (disabled in
    // UI), success (backend version/plan-hash), or error (real backend message).
    // No fabricated "wired" toasts, no local-only pseudo-state.
    const publish = usePublishWorkflow();
    const saveMutation = useSaveWorkflowMutation(workflowId);
    const validateMutation = useValidateMutation();

    // Serialize from current canvas state on action — NOT from the debounced
    // `serializeResult` which can be 150ms stale. A click within 150ms of an
    // edit would otherwise submit the previous canvas state (review finding #12).
    const serializeTo = useCallback(() => {
        const serialized = serializeWorkflow(nodes, edges, {
            id: workflowId === "new" ? "workflow" : workflowId,
            name: workflowName,
            version: version ?? 1,
        });
        const unsupported = nodes.filter((n) => !executableKinds.has(n.data.kind));
        if (serialized.workflow) {
            return { workflow: serialized.workflow as WorkflowJson, errors: [], warnings: serialized.warnings };
        }
        return { workflow: null, errors: serialized.errors, warnings: serialized.warnings };
    }, [nodes, edges, workflowId, workflowName, version]);

    // Save in "new" mode creates the real workflow row first (same control-plane
    // POST as the workflow list's Create button), persists the canvas as its
    // first immutable version, then navigates to the durable id so the editor
    // reloads from the backend — no fabricated local persistence.
    const createAndSave = useCallback(async () => {
        const result = serializeTo();
        if (!result.workflow) {
            toast.error(`Cannot save — ${result.errors.join("; ")}`);
            return;
        }
        try {
            const row = await createWorkflow(result.workflow.name || "New workflow");
            await saveWorkflowVersion(row.id, { ...result.workflow, id: row.id, version: 1 });
            toast.success(`Created ${row.id} · v1`);
            navigate({ to: "/workflows/$workflowId", params: { workflowId: row.id } });
        } catch (err) {
            toast.error(`Create failed — ${err instanceof Error ? err.message : String(err)}`);
        }
    }, [serializeTo, navigate]);

    const onSave = useCallback(() => {
        if (workflowId === "new") {
            void createAndSave();
            return;
        }
        const result = serializeTo();
        if (!result.workflow) {
            toast.error(`Cannot save — ${result.errors.join("; ")}`);
            return;
        }
        saveMutation.mutate(result.workflow, {
            onSuccess: (row) => {
                setVersion(row.version);
                setDirty(false);
                toast.success(`Saved v${row.version}`);
            },
            onError: (err) => toast.error(`Save failed — ${err.message}`),
        });
    }, [workflowId, serializeTo, saveMutation, createAndSave]);

    const onValidate = useCallback(() => {
        if (workflowId === "new") {
            toast.error("Cannot validate an unsaved workflow — open an existing workflow first.");
            return;
        }
        const result = serializeTo();
        if (!result.workflow) {
            toast.error(`Cannot validate — ${result.errors.join("; ")}`);
            return;
        }
        validateMutation.mutate(result.workflow, {
            onSuccess: (info) => {
                setPlanHash(info.plan_hash);
                setDirty(false);
                toast.success(`Valid — plan ${info.plan_hash?.slice(0, 8) ?? "—"}`);
            },
            onError: (err) => toast.error(`Validation failed — ${err.message}`),
        });
    }, [workflowId, serializeTo, validateMutation]);

    const onPublish = useCallback(() => {
        if (workflowId === "new") {
            toast.error("Cannot publish an unsaved workflow — save it first.");
            return;
        }
        const result = serializeTo();
        if (!result.workflow) {
            toast.error(`Cannot publish — ${result.errors.join("; ")}`);
            return;
        }
        if (result.warnings.length > 0)
            toast.warning(`Publishing with warnings: ${result.warnings.join("; ")}`);
        publish.mutate(
            { workflow: result.workflow, lanes: {} },
            {
                onSuccess: (info) =>
                    toast.success(`Published v${info.version} · ${info.workflowId}`),
                onError: (err) => toast.error(`Publish failed — ${err.message}`),
            },
        );
    }, [workflowId, serializeTo, publish]);

    // ── Real Run ─────────────────────────────────────────────────────────
    // Execution state is driven exclusively by real mutation outcomes: start →
    // running, resolved → completed (real envelope), rejected → failed (real
    // backend error), aborted → cancelled. `running` is DERIVED from the
    // reducer — there is no second boolean that can drift (Phase 6.5 §3.1/§6).
    const [run, setRun] = useState<RunState>({ phase: "idle" });
    // A streaming run (tokens arriving) is still an active run: the toolbar
    // must keep showing Stop, or a user can start a second concurrent run
    // that orphans the first stream (unabortable).
    const running = run.phase === "running" || run.phase === "streaming";
    // Live token output is written imperatively into a leaf <pre> (one span
    // per token, O(1)/token) instead of re-dispatching every token through
    // the top-level reducer — that re-rendered the whole editor tree at
    // token cadence and re-copied the full transcript per token (O(n²)).
    const streamOutRef = useRef<HTMLPreElement | null>(null);
    const [runBody, setRunBody] = useState(
        JSON.stringify(
            { messages: [{ role: "user", content: "hello from the run panel" }] },
            null,
            2,
        ),
    );
    const [runPanelOpen, setRunPanelOpen] = useState(false);
    const abortRef = useRef<AbortController | null>(null);

    const runDispatch = useCallback((action: RunAction) => {
        setRun((s) => runReducer(s, action));
    }, []);

    const submitRun = useCallback(async () => {
        if (workflowId === "new") {
            toast.error("Cannot run an unsaved workflow — save and publish it first.");
            return;
        }
        if (dirty) {
            toast.warning(
                "Run executes the published ACTIVE version — unsaved changes won't be executed. Save & publish to run them.",
            );
        }
        let body: unknown;
        try {
            body = JSON.parse(runBody || "null");
        } catch {
            toast.error("Run request body is not valid JSON.");
            return;
        }
        const controller = new AbortController();
        abortRef.current = controller;
        runDispatch({ type: "start" });
        setRunPanelOpen(true);

        // Imperative stream sink: clear any previous run's spans now (the panel
        // is already mounted on subsequent runs). On the very first run the ref
        // is still null here — the lazy reset inside the token handler covers
        // that case, and a token-less first run shows "—" anyway.
        const sink = streamOutRef; // ref, not element — resolve per token
        if (sink.current) {
            sink.current.textContent = "";
            delete sink.current.dataset["started"];
        }

        try {
            const { runWorkflowStream } = await import("@/lib/api");
            let streamingDispatched = false;
            for await (const event of runWorkflowStream(workflowId, body, controller.signal)) {
                switch (event.type) {
                    case "token": {
                        const el = sink.current;
                        if (el) {
                            if (el.dataset["started"] !== "1") {
                                el.textContent = "";
                                el.dataset["started"] = "1";
                            }
                            const span = document.createElement("span");
                            span.textContent = event.delta;
                            el.appendChild(span);
                        }
                        if (!streamingDispatched) {
                            streamingDispatched = true;
                            runDispatch({ type: "streaming", delta: "" });
                        }
                        break;
                    }
                    case "done":
                        runDispatch({
                            type: "completed",
                            result: {
                                requestId: event.request_id,
                                workflowId: event.workflow_id,
                                workflowVersion: event.workflow_version,
                                snapshotVersion: event.snapshot_version,
                                planHash: event.plan_hash,
                                output: event.output,
                            },
                        });
                        return;
                    case "error":
                        runDispatch({ type: "failed", error: event.error });
                        toast.error(`Run failed — ${event.error}`);
                        return;
                }
            }
            if (abortRef.current === controller) {
                runDispatch({ type: "failed", error: "stream ended before terminal event" });
                toast.error("Run failed — stream ended unexpectedly");
            }
        } catch (err: unknown) {
            if (err instanceof DOMException && err.name === "AbortError") {
                runDispatch({ type: "cancel" });
            } else {
                runDispatch({ type: "failed", error: String(err) });
                toast.error(`Run failed — ${String(err)}`);
            }
        }
    }, [workflowId, dirty, runBody, runDispatch]);

    const cancelRun = useCallback(() => {
        abortRef.current?.abort();
        abortRef.current = null;
        runDispatch({ type: "cancel" });
    }, [runDispatch]);

    const createNode = useCallback(
        (kind: string, position: { x: number; y: number }): RelayNode => {
            nodeSeq += 1;
            return {
                id: `${kind}-${nodeSeq}`,
                type: "relay",
                position,
                data: {
                    kind: kind as RelayNode["data"]["kind"],
                    title: kind,
                    lines: [],
                    metaLeft: "draft",
                    status: "idle",
                },
            };
        },
        [],
    );

    const addNodeAtCenter = useCallback(
        (kind: string) => {
            const center = screenToFlowPosition({
                x: window.innerWidth / 2,
                y: window.innerHeight / 2,
            });
            const node = createNode(kind, center);
            undo.record();
            setNodes((ns) => [...ns, node]);
            markDirty();
        },
        [createNode, screenToFlowPosition, setNodes, undo],
    );

    // Context menu state.
    const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
    useEffect(() => {
        if (!contextMenu) return;
        const close = () => setContextMenu(null);
        const onKey = (e: KeyboardEvent) => {
            if (e.key === "Escape") close();
        };
        window.addEventListener("click", close, { once: true });
        window.addEventListener("keydown", onKey, { once: true });
        return () => {
            window.removeEventListener("click", close);
            window.removeEventListener("keydown", onKey);
        };
    }, [contextMenu]);

    // Keyboard shortcuts.
    const wrapperRef = useRef<HTMLDivElement>(null);
    useEffect(() => {
        const el = wrapperRef.current;
        if (!el) return;
        const handler = (e: KeyboardEvent) => {
            // Delete selected nodes/edges.
            if (e.key === "Delete" || e.key === "Backspace") {
                if (
                    (e.target as HTMLElement).tagName === "INPUT" ||
                    (e.target as HTMLElement).tagName === "TEXTAREA"
                )
                    return;
                if (nodes.some((n) => n.selected) || edges.some((e) => e.selected)) {
                    undo.record();
                    setNodes((ns) => ns.filter((n) => !n.selected));
                    setEdges((es) => es.filter((e) => !e.selected));
                    markDirty();
                }
            }
            // Ctrl+Z / Ctrl+Shift+Z
            if ((e.metaKey || e.ctrlKey) && e.key === "z") {
                e.preventDefault();
                if (e.shiftKey) undo.redo();
                else undo.undo();
            }
            // Ctrl+S save.
            if ((e.metaKey || e.ctrlKey) && e.key === "s") {
                e.preventDefault();
                onSave();
            }
        };
        el.addEventListener("keydown", handler);
        return () => el.removeEventListener("keydown", handler);
    }, [undo, setNodes, setEdges, onSave, nodes, edges]);

    return (
        <div className="flex min-h-0 flex-1 flex-col">
            <Toolbar
                running={running}
                workflowId={workflowId}
                workflowName={workflowName}
                version={version}
                onRun={submitRun}
                onStop={cancelRun}
                onSave={onSave}
                onValidate={onValidate}
                onPublish={onPublish}
                onUndo={() => undo.undo()}
                onRedo={() => undo.redo()}
                canUndo={undo.canUndo}
                canRedo={undo.canRedo}
                libraryOpen={libraryOpen}
                inspectorOpen={inspectorOpen}
                toggleLibrary={() => setLibraryOpen((o) => !o)}
                toggleInspector={() => setInspectorOpen((o) => !o)}
                status={status}
            />

            <div className="flex min-h-0 flex-1">
                {libraryOpen && (
                    <NodeLibrary
                        className="w-[196px] shrink-0 border-r border-border"
                        onAdd={addNodeAtCenter}
                    />
                )}

                <div
                    ref={wrapperRef}
                    tabIndex={-1}
                    className="relay-canvas relative min-w-0 flex-1 bg-canvas outline-none"
                    onDrop={(event) => {
                        event.preventDefault();
                        const kind = event.dataTransfer.getData("application/relay-node");
                        if (!kind) return;
                        const position = screenToFlowPosition({
                            x: event.clientX,
                            y: event.clientY,
                        });
                        const node = createNode(kind, position);
                        undo.record();
                        setNodes((ns) => [...ns, node]);
                        markDirty();
                    }}
                    onDragOver={(e) => {
                        e.preventDefault();
                        e.dataTransfer.dropEffect = "move";
                    }}
                >
                    <ReactFlow
                        nodes={nodes}
                        edges={edges}
                        onNodesChange={(changes) => {
                            // Record once per gesture via onNodeDragStart;
                            // removes are recorded here (node drags may also
                            // emit position changes on every frame, which
                            // would flood undo with one entry per animation
                            // frame and evict the pre-drag snapshot).
                            if (changes.some((c) => c.type === "remove")) undo.record();
                            onNodesChange(changes);
                            if (
                                changes.some(
                                    (c) =>
                                        c.type === "position" ||
                                        c.type === "add" ||
                                        c.type === "remove" ||
                                        c.type === "replace",
                                )
                            )
                                markDirty();
                        }}
                        onEdgesChange={(changes) => {
                            if (changes.some((c) => c.type === "remove")) undo.record();
                            onEdgesChange(changes);
                            if (
                                changes.some(
                                    (c) =>
                                        c.type === "add" ||
                                        c.type === "remove" ||
                                        c.type === "replace",
                                )
                            )
                                markDirty();
                        }}
                        onConnect={(c) => {
                            undo.record();
                            onConnect(c);
                            markDirty();
                        }}
                        onNodeDragStart={() => {
                            // Record the pre-drag snapshot once per gesture
                            // (xyflow's onNodeDrag fires per move frame; that
                            // floods undo and can evict the real snapshot).
                            undo.record();
                        }}
                        onSelectionChange={onSelectionChange}
                        isValidConnection={isValidConnection}
                        onPaneContextMenu={(e) => {
                            e.preventDefault();
                            setContextMenu({ x: e.clientX, y: e.clientY });
                        }}
                        nodeTypes={relayNodeTypes}
                        edgeTypes={edgeTypes}
                        snapToGrid
                        snapGrid={[16, 16]}
                        multiSelectionKeyCode="Shift"
                        edgesReconnectable
                        proOptions={{ hideAttribution: true }}
                        defaultViewport={{ x: 24, y: 0, zoom: 0.72 }}
                        minZoom={0.25}
                        maxZoom={1.75}
                    >
                        <Background
                            variant={BackgroundVariant.Dots}
                            gap={16}
                            size={1}
                            color="var(--color-border)"
                        />
                        <Controls
                            className="!left-2 !bottom-2 !rounded-sm !border !border-border !bg-panel !shadow-none [&>button]:!border-border [&>button]:!bg-panel [&>button]:!fill-current [&>button]:!text-foreground [&>button:hover]:!bg-panel-raised"
                            showInteractive={false}
                        />
                        <MiniMap
                            pannable
                            zoomable
                            className="!right-2 !bottom-2"
                            style={{ width: 168, height: 96 }}
                            maskColor="oklch(0.14 0.008 264 / 70%)"
                            nodeColor={(n) => {
                                const meta = nodeMeta[(n as RelayNode).data?.kind];
                                if (!meta) return "var(--color-border-strong)";
                                const map: Record<string, string> = {
                                    "text-info": "oklch(0.65 0.12 240)",
                                    "text-primary": "oklch(0.55 0.18 270)",
                                    "text-warn": "oklch(0.75 0.15 75)",
                                    "text-ok": "oklch(0.65 0.17 155)",
                                    "text-violet": "oklch(0.55 0.2 300)",
                                    "text-fail": "oklch(0.6 0.18 25)",
                                };
                                return map[meta.accent] ?? "var(--color-border-strong)";
                            }}
                        />
                    </ReactFlow>

                    {latestPending ? (
                        <div className="pointer-events-none absolute top-2 left-1/2 z-10 -translate-x-1/2">
                            <div className="flex items-center gap-2 rounded-sm border border-border bg-panel/95 px-2.5 py-1.5 shadow-panel">
                                <StatusDot status="loading" />
                                <span className="num text-[11px]">loading workflow…</span>
                            </div>
                        </div>
                    ) : null}

                    {noSavedVersions && !latestPending && (
                        <div className="pointer-events-none absolute top-2 left-1/2 z-10 -translate-x-1/2">
                            <div className="flex items-center gap-2 rounded-sm border border-warn/40 bg-panel/95 px-2.5 py-1.5 shadow-panel">
                                <AlertTriangle className="size-3 text-warn" />
                                <span className="num text-[11px]">
                                    no saved versions yet — this workflow has never been persisted
                                </span>
                            </div>
                        </div>
                    )}

                    {contextMenu && (
                        <div
                            className="absolute z-50 min-w-[160px] rounded-sm border border-border bg-panel py-1 shadow-panel"
                            style={{ left: contextMenu.x, top: contextMenu.y }}
                        >
                            <div className="px-2 py-1 text-[10px] font-medium text-muted-foreground">
                                Add node
                            </div>
                            {[
                                "input",
                                "output",
                                "transform",
                                "condition",
                                "route",
                                "provider",
                                "mcp",
                                "skill",
                            ].map((k) => {
                                const m = nodeMeta[k as keyof typeof nodeMeta];
                                const Icon = m.icon;
                                return (
                                    <button
                                        key={k}
                                        onClick={() => {
                                            addNodeAtCenter(k);
                                            setContextMenu(null);
                                        }}
                                        className="flex w-full items-center gap-2 px-2 py-1.5 text-xs hover:bg-panel-raised"
                                    >
                                        <Icon className={cn("size-3.5", m.accent)} />
                                        {m.label}
                                    </button>
                                );
                            })}
                        </div>
                    )}
                </div>

                {inspectorOpen && (
                    <Inspector
                        className="w-[268px] shrink-0 border-l border-border"
                        node={canonicalNode}
                        onConfigChange={onConfigChange}
                        onTitleChange={onTitleChange}
                        planHash={planHash}
                        onClose={() => setInspectorOpen(false)}
                        issues={validation.issues}
                        laneOptions={lanes.map((l) => ({ value: l.id, label: l.id }))}
                        providerOptions={providerOptions}
                        modelOptions={modelOptions}
                    />
                )}
            </div>

            {runPanelOpen && (
                <RunPanel
                    phase={run.phase}
                    result={run.result}
                    error={run.error}
                    streamOutRef={streamOutRef}
                    onBodyChange={(v) => setRunBody(v)}
                    body={runBody}
                    onSubmit={submitRun}
                    onCancel={cancelRun}
                    onClose={() => {
                        if (run.phase === "running") cancelRun();
                        setRunPanelOpen(false);
                        setRun({ phase: "idle" });
                    }}
                />
            )}

            <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-border bg-panel px-3 text-[11px]">
                {planHash ? (
                    <span className="flex items-center gap-1.5 text-ok">
                        <Check className="size-3" />
                        <span className="num">{planHash.slice(0, 8)} — validated</span>
                    </span>
                ) : (
                    <span className="flex items-center gap-1.5 text-muted-foreground">
                        <CircleSlash className="size-3" />
                        <span className="num">not validated</span>
                    </span>
                )}
                <span className="h-4 w-px bg-border" />
                <span className="num hidden text-muted-foreground sm:inline">
                    {nodes.length} nodes · {edges.length} edges
                </span>
                <span className="ml-auto flex items-center gap-3">
                    <span className="num flex items-center gap-1.5">
                        {dirty ? (
                            <span className="text-warn">unsaved changes</span>
                        ) : (
                            <span className="flex items-center gap-1.5 text-ok">
                                <Check className="size-3" /> saved
                            </span>
                        )}
                    </span>
                </span>
            </footer>
        </div>
    );
}

/** Canonical view of the selected RF node for the Inspector. Reuses the
 *  canonical serializer's view→canonical mapping (same config/ports the
 *  validator sees), so per-node issues match the displayed config. */
function inspectorCanonical(rf: RelayNode | null | undefined): CanonicalNode | null {
    if (!rf) return null;
    return fromViewNode(rf as Parameters<typeof fromViewNode>[0]);
}

function RunPanel({
    phase,
    result,
    error = undefined,
    streamOutRef,
    body,
    onBodyChange,
    onSubmit,
    onCancel,
    onClose,
}: {
    phase: RunState["phase"];
    result: RunState["result"];
    error: string | undefined;
    /** Imperative sink for live token output (avoids re-render per token). */
    streamOutRef: React.RefObject<HTMLPreElement | null>;
    body: string;
    onBodyChange: (v: string) => void;
    onSubmit: () => void;
    onCancel: () => void;
    onClose: () => void;
}) {
    const active = phase === "running" || phase === "streaming";
    return (
        <div className="flex shrink-0 items-start gap-3 border-t border-border bg-panel px-3 py-2">
            <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <div className="flex items-center gap-2">
                    <span className="label-xs">Request body</span>
                    <span className="num text-[10px] text-muted-foreground">
                        executes the published ACTIVE version
                    </span>
                </div>
                <textarea
                    value={body}
                    onChange={(e) => onBodyChange(e.target.value)}
                    rows={3}
                    spellCheck={false}
                    disabled={active}
                    className="num w-full resize-y rounded-sm border border-border bg-canvas px-2 py-1.5 font-mono text-[11px] outline-none focus:border-primary disabled:opacity-60"
                />
            </div>

            <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <div className="flex items-center gap-2">
                    <span className="label-xs">Execution state</span>
                    <span className="num text-[10px] uppercase text-muted-foreground">{phase}</span>
                </div>
                {phase === "completed" && result ? (
                    <div className="space-y-px">
                        <KV k="Request" v={result.requestId} />
                        <KV k="Workflow" v={`${result.workflowId} · v${result.workflowVersion}`} />
                        <KV k="Snapshot" v={`v${result.snapshotVersion}`} />
                        <KV k="Plan" v={result.planHash.slice(0, 12)} />
                    </div>
                ) : phase === "failed" ? (
                    <div className="rounded-sm border border-fail/40 bg-fail/8 px-2 py-1.5 text-[11px] text-fail">
                        {error ?? "run failed"}
                    </div>
                ) : phase === "cancelled" ? (
                    <div className="text-[11px] text-muted-foreground">
                        Run cancelled by the user.
                    </div>
                ) : phase === "running" || phase === "streaming" ? (
                    <div className="flex items-center gap-1.5 text-[11px] text-info">
                        <StatusDot status="running" />
                        {phase === "streaming" ? "streaming tokens…" : "executing on the gateway…"}
                    </div>
                ) : (
                    <div className="text-[11px] text-muted-foreground">Not started.</div>
                )}
            </div>

            <div className="flex min-w-0 max-w-[50%] flex-1 flex-col gap-1.5">
                <div className="label-xs">Output</div>
                <pre
                    ref={streamOutRef}
                    className="num max-h-[120px] min-h-0 flex-1 overflow-auto rounded-sm border border-border bg-canvas px-2 py-1.5 font-mono text-[11px]"
                >
                    {phase === "completed" && result
                        ? JSON.stringify(result.output, null, 2)
                        : "—"}
                </pre>
            </div>

            <div className="flex shrink-0 flex-col gap-1.5">
                <button
                    onClick={active ? onCancel : onSubmit}
                    className={cn(
                        "focus-ring flex h-7 items-center justify-center gap-1.5 rounded-sm px-2.5 text-xs font-medium",
                        active
                            ? "bg-fail/15 text-fail hover:bg-fail/25"
                            : "bg-primary text-primary-foreground hover:opacity-90",
                    )}
                >
                    {active ? <Square className="size-3" /> : <Play className="size-3" />}
                    {active ? "Abort" : "Run"}
                </button>
                <button
                    onClick={onClose}
                    className="focus-ring flex h-7 items-center justify-center gap-1.5 rounded-sm border border-border px-2.5 text-xs text-muted-foreground hover:border-border-strong hover:text-foreground"
                >
                    <X className="size-3" /> Close
                </button>
            </div>
        </div>
    );
}
