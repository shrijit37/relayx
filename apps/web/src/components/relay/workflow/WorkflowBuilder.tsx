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
import { relayNodeTypes, type RelayNode } from "./nodes";
import { defaultEdges, defaultNodes } from "./graph";
import { deserializeWorkflow, serializeWorkflow } from "@/lib/workflow-serializer";
import { createWorkflow, saveWorkflowVersion } from "@/lib/api";
import {
  usePublishWorkflow,
  useRunWorkflowMutation,
  useSaveWorkflowMutation,
  useValidateMutation,
  useWorkflowLatestVersion,
} from "@/lib/use-workflow-publication";
import { runReducer, type RunAction, type RunState } from "@/lib/run-state";

let nodeSeq = 0;

function Toolbar({
  running,
  workflowId,
  workflowName,
  version,
  planHash,
  onRun,
  onStop,
  onSave,
  onValidate,
  onPublish,
  libraryOpen,
  inspectorOpen,
  toggleLibrary,
  toggleInspector,
}: {
  running: boolean;
  workflowId: string;
  workflowName: string;
  version: number | null;
  planHash: string | null;
  onRun: () => void;
  onStop: () => void;
  onSave: () => void;
  onValidate: () => void;
  onPublish: () => void;
  libraryOpen: boolean;
  inspectorOpen: boolean;
  toggleLibrary: () => void;
  toggleInspector: () => void;
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
        {planHash !== null && (
          <span className="flex items-center gap-1.5 rounded-sm border border-ok/30 bg-ok/10 px-1.5 py-0.5 text-[10px] text-ok">
            <StatusDot status="healthy" /> Valid
          </span>
        )}
        {workflowId !== "new" && (
          <span className="num hidden rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground lg:inline">
            {workflowId}
          </span>
        )}
      </div>

      <div className="ml-auto flex items-center gap-1">
        <TButton icon={Undo2} label="Undo" />
        <TButton icon={Redo2} label="Redo" />
        <span className="mx-1 h-5 w-px bg-border" />
        <TButton icon={ShieldCheck} label="Validate" text="Validate" onClick={onValidate} />
        <TButton icon={Save} label="Save" text="Save" onClick={onSave} />
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

function TButton({
  icon: Icon,
  label,
  text,
  onClick,
}: {
  icon: React.ElementType;
  label: string;
  text?: string;
  onClick?: () => void;
}) {
  return (
    <button
      aria-label={label}
      onClick={onClick}
      className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-transparent px-2 text-xs text-muted-foreground hover:border-border hover:text-foreground"
    >
      <Icon className="size-3.5" />
      {text ? <span className="hidden lg:inline">{text}</span> : null}
    </button>
  );
}

/**
 * The Run panel — a real execution surface. Every displayed value is
 * backend-truth: the request-body textarea is user input, the phase comes
 * from the run-state machine, and the result/error/metadata come from the
 * control-plane run envelope. Nothing here is simulated (Phase 6.5 §3.1).
 */
function RunPanel({
  phase,
  result,
  error = undefined,
  body,
  onBodyChange,
  onSubmit,
  onCancel,
  onClose,
}: {
  phase: RunState["phase"];
  result: RunState["result"];
  error: string | undefined;
  body: string;
  onBodyChange: (v: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
  onClose: () => void;
}) {
  const running = phase === "running";
  return (
    <div className="flex shrink-0 items-start gap-3 border-t border-border bg-panel px-3 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="label-xs">Request body</span>
          <span className="num text-[10px] text-muted-foreground">executes the published ACTIVE version</span>
        </div>
        <textarea
          value={body}
          onChange={(e) => onBodyChange(e.target.value)}
          rows={3}
          spellCheck={false}
          disabled={running}
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
          <div className="text-[11px] text-muted-foreground">Run cancelled by the user.</div>
        ) : phase === "running" ? (
          <div className="flex items-center gap-1.5 text-[11px] text-info">
            <StatusDot status="running" />
            executing on the gateway…
          </div>
        ) : (
          <div className="text-[11px] text-muted-foreground">Not started.</div>
        )}
      </div>

      <div className="flex min-w-0 max-w-[50%] flex-1 flex-col gap-1.5">
        <div className="label-xs">Output</div>
        <pre className="num max-h-[120px] min-h-0 flex-1 overflow-auto rounded-sm border border-border bg-canvas px-2 py-1.5 font-mono text-[11px]">
          {phase === "completed" && result ? JSON.stringify(result.output, null, 2) : "—"}
        </pre>
      </div>

      <div className="flex shrink-0 flex-col gap-1.5">
        <button
          onClick={running ? onCancel : onSubmit}
          className={cn(
            "focus-ring flex h-7 items-center justify-center gap-1.5 rounded-sm px-2.5 text-xs font-medium",
            running ? "bg-fail/15 text-fail hover:bg-fail/25" : "bg-primary text-primary-foreground hover:opacity-90",
          )}
        >
          {running ? <Square className="size-3" /> : <Play className="size-3" />}
          {running ? "Abort" : "Run"}
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

function Canvas({ workflowId }: { workflowId: string }) {
  const [nodes, setNodes, onNodesChange] = useNodesState<RelayNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [selected, setSelected] = useState<RelayNode | null>(null);
  const [libraryOpen, setLibraryOpen] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [running, setRunning] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [workflowName, setWorkflowName] = useState("Unnamed workflow");
  const [version, setVersion] = useState<number | null>(null);
  const [planHash, setPlanHash] = useState<string | null>(null);
  const [noSavedVersions, setNoSavedVersions] = useState(false);
  const { screenToFlowPosition } = useReactFlow();

  const publish = usePublishWorkflow();
  const saveMutation = useSaveWorkflowMutation(workflowId);
  const validateMutation = useValidateMutation();
  const { data: latest, isPending: latestPending } = useWorkflowLatestVersion(workflowId);

  // Load the workflow once on mount.
  useEffect(() => {
    if (workflowId === "new") {
      setNodes(defaultNodes);
      setEdges(defaultEdges);
      setVersion(1);
      setWorkflowName("New workflow");
      setNoSavedVersions(true);
    } else if (latest) {
      const result = deserializeWorkflow(latest.workflow_json);
      if (result.warnings.length > 0) {
        toast.warning(`Loaded with warnings: ${result.warnings.join("; ")}`);
      }
      setNodes(result.nodes);
      setEdges(result.edges);
      setVersion(latest.version);
      setWorkflowName(latest.workflow_json.name);
      setNoSavedVersions(false);
    }
  }, [latest, workflowId, setNodes, setEdges]);

  const serializeTo = useCallback(
    (idOverride?: string) => {
      const meta = {
        id: idOverride ?? workflowId,
        name: workflowName,
        version: version === null ? 1 : version,
      };
      return serializeWorkflow(nodes, edges, meta);
    },
    [workflowId, workflowName, version, nodes, edges],
  );

  const navigate = useNavigate();

  // Save in "new" mode creates the real workflow row first (same control-plane
  // POST as the workflow list's Create button), persists the canvas as its
  // first immutable version, then navigates to the durable id so the editor
  // reloads from the backend (§4 — no fabricated local persistence).
  const createAndSave = useCallback(async () => {
    const result = serializeTo();
    if (!result.workflow) {
      toast.error(`Cannot save — ${result.errors.join("; ")}`);
      return;
    }
    const name = result.workflow.name || "New workflow";
    try {
      const row = await createWorkflow(name);
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
    if (result.warnings.length > 0) {
      toast.warning(`Publishing with warnings: ${result.warnings.join("; ")}`);
    }
    publish.mutate(
      { workflow: result.workflow, lanes: result.lanes },
      {
        onSuccess: (info) => toast.success(`Published v${info.version} · ${info.workflowId}`),
        onError: (err) => toast.error(`Publish failed — ${err.message}`),
      },
    );
  }, [workflowId, serializeTo, publish]);

  const onConnect = useCallback(
    (c: Connection) => setEdges((eds) => addEdge({ ...c }, eds)),
    [setEdges],
  );

  const onSelectionChange = useCallback((p: OnSelectionChangeParams) => {
    setSelected((p.nodes[0] as RelayNode) ?? null);
  }, []);

  // ── Real Run ─────────────────────────────────────────────────────────
  // The execution-state machine is driven exclusively by real mutation
  // outcomes: submitted → running, resolved → completed (real envelope),
  // rejected → failed (real backend error), aborted → cancelled. There is no
  // fake progress, timing, or success path (Phase 6.5 §3.1/§6).
  const [run, setRun] = useState<RunState>({ phase: "idle" });
  const [runBody, setRunBody] = useState(
    JSON.stringify({ messages: [{ role: "user", content: "hello from the run panel" }] }, null, 2),
  );
  const [runPanelOpen, setRunPanelOpen] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  const runMutation = useRunWorkflowMutation();

  const runDispatch = useCallback((action: RunAction) => {
    setRun((s) => runReducer(s, action));
  }, []);

  const submitRun = useCallback(() => {
    if (workflowId === "new") {
      toast.error("Cannot run an unsaved workflow — save and publish it first.");
      return;
    }
    if (dirty) {
      toast.warning("Run executes the published ACTIVE version — unsaved changes won't be executed. Save & publish to run them.");
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
    setRunning(true);
    runDispatch({ type: "start" });
    setRunPanelOpen(true);
    runMutation.mutate(
      { workflowId, body, signal: controller.signal },
      {
        onSuccess: (res) => {
          setRunning(false);
          runDispatch({
            type: "completed",
            result: {
              requestId: res.request_id,
              workflowId: res.workflow_id,
              workflowVersion: res.workflow_version,
              snapshotVersion: res.snapshot_version,
              planHash: res.plan_hash,
              output: res.output,
            },
          });
        },
        onError: (err) => {
          setRunning(false);
          runDispatch({ type: "failed", error: err.message });
          toast.error(`Run failed — ${err.message}`);
        },
      },
    );
  }, [workflowId, dirty, runBody, runDispatch, runMutation]);

  const cancelRun = useCallback(() => {
    abortRef.current?.abort();
    abortRef.current = null;
    setRunning(false);
    runDispatch({ type: "cancel" });
  }, [runDispatch]);

  const markDirty = useCallback(() => {
    setDirty(true);
    setPlanHash(null);
  }, []);

  const onDrop = useCallback(
    (event: React.DragEvent) => {
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
          title: "Unconfigured",
          lines: ["no configuration", "not compiled into plan"],
          metaLeft: "draft",
          status: "idle",
        },
      };
      setNodes((ns) => [...ns, newNode]);
      markDirty();
    },
    [screenToFlowPosition, setNodes, markDirty],
  );

  const inspectorNode = useMemo(() => nodes.find((n) => n.id === selected?.id), [nodes, selected]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Toolbar
        running={running}
        workflowId={workflowId}
        workflowName={workflowName}
        version={version}
        planHash={planHash}
        onRun={() => setRunPanelOpen((o) => !o)}
        onStop={cancelRun}
        onSave={onSave}
        onValidate={onValidate}
        onPublish={onPublish}
        libraryOpen={libraryOpen}
        inspectorOpen={inspectorOpen}
        toggleLibrary={() => setLibraryOpen((o) => !o)}
        toggleInspector={() => setInspectorOpen((o) => !o)}
      />

      <div className="flex min-h-0 flex-1">
        {libraryOpen && (
          <NodeLibrary className="hidden w-[196px] shrink-0 border-r border-border lg:flex" />
        )}

        <div
          className="relay-canvas relative min-w-0 flex-1 bg-canvas"
          onDrop={onDrop}
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
              if (changes.some((c) => c.type === "position" || c.type === "add" || c.type === "remove" || c.type === "replace")) {
                markDirty();
              }
            }}
            onEdgesChange={(changes) => {
              onEdgesChange(changes);
              if (changes.some((c) => c.type === "add" || c.type === "remove" || c.type === "replace")) {
                markDirty();
              }
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
            className="hidden w-[268px] shrink-0 border-l border-border xl:flex"
            data={inspectorNode?.data ?? undefined}
            nodeId={inspectorNode?.id ?? undefined}
            planHash={planHash}
            onClose={() => setInspectorOpen(false)}
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
            runDispatch({ type: "reset" });
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
        <span className="h-4 w-px bg-border sm:inline" />
        <span className="num hidden text-muted-foreground md:inline">
          {planHash ? `plan ${planHash.slice(0, 8)}` : "not compiled"}
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