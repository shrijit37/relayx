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
} from "lucide-react";
import { cn } from "@/lib/utils";
import { validationIssues } from "@/lib/relay-data";
import { StatusDot } from "../primitives";
import { Inspector } from "./Inspector";
import { NodeLibrary } from "./NodeLibrary";
import { relayNodeTypes, type RelayNode, type RunState } from "./nodes";
import { executionEdges, executionPath, initialEdges, initialNodes } from "./graph";

let nodeSeq = 0;

function Toolbar({
  running,
  onRun,
  onStop,
  libraryOpen,
  inspectorOpen,
  toggleLibrary,
  toggleInspector,
}: {
  running: boolean;
  onRun: () => void;
  onStop: () => void;
  libraryOpen: boolean;
  inspectorOpen: boolean;
  toggleLibrary: () => void;
  toggleInspector: () => void;
}) {
  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-panel px-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate text-xs font-semibold">Production Gateway</span>
        <span className="num rounded-sm border border-border bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
          v24
        </span>
        <span className="flex items-center gap-1.5 rounded-sm border border-ok/30 bg-ok/10 px-1.5 py-0.5 text-[10px] text-ok">
          <StatusDot status="healthy" /> Valid · Deployed
        </span>
        <span className="num hidden rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground lg:inline">
          Production
        </span>
      </div>

      <div className="ml-auto flex items-center gap-1">
        <TButton icon={Undo2} label="Undo" />
        <TButton icon={Redo2} label="Redo" />
        <span className="mx-1 h-5 w-px bg-border" />
        <TButton icon={ShieldCheck} label="Validate" text="Validate" />
        <TButton icon={Save} label="Save" text="Save" />
        <Link
          to="/workflows/$workflowId/versions"
          params={{ workflowId: "production-gateway" }}
          className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border px-2 text-xs hover:border-border-strong"
        >
          <History className="size-3.5 text-muted-foreground" />
          <span className="hidden lg:inline">Versions</span>
        </Link>
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
        <button className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-border-strong px-2 text-xs font-medium hover:border-primary hover:text-primary">
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
}: {
  icon: React.ElementType;
  label: string;
  text?: string;
}) {
  return (
    <button
      aria-label={label}
      className="focus-ring flex h-7 items-center gap-1.5 rounded-sm border border-transparent px-2 text-xs text-muted-foreground hover:border-border hover:text-foreground"
    >
      <Icon className="size-3.5" />
      {text ? <span className="hidden lg:inline">{text}</span> : null}
    </button>
  );
}

