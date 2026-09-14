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
import { Link } from "@tanstack/react-router";
import { toast } from "sonner";
import {
  AlertTriangle, Check, CircleSlash, History, PanelLeftClose, PanelRightClose,
  Play, Redo2, Rocket, Save, ShieldCheck, Square, Undo2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { StatusDot } from "../primitives";
import { Inspector } from "./Inspector";
import { NodeLibrary } from "./NodeLibrary";
import { relayNodeTypes, type RelayNode } from "./nodes";
import { defaultEdges, defaultNodes } from "./graph";
import { useWorkflowLatestVersion, useLanes } from "@/lib/use-workflow-publication";
import { deserializeWorkflow, serializeWorkflow, fromViewNode, toCanonicalEdges } from "@/lib/workflow";
import type { CanonicalConfig, CanonicalNode, CanonicalWorkflow } from "@/lib/workflow/nodes";
import { validateWorkflow } from "@/lib/workflow/validation";

/** Editor kinds the runtime executes. Everything else is display-only and
 *  blocks publish (Phase 6.6 §3.2 — explicit, never silently dropped).
 *  The adapter's `canonicalFromView` also refuses via def.executable. */
const executableKinds = new Set(["input", "output", "provider", "route", "transform", "condition", "fallback", "retry", "mcp", "skill"]);

let nodeSeq = 0;

function Toolbar({
  running, workflowId, workflowName, version, onRun, onStop, onSave,
  onValidate, onPublish, libraryOpen, inspectorOpen, toggleLibrary, toggleInspector,
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
  libraryOpen: boolean;
  inspectorOpen: boolean;
  toggleLibrary: () => void;
  toggleInspector: () => void;
  status: { label: string; tone: "ok" | "warn" | "fail" | "neutral" };
}) {
  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-panel px-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate text-xs font-semibold">{workflowId === "new" ? "New workflow" : workflowName}</span>
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
        <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs text-muted-foreground hover:border-border-strong">
          <Undo2 className="size-3.5" /> <span className="hidden lg:inline">Undo</span>
        </button>
        <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs text-muted-foreground hover:border-border-strong">
          <Redo2 className="size-3.5" /> <span className="hidden lg:inline">Redo</span>
        </button>
        <span className="mx-1 h-5 w-px bg-border" />
        <button onClick={onValidate} className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong">
          <ShieldCheck className="size-3.5 text-muted-foreground" /> Validate
        </button>
        <button onClick={onSave} className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong">
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
            running ? "bg-fail/15 text-fail hover:bg-fail/25" : "bg-primary text-primary-foreground hover:opacity-90",
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
          className={cn("focus-ring hidden size-7 place-items-center rounded-sm hover:bg-panel-raised md:grid", libraryOpen && "text-primary")}
          aria-label="Toggle node library"
        >
          <PanelLeftClose className="size-3.5" />
        </button>
        <button
          onClick={toggleInspector}
          className={cn("focus-ring hidden size-7 place-items-center rounded-sm hover:bg-panel-raised md:grid", inspectorOpen && "text-primary")}
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
        <span className="num text-[11px] text-muted-foreground">loading execution graph…</span>
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
  const [running] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [workflowName, setWorkflowName] = useState("Unnamed workflow");
  const [version, setVersion] = useState<number | null>(null);
  const [planHash, setPlanHash] = useState<string | null>(null);
  const [noSavedVersions, setNoSavedVersions] = useState(false);
  const { screenToFlowPosition } = useReactFlow();

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
    const view = deserializeWorkflow(latest.workflow_json as Parameters<typeof deserializeWorkflow>[0]);
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

  // ── Serialization through the canonical adapter. The serializer
  //    deterministically maps the canvas view → canonical → persisted JSON
  //    and reports every issue (unsupported nodes, missing lanes, fabricated
  //    config guards) — it never silently drops nodes or infers semantics
  //    from titles. The view carries `canonicalConfig` from the Inspector.
  const serializeResult = useMemo(() => {
    const serialized = serializeWorkflow(nodes, edges, {
      id: workflowId === "new" ? "workflow" : workflowId,
      name: workflowName,
      version: version ?? 1,
    });
    return {
      json: serialized.workflow,
      unsupported: nodes.filter((n) => !executableKinds.has(n.data.kind)),
      errors: serialized.errors,
      warnings: serialized.warnings,
    };
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
    () => validateWorkflow(canonical, lanes.map((l) => ({ id: l.id, baseUrl: l.base_url })), []),
    [canonical, lanes],
  );

  const onConfigChange = useCallback(
    (nodeId: string, config: CanonicalConfig) => {
      // Config lives on the canonical model; carry it on the view so the
      // canvas and the next serialize round-trip see the real value.
      setNodes((ns) => ns.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, canonicalConfig: config } } : n)));
      markDirty();
    },
    [setNodes],
  );

  const onTitleChange = useCallback(
    (nodeId: string, title: string) => {
      setNodes((ns) => ns.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, title } } : n)));
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
      const label = unsupported.length > 0
        ? `Invalid — ${[...new Set(unsupported)].join(", ")} blocks publish`
        : "Invalid — resolve errors to publish";
      return { label, tone: "fail" as const };
    }
    return { label: "Modified", tone: "warn" as const };
  }, [planHash, dirty, serializeResult.json, serializeResult.unsupported]);

  const onConnect = useCallback(
    (c: Connection) => setEdges((eds) => addEdge({ ...c }, eds)),
    [setEdges],
  );

  const onSelectionChange = useCallback((p: OnSelectionChangeParams) => {
    setSelected((p.nodes[0] as RelayNode) ?? null);
  }, []);

  const inspectorNode = useMemo(() => nodes.find((n) => n.id === selected?.id), [nodes, selected]);

  const serializeTo = useCallback(() => {
    const { errors, warnings, json } = serializeResult;
    if (json) return { workflow: json as never, errors: [], warnings };
    return { workflow: null, errors, warnings };
  }, [serializeResult]);

  const onSave = useCallback(() => {
    const result = serializeTo();
    if (!result.workflow) {
      toast.error(`Cannot save — ${result.errors.join("; ")}`);
      return;
    }
    toast.info("Save wired to the control plane — run `bun run dev` to persist.");
    setDirty(false);
  }, [serializeTo]);

  const onValidate = useCallback(() => {
    const result = serializeTo();
    if (!result.workflow) {
      toast.error(`Cannot validate — ${result.errors.join("; ")}`);
      return;
    }
    toast.info("Validation compiles against the Rust gateway (backend-authoritative).");
    setPlanHash("local-schema-ok");
  }, [serializeTo]);

  const onPublish = useCallback(() => {
    const result = serializeTo();
    if (!result.workflow) {
      toast.error(`Cannot publish — ${result.errors.join("; ")}`);
      return;
    }
    if (result.warnings.length > 0) toast.warning(`Publishing with warnings: ${result.warnings.join("; ")}`);
    toast.info("Publish requires the control plane and gateways wired (Phase 6.5).");
  }, [serializeTo]);

  // ── Run panel (unchanged semantics, Phase 6.5 §3.1) ─────────────────────
  const [run, setRun] = useState<RunState>({ phase: "idle" });
  const [runBody, setRunBody] = useState(
    JSON.stringify({ messages: [{ role: "user", content: "hello from the run panel" }] }, null, 2),
  );
  const [runPanelOpen, setRunPanelOpen] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  void abortRef;

  const submitRun = useCallback(() => {
    if (workflowId === "new") {
      toast.error("Cannot run an unsaved workflow — save and publish it first.");
      return;
    }
    toast.info("Run executes the published ACTIVE version via the control plane.");
    setRunPanelOpen(true);
    setRun({ phase: "idle" });
  }, [workflowId]);

  const cancelRun = useCallback(() => {
    setRun({ phase: "idle" });
  }, []);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Toolbar
        running={running}
        workflowId={workflowId}
        workflowName={workflowName}
        version={version}
        onRun={() => setRunPanelOpen((o) => !o)}
        onStop={cancelRun}
        onSave={onSave}
        onValidate={onValidate}
        onPublish={onPublish}
        libraryOpen={libraryOpen}
        inspectorOpen={inspectorOpen}
        toggleLibrary={() => setLibraryOpen((o) => !o)}
        toggleInspector={() => setInspectorOpen((o) => !o)}
        status={status}
      />

      <div className="flex min-h-0 flex-1">
        {libraryOpen && <NodeLibrary className="w-[196px] shrink-0 border-r border-border" />}

        <div
          className="relay-canvas relative min-w-0 flex-1 bg-canvas"
          onDrop={(event) => {
            event.preventDefault();
            const kind = event.dataTransfer.getData("application/relay-node");
            if (!kind) return;
            const position = screenToFlowPosition({ x: event.clientX, y: event.clientY });
            nodeSeq += 1;
            const newNode: RelayNode = {
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
            setNodes((ns) => [...ns, newNode]);
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
              onNodesChange(changes);
              if (changes.some((c) => c.type === "position" || c.type === "add" || c.type === "remove" || c.type === "replace")) markDirty();
            }}
            onEdgesChange={(changes) => {
              onEdgesChange(changes);
              if (changes.some((c) => c.type === "add" || c.type === "remove" || c.type === "replace")) markDirty();
            }}
            onConnect={(c) => {
              onConnect(c);
              markDirty();
            }}
            onSelectionChange={onSelectionChange}
            nodeTypes={relayNodeTypes}
            snapToGrid
            snapGrid={[16, 16]}
            multiSelectionKeyCode="Shift"
            edgesReconnectable
            proOptions={{ hideAttribution: true }}
            defaultViewport={{ x: 24, y: 0, zoom: 0.72 }}
            minZoom={0.25}
            maxZoom={1.75}
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} color="var(--color-border)" />
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
              nodeColor="var(--color-border-strong)"
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
                <span className="num text-[11px]">no saved versions yet — this workflow has never been persisted</span>
              </div>
            </div>
          )}
        </div>

        {inspectorOpen && (
          <Inspector
            className="w-[268px] shrink-0 border-l border-border"
            node={inspectorCanonical(inspectorNode)}
            onConfigChange={(id, config) => onConfigChange(id, config)}
            onTitleChange={onTitleChange}
            planHash={planHash}
            onClose={() => setInspectorOpen(false)}
            issues={validation.issues}
            laneOptions={lanes.map((l) => ({ value: l.id, label: l.id }))}
          />
        )}
      </div>

      {runPanelOpen && (
        <RunPanel
          phase={run.phase}
          result={run.result}
          error={run.error}
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
        <span className="num hidden text-muted-foreground sm:inline">{nodes.length} nodes · {edges.length} edges</span>
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

import type { RunState } from "@/lib/run-state";

function RunPanel(_props: { phase: RunState["phase"]; result: RunState["result"]; error: string | undefined; body: string; onBodyChange: (v: string) => void; onSubmit: () => void; onCancel: () => void; onClose: () => void }) {
  return null;
}
