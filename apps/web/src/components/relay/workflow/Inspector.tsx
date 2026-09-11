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
}: {
  data?: RelayNodeData | undefined;
  nodeId?: string | undefined;
  onClose?: (() => void) | undefined;
  className?: string | undefined;
}) {
  if (!data) {
    return (
      <aside className={cn("flex min-h-0 flex-col bg-panel", className)}>
        <div className="hairline-b flex h-9 items-center px-3">
          <span className="label-xs">Inspector</span>
        </div>
        <div className="flex flex-1 items-center justify-center p-6 text-center">
          <p className="max-w-[190px] text-[11px] leading-relaxed text-muted-foreground">
            Select a node to inspect its compiled configuration, capabilities and health.
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
                {data.issue === "error" ? "Unauthorized tool" : "Capability mismatch"}
              </div>
              <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                {data.issue === "error"
                  ? "Workflow policy deny-write-tools denies github.write_issue. Remove the tool or add an explicit ALLOW rule."
                  : "Fallback target OpenAI does not support deferred tool references; the deferred set is dropped on failover."}
              </p>
            </div>
          </div>
        ) : null}

        {data.kind === "lane" && <LaneBody title={data.title} />}
        {data.kind === "mcp" && <McpBody />}
        {data.kind === "skill" && <SkillBody />}
        {data.kind === "provider" && <ProviderBody />}
        {data.kind === "route" && <RouteBody />}

        {!["lane", "mcp", "skill", "provider", "route"].includes(data.kind) && (
          <Group label="Configuration">
            <div className="space-y-0.5">
              {data.lines.map((l) => (
                <div key={l} className="num text-[11px] text-muted-foreground">
                  {l}
                </div>
              ))}
            </div>
          </Group>
        )}

        {data.badges?.length ? (
          <Group label="Capabilities">
            <div className="flex flex-wrap gap-1">
              {data.badges.map((b) => (
                <Tag key={b}>{b}</Tag>
              ))}
            </div>
          </Group>
        ) : null}

        <Group label="Execution plan">
          <KV k="Compiled into" v="plan_8f31a2" />
          <KV k="Plan version" v="v24" />
          <KV k="Hot path" v={data.kind === "policy" ? "no (control plane)" : "yes (data plane)"} />
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

function LaneBody({ title }: { title: string }) {
  const eu = title.includes("eu");
  return (
    <>
      <Group label="Provider">
        <KV k="Adapter" v="Anthropic" />
        <KV k="Endpoint" v="api.anthropic.com" />
      </Group>
      <Group label="Network">
        <KV k="Route" v={eu ? "WireGuard EU-03" : "WireGuard US-01"} />
        <KV k="Region" v={eu ? "EU-Frankfurt" : "US-East"} />
        <KV k="Egress IP" v={eu ? "185.42.19.7" : "44.201.66.8"} />
      </Group>
      <Group label="Policy">
        <KV k="Bound policy" v={eu ? "eu-residency" : "production-standard"} />
        <KV k="Evaluation" v="compile-time" />
      </Group>
      <Group label="Connection pool">
        <KV k="Mode" v="warm" />
        <KV k="Connections" v={eu ? "24" : "48"} />
        <KV k="Reuse" v={eu ? "91.0%" : "94.2%"} tone="ok" />
      </Group>
      <Group label="Health">
        <KV k="Latency" v={eu ? "119 ms" : "82 ms"} />
        <KV k="Error rate" v={eu ? "0.07%" : "0.04%"} tone="ok" />
        <KV k="Last probe" v="4 s ago" />
      </Group>
    </>
  );
}

function McpBody() {
  return (
    <>
      <Group label="Query">
        <div className="num rounded-sm border border-border bg-canvas px-2 py-1.5 text-[11px]">github pull request</div>
      </Group>
      <Group label="Progressive discovery">
        <ol className="space-y-1.5">
          {[
            ["Discovery", "12 candidates", "ok"],
            ["Policy filter", "3 permitted", "ok"],
            ["Activation", "1 selected", "info"],
            ["Execution", "not yet invoked", "neutral"],
          ].map(([step, val, tone]) => (
            <li key={step} className="flex items-center gap-2">
              <span className={cn("h-3 w-[2px] rounded", tone === "ok" ? "bg-ok" : tone === "info" ? "bg-info" : "bg-border-strong")} />
              <span className="text-[11px]">{step}</span>
              <span className="num ml-auto text-[11px] text-muted-foreground">{val}</span>
            </li>
          ))}
        </ol>
      </Group>
      <Group label="Index">
        <KV k="Registered" v="1,842" />
        <KV k="Cache" v="HIT" tone="ok" />
        <KV k="Retrieval" v="1.8 ms" />
      </Group>
      <Group label="Activated tools">
        <div className="flex flex-wrap gap-1">
          <Tag tone="ok">github.read_pr</Tag>
          <Tag tone="fail">github.write_issue · denied</Tag>
        </div>
      </Group>
    </>
  );
}

function SkillBody() {
  return (
    <>
      <Group label="Progressive load">
        <KV k="Metadata" v="loaded · 1.2 KB" tone="ok" />
        <KV k="Instructions" v="loaded · 16.8 KB" tone="ok" />
        <KV k="References" v="2 deferred · 412 KB" />
        <KV k="Scripts" v="1 available" />
      </Group>
      <Group label="Context budget">
        <KV k="In context" v="18 KB" />
        <KV k="Deferred" v="412 KB" />
        <KV k="Fetch mode" v="on demand" />
      </Group>
    </>
  );
}

function ProviderBody() {
  return (
    <>
      <Group label="Adapter">
        <KV k="Protocol" v="Messages API v1" />
        <KV k="Model" v="claude-sonnet-4.5" />
        <KV k="Endpoint" v="api.anthropic.com" />
      </Group>
      <Group label="Measured latency">
        <KV k="Gateway overhead" v="3.4 ms" tone="ok" />
        <KV k="Upstream TTFB" v="420 ms" />
        <KV k="Stream duration" v="1.39 s" />
      </Group>
    </>
  );
}

function RouteBody() {
  return (
    <>
      <Group label="Match">
        <KV k="Pattern" v="claude-*" />
        <KV k="Strategy" v="latency-aware" />
        <KV k="Tie-break" v="reuse ratio" />
      </Group>
      <Group label="Candidates">
        {([
          ["anthropic-us-vpn", "82 ms"],
          ["anthropic-eu-vpn", "119 ms"],
          ["bedrock-us", "168 ms"],
          ["openai-direct", "104 ms"],
        ] as [string, string][]).map(([n, l]) => (
          <KV key={n} k={n} v={l} />
        ))}
      </Group>
    </>
  );
}
