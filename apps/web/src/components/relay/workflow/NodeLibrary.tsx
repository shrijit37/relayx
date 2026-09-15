import { useState } from "react";
import { Search } from "lucide-react";
import { cn } from "@/lib/utils";
import { nodeMeta, type NodeKind } from "./nodes";

const categories: { name: string; items: { kind: NodeKind; label: string; hint: string }[] }[] = [
  {
    name: "Core",
    items: [
      { kind: "input", label: "Input", hint: "HTTP / protocol ingress" },
      { kind: "output", label: "Output", hint: "streaming or buffered" },
      { kind: "transform", label: "Transform", hint: "protocol translation" },
      { kind: "condition", label: "Condition", hint: "branch on signals" },
    ],
  },
  {
    name: "Routing",
    items: [
      { kind: "route", label: "Route", hint: "model / pattern router" },
      { kind: "lane", label: "Lane", hint: "provider + network + policy" },
      { kind: "fallback", label: "Fallback", hint: "ordered degradation" },
      { kind: "retry", label: "Retry", hint: "budgeted attempts" },
    ],
  },
  {
    name: "Providers",
    items: [
      { kind: "provider", label: "Provider", hint: "adapter + capabilities" },
      { kind: "endpoint", label: "Endpoint", hint: "regional host" },
    ],
  },
  {
    name: "Agent",
    items: [
      { kind: "mcp", label: "MCP Discovery", hint: "progressive candidates" },
      { kind: "tool", label: "Tool Activation", hint: "policy-gated exposure" },
      { kind: "skill", label: "Skill", hint: "deferred references" },
      { kind: "agent", label: "Agent / Model", hint: "loop + tool budget" },
    ],
  },
  {
    name: "Platform",
    items: [
      { kind: "policy", label: "Policy", hint: "deterministic matcher" },
      { kind: "observability", label: "Observability", hint: "traces + metrics" },
    ],
  },
];

export function NodeLibrary({ className, onAdd }: { className?: string; onAdd?: (kind: string) => void }) {
  const [q, setQ] = useState("");
  const query = q.trim().toLowerCase();

  const filtered = categories
    .map((c) => ({
      ...c,
      items: c.items.filter(
        (i) => !query || i.label.toLowerCase().includes(query) || i.hint.includes(query) || i.kind.includes(query),
      ),
    }))
    .filter((c) => c.items.length);

  return (
    <div className={cn("flex min-h-0 flex-col bg-panel", className)}>
      <div className="hairline-b flex h-9 items-center gap-2 px-2">
        <Search className="size-3.5 text-muted-foreground" />
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search nodes..."
          className="h-full w-full bg-transparent text-xs outline-none placeholder:text-muted-foreground"
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
        {filtered.map((c) => (
          <div key={c.name} className="mb-3">
            <div className="label-xs px-1 pb-1">{c.name}</div>
            <ul className="space-y-px">
              {c.items.map((i) => {
                const meta = nodeMeta[i.kind];
                const Icon = meta.icon;
                return (
                  <li key={i.kind}>
                    <div
                      draggable
                      onClick={() => onAdd?.(i.kind)}
                      onDragStart={(e) => {
                        e.dataTransfer.setData("application/relay-node", i.kind);
                        e.dataTransfer.effectAllowed = "move";
                      }}
                      className="group flex cursor-grab items-start gap-2 rounded-sm border border-transparent px-1.5 py-1.5 transition-colors hover:border-border hover:bg-panel-raised active:cursor-grabbing"
                    >
                      <Icon className={cn("mt-px size-3.5 shrink-0", meta.accent)} />
                      <div className="min-w-0">
                        <div className="truncate text-xs">{i.label}</div>
                        <div className="truncate text-[10px] text-muted-foreground">{i.hint}</div>
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
        {!filtered.length && (
          <p className="px-1 py-6 text-center text-[11px] text-muted-foreground">
            No node matches “{q}”.
          </p>
        )}
      </div>
      <div className="border-t border-border px-2 py-1.5 text-[10px] text-muted-foreground">
        Drag a node onto the canvas
      </div>
    </div>
  );
}
