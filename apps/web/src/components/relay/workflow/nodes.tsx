import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import {
  AlertTriangle,
  ArrowRightLeft,
  Boxes,
  GitFork,
  Globe,
  LogIn,
  Radar,
  Radio,
  RefreshCw,
  Route,
  ShieldCheck,
  Signal,
  Sparkles,
  Split,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { StatusDot } from "../primitives";

export type RunState = "idle" | "queued" | "running" | "streaming" | "completed" | "failed";

export type RelayNodeData = {
  kind: NodeKind;
  title: string;
  lines: string[];
  badges?: string[];
  status?: string;
  metaLeft?: string;
  metaRight?: string;
  issue?: "error" | "warn";
  runState?: RunState;
  branches?: string[];
};

export type NodeKind =
  | "input"
  | "output"
  | "transform"
  | "condition"
  | "route"
  | "lane"
  | "fallback"
  | "retry"
  | "provider"
  | "endpoint"
  | "mcp"
  | "tool"
  | "skill"
  | "agent"
  | "policy"
  | "observability";

export const nodeMeta: Record<NodeKind, { label: string; icon: LucideIcon; accent: string }> = {
  input: { label: "Input", icon: LogIn, accent: "text-info" },
  output: { label: "Output", icon: Radio, accent: "text-info" },
  transform: { label: "Transform", icon: ArrowRightLeft, accent: "text-primary" },
  condition: { label: "Condition", icon: Split, accent: "text-warn" },
  route: { label: "Model Router", icon: Route, accent: "text-primary" },
  lane: { label: "Lane", icon: Globe, accent: "text-ok" },
  fallback: { label: "Fallback", icon: GitFork, accent: "text-warn" },
  retry: { label: "Retry", icon: RefreshCw, accent: "text-warn" },
  provider: { label: "Provider", icon: Boxes, accent: "text-violet" },
  endpoint: { label: "Endpoint", icon: Signal, accent: "text-violet" },
  mcp: { label: "MCP Discovery", icon: Radar, accent: "text-violet" },
  tool: { label: "Tool Activation", icon: ShieldCheck, accent: "text-violet" },
  skill: { label: "Skill", icon: Sparkles, accent: "text-violet" },
  agent: { label: "Agent / Model", icon: Boxes, accent: "text-violet" },
  policy: { label: "Policy", icon: ShieldCheck, accent: "text-ok" },
  observability: { label: "Observability", icon: Signal, accent: "text-info" },
};

const runStateStyles: Record<RunState, string> = {
  idle: "",
  queued: "border-border-strong",
  running: "border-info shadow-[0_0_0_1px_var(--color-info)]",
  streaming: "border-info shadow-[0_0_0_1px_var(--color-info)]",
  completed: "border-ok/60",
  failed: "border-fail shadow-[0_0_0_1px_var(--color-fail)]",
};

export type RelayNode = Node<RelayNodeData, "relay">;

export function RelayFlowNode({ data, selected }: NodeProps<RelayNode>) {
  const meta = nodeMeta[data.kind];
  const Icon = meta.icon;
  const run = data.runState ?? "idle";

  return (
    <div
      className={cn(
        "w-[228px] rounded-md border border-border bg-panel shadow-node",
        run !== "idle" && runStateStyles[run],
        data.issue === "error" && "border-fail/70",
        data.issue === "warn" && "border-warn/60",
        selected && "border-primary",
      )}
    >
      <Handle type="target" position={Position.Left} />

      <div className="hairline-b flex h-7 items-center gap-1.5 px-2">
        <Icon className={cn("size-3.5", meta.accent)} />
        <span className="label-xs truncate">{meta.label}</span>
        {run !== "idle" && (
          <span
            className={cn(
              "num ml-auto text-[9px] tracking-wide uppercase",
              run === "failed" ? "text-fail" : run === "completed" ? "text-ok" : "text-info",
            )}
          >
            {run}
          </span>
        )}
        {run === "idle" && data.issue && (
          <AlertTriangle className={cn("ml-auto size-3", data.issue === "error" ? "text-fail" : "text-warn")} />
        )}
      </div>

      <div className="px-2 py-2">
        <div className="truncate text-xs font-medium">{data.title}</div>
        <div className="mt-1 space-y-0.5">
          {data.lines.map((l) => (
            <div key={l} className="num truncate text-[11px] text-muted-foreground">
              {l}
            </div>
          ))}
        </div>
        {data.badges?.length ? (
          <div className="mt-1.5 flex flex-wrap gap-1">
            {data.badges.map((b) => (
              <span
                key={b}
                className="rounded-[3px] border border-border bg-muted px-1 py-px text-[9px] text-muted-foreground"
              >
                {b}
              </span>
            ))}
          </div>
        ) : null}
      </div>

      {(data.metaLeft || data.metaRight || data.status) && (
        <div className="flex h-6 items-center justify-between border-t border-border px-2">
          <span className="num text-[10px] text-muted-foreground">{data.metaLeft}</span>
          <span className="flex items-center gap-1.5">
            {data.metaRight ? <span className="num text-[10px] text-muted-foreground">{data.metaRight}</span> : null}
            {data.status ? (
              <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
                <StatusDot status={data.status} />
                {data.status}
              </span>
            ) : null}
          </span>
        </div>
      )}

      {data.branches?.length ? (
        <>
          {data.branches.map((b, i) => (
            <div
              key={b}
              className="num absolute right-[-6px] text-[9px] text-muted-foreground"
              style={{ top: `${34 + i * 22}px`, transform: "translateX(100%)", paddingLeft: 8 }}
            >
              {b}
            </div>
          ))}
          {data.branches.map((b, i) => (
            <Handle
              key={`h-${b}`}
              id={b}
              type="source"
              position={Position.Right}
              style={{ top: 38 + i * 22 }}
            />
          ))}
        </>
      ) : (
        <Handle type="source" position={Position.Right} />
      )}
    </div>
  );
}

export const relayNodeTypes = { relay: RelayFlowNode };