function Canvas() {
  const [nodes, setNodes, onNodesChange] = useNodesState<RelayNode>(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>(initialEdges);
  const [selected, setSelected] = useState<RelayNode | null>(null);
  const [libraryOpen, setLibraryOpen] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [running, setRunning] = useState(false);
  const [step, setStep] = useState(-1);
  const timers = useRef<number[]>([]);
  const { screenToFlowPosition } = useReactFlow();

  const onConnect = useCallback(
    (c: Connection) => setEdges((eds) => addEdge({ ...c }, eds)),
    [setEdges],
  );

  const onSelectionChange = useCallback((p: OnSelectionChangeParams) => {
    setSelected((p.nodes[0] as RelayNode) ?? null);
  }, []);

  const clearTimers = () => {
    timers.current.forEach((t) => window.clearTimeout(t));
    timers.current = [];
  };

  const applyRunStates = useCallback(
    (states: Record<string, RunState>) => {
      setNodes((ns) =>
        ns.map((n) => ({ ...n, data: { ...n.data, runState: states[n.id] ?? "idle" } })) as RelayNode[],
      );
    },
    [setNodes],
  );

  const stop = useCallback(() => {
    clearTimers();
    setRunning(false);
    setStep(-1);
    applyRunStates({});
    setEdges((es) => es.map((e) => ({ ...e, className: e.className?.replace("edge-active", "").trim() ?? "" })));
  }, [applyRunStates, setEdges]);

  const run = useCallback(() => {
    clearTimers();
    setRunning(true);
    const states: Record<string, RunState> = {};
    executionPath.forEach((p) => (states[p.id] = "queued"));
    applyRunStates({ ...states });

    executionPath.forEach((p, i) => {
      const t = window.setTimeout(() => {
        setStep(i);
        executionPath.slice(0, i).forEach((prev) => (states[prev.id] = "completed"));
        states[p.id] = p.id === "output" ? "streaming" : "running";
        applyRunStates({ ...states });
        setEdges((es) =>
          es.map((e) => ({
            ...e,
            className: executionEdges.slice(0, i + 1).includes(e.id)
              ? cn(e.className?.replace("edge-active", ""), "edge-active")
              : (e.className?.replace("edge-active", "").trim() ?? ""),
          })),
        );
      }, 500 + i * 620);
      timers.current.push(t);
    });

    const done = window.setTimeout(
      () => {
        executionPath.forEach((p) => (states[p.id] = "completed"));
        applyRunStates({ ...states });
        setRunning(false);
      },
      500 + executionPath.length * 620 + 700,
    );
    timers.current.push(done);
  }, [applyRunStates, setEdges]);

  useEffect(() => clearTimers, []);

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
    },
    [screenToFlowPosition, setNodes],
  );

  const errorCount = validationIssues.filter((i) => i.level === "error").length;
  const warnCount = validationIssues.filter((i) => i.level === "warn").length;

  const currentStep = step >= 0 ? executionPath[step] : undefined;

  const inspectorNode = useMemo(() => nodes.find((n) => n.id === selected?.id), [nodes, selected]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Toolbar
        running={running}
        onRun={run}
        onStop={stop}
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
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
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

          {running || currentStep ? (
            <div className="pointer-events-none absolute top-2 left-1/2 z-10 -translate-x-1/2">
              <div className="flex items-center gap-2 rounded-sm border border-border bg-panel/95 px-2.5 py-1.5 shadow-panel">
                <StatusDot status={running ? "running" : "completed"} />
                <span className="num text-[11px]">
                  {running ? `Executing ${currentStep?.label ?? "…"}` : "Run completed"}
                </span>
                <span className="num text-[11px] text-muted-foreground">{currentStep?.ms}</span>
                <Link
                  to="/runs/$runId"
                  params={{ runId: "8F31A2" }}
                  className="pointer-events-auto num text-[11px] text-primary hover:underline"
                >
                  open run →
                </Link>
              </div>
            </div>
          ) : null}
        </div>

        {inspectorOpen && (
          <Inspector
            className="hidden w-[268px] shrink-0 border-l border-border xl:flex"
            data={inspectorNode?.data ?? undefined}
            nodeId={inspectorNode?.id ?? undefined}
            onClose={() => setInspectorOpen(false)}
          />
        )}
      </div>

      <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-border bg-panel px-3 text-[11px]">
        <span className="flex items-center gap-1.5 text-fail">
          <CircleSlash className="size-3" />
          <span className="num">{errorCount} errors</span>
        </span>
        <span className="flex items-center gap-1.5 text-warn">
          <AlertTriangle className="size-3" />
          <span className="num">{warnCount} warnings</span>
        </span>
        <span className="h-4 w-px bg-border" />
        <span className="num hidden text-muted-foreground sm:inline">{nodes.length} nodes · {edges.length} edges</span>
        <span className="h-4 w-px bg-border sm:inline" />
        <span className="num hidden text-muted-foreground md:inline">plan_8f31a2 · compiled 14.2 KB</span>
        <span className="ml-auto flex items-center gap-3">
          <span className="num hidden text-muted-foreground lg:inline">snap 16px</span>
          <span className="num flex items-center gap-1.5 text-ok">
            <Check className="size-3" /> saved 12s ago
          </span>
        </span>
      </footer>
    </div>
  );
}

export function WorkflowBuilder() {
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
        <Canvas />
      </div>
    </ReactFlowProvider>
  );
}
