import { AlertTriangle, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { KV, SectionLabel, StatusText, Tag } from "../primitives";
import { nodeMeta, type RelayNodeData } from "./nodes";

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="border-b border-border px-3 py-2.5 last:border-b-0">
      <SectionLabel>{label}</SectionLabel>
      <div className="mt-1.5">{children}</div>
    </div>
  );
}

export function Inspector({
  data,
  nodeId,
  onClose,
  className,
  planHash,
}: {
  data?: RelayNodeData | undefined;
  nodeId?: string | undefined;
  onClose?: (() => void) | undefined;
  className?: string | undefined;
  planHash?: string | null;
}) {
  if (!data) {
    return (
      <aside className={cn("flex min-h-0 flex-col bg-panel", className)}>
        <div className="hairline-b flex h-9 items-center px-3">
          <span className="label-xs">Inspector</span>
        </div>
        <div className="flex flex-1 items-center justify-center p-6 text-center">
          <p className="max-w-[190px] text-[11px] leading-relaxed text-muted-foreground">
            Select a node to inspect its serialized configuration. Runtime health/per-node metrics are not shown —
            there is no per-node telemetry backend yet.
          </p>
        </div>
      </aside>
    );
  }

  const meta = nodeMeta[data.kind];
  const Icon = meta.icon;

  return (
    <aside className={cn("flex min-h-0 flex-col bg-panel", className)}>
      <div className="hairline-b flex h-9 items-center gap-2 px-3">
        <Icon className={cn("size-3.5", meta.accent)} />
        <span className="label-xs flex-1 truncate">{meta.label}</span>
        {onClose ? (
          <button onClick={onClose} className="focus-ring text-muted-foreground hover:text-foreground">
            <X className="size-3.5" />
          </button>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b border-border px-3 py-2.5">
          <div className="text-sm font-medium">{data.title}</div>
          <div className="num mt-0.5 text-[10px] text-muted-foreground">node_id: {nodeId}</div>
        </div>

        {data.status ? (
          <Group label="Status">
            <StatusText status={data.status} />
          </Group>
        ) : null}

        {data.issue ? (
          <div
            className={cn(
              "flex gap-2 border-b border-border px-3 py-2.5",
              data.issue === "error" ? "bg-fail/8" : "bg-warn/8",
            )}
          >
            <AlertTriangle className={cn("mt-px size-3.5 shrink-0", data.issue === "error" ? "text-fail" : "text-warn")} />
            <div>
              <div className={cn("text-xs font-medium", data.issue === "error" ? "text-fail" : "text-warn")}>
                {data.issue === "error" ? "Configuration error" : "Warning"}
              </div>
              <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                {data.issue === "error"
                  ? "This node may not compile into a valid plan — validate before publishing."
                  : "This node serializes with a warning."}
              </p>
            </div>
          </div>
        ) : null}

        <Group label="Configuration">
          <div className="space-y-0.5">
            {data.lines.map((l) => (
              <div key={l} className="num text-[11px] text-muted-foreground">
                {l}
              </div>
            ))}
            {data.metaLeft ? <div className="num text-[10px] text-muted-foreground">kind: {data.metaLeft}</div> : null}
          </div>
        </Group>

        {data.kind === "mcp" || data.kind === "skill" || data.kind === "tool" || data.kind === "agent" || data.kind === "policy" || data.kind === "observability" ? (
          <Group label="Runtime">
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              {data.kind === "mcp"
                ? "MCP execution is Phase 7 — this node is display-only until the MCP backend ships."
                : data.kind === "skill"
                  ? "Skill loading is Phase 7 — this node is display-only until the Skill backend ships."
                  : "This node kind is not executed by the runtime yet."}
            </p>
          </Group>
        ) : null}

        {data.badges?.length ? (
          <Group label="Capabilities">
            <div className="flex flex-wrap gap-1">
              {data.badges.map((b) => (
                <Tag key={b}>{b}</Tag>
              ))}
            </div>
            <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
              Capability matrices are not audited by a backend yet — these are editor-side labels.
            </p>
          </Group>
        ) : null}

        <Group label="Execution">
          <KV k="Plan hash" v={planHash ? planHash.slice(0, 12) : "not compiled"} mono={false} tone={planHash ? "ok" : "neutral"} />
          <KV k="Served by" v="Rust gateway data plane" />
        </Group>
      </div>

      <div className="border-t border-border p-2">
        <button className="focus-ring h-7 w-full rounded-sm border border-border-strong bg-panel-raised text-xs font-medium hover:border-primary hover:text-primary">
          Edit {meta.label.toLowerCase()}
        </button>
      </div>
    </aside>
  );
}