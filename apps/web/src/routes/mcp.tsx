import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/relay/AppShell";
import { Metric, PageHeader, Panel, StatusText, Tag, TableShell, Td } from "@/components/relay/primitives";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mcpServers, mcpTools } from "@/lib/relay-data";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/mcp")({
  head: () => ({
    meta: [
      { title: "MCP / Tools — relay-x" },
      { name: "description", content: "Progressive MCP discovery: register, index, discover, rank, policy filter, activate, execute, observe." },
      { property: "og:title", content: "MCP / Tools — relay-x" },
      { property: "og:description", content: "Registration is not exposure. Discovery is not authorization." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: McpPage,
});

const lifecycle = [
  { step: "REGISTER", detail: "1,842 tools across 5 servers", tone: "neutral" },
  { step: "INDEX", detail: "vector + lexical index, 4 min ago", tone: "neutral" },
  { step: "DISCOVER", detail: "14 candidates for current query", tone: "info" },
  { step: "RANK", detail: "score ≥ 0.60", tone: "info" },
  { step: "POLICY FILTER", detail: "4 permitted · 10 denied", tone: "warn" },
  { step: "ACTIVATE", detail: "2 exposed to the model", tone: "ok" },
  { step: "EXECUTE", detail: "12.4k calls / 24 h", tone: "ok" },
  { step: "OBSERVE", detail: "latency, denials, error class", tone: "neutral" },
];

function McpPage() {
  return (
    <AppShell>
      <PageHeader
        title="MCP / Tools"
        subtitle="Capability registry — registered tools are not automatically exposed to the model."
      />
      <div className="space-y-3 p-4">
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
          <Metric label="Registered" value="1,842" hint="indexed tools" />
          <Metric label="Candidates" value="14" tone="info" hint="current query" />
          <Metric label="Permitted" value="4" tone="warn" hint="after policy filter" />
          <Metric label="Activated" value="2" tone="ok" hint="exposed to model" />
        </div>

        <div className="grid gap-3 xl:grid-cols-4">
          <Tabs defaultValue="servers" className="xl:col-span-3">
            <TabsList className="h-8 rounded-sm bg-panel p-0.5">
              {["servers", "tools", "discovery", "permissions", "activation"].map((t) => (
                <TabsTrigger key={t} value={t} className="h-7 rounded-sm px-2.5 text-xs capitalize">
                  {t === "servers" ? "MCP servers" : t}
                </TabsTrigger>
              ))}
            </TabsList>

            <TabsContent value="servers" className="mt-2">
              <Panel dense>
                <TableShell head={["Server", "Transport", "Scope", "Tools", "Indexed", "Status"]}>
                  {mcpServers.map((s) => (
                    <tr key={s.name} className="hover:bg-panel-raised/50">
                      <Td className="num font-medium">{s.name}</Td>
                      <Td className="num text-muted-foreground">{s.transport}</Td>
                      <Td className="num text-muted-foreground">{s.scope}</Td>
                      <Td className="num">{s.tools}</Td>
                      <Td className="num text-muted-foreground">{s.indexed}</Td>
                      <Td>
                        <StatusText status={s.status} />
                      </Td>
                    </tr>
                  ))}
                </TableShell>
              </Panel>
            </TabsContent>

            <TabsContent value="tools" className="mt-2">
              <Panel dense>
                <TableShell head={["Tool", "Server", "Rank", "Permitted", "Activated", "Calls / 24h"]}>
                  {mcpTools.map((t) => (
                    <tr key={t.name} className="hover:bg-panel-raised/50">
                      <Td className="num">{t.name}</Td>
                      <Td className="num text-muted-foreground">{t.server}</Td>
                      <Td className="num">{t.score.toFixed(2)}</Td>
                      <Td>
                        <StatusText status={t.permitted ? "allow" : "deny"} />
                      </Td>
                      <Td className={cn("num", t.activated ? "text-ok" : "text-muted-foreground")}>
                        {t.activated ? "yes" : "no"}
                      </Td>
                      <Td className="num">{t.calls}</Td>
                    </tr>
                  ))}
                </TableShell>
              </Panel>
            </TabsContent>

            <TabsContent value="discovery" className="mt-2">
              <Panel title="Query: github pull request">
                <div className="grid gap-3 md:grid-cols-4">
                  {[
                    ["Candidates", "14", "vector + lexical", "info"],
                    ["Ranked", "9", "score ≥ 0.60", "info"],
                    ["Permitted", "4", "policy filter", "warn"],
                    ["Activated", "2", "context budget", "ok"],
                  ].map(([l, v, h]) => (
                    <div key={l} className="rounded-sm border border-border bg-canvas px-2.5 py-2">
                      <div className="label-xs">{l}</div>
                      <div className="num mt-1 text-lg leading-none">{v}</div>
                      <div className="mt-1 text-[10px] text-muted-foreground">{h}</div>
                    </div>
                  ))}
                </div>
                <div className="mt-3 space-y-px">
                  {mcpTools.map((t) => (
                    <div key={t.name} className="flex items-center gap-2 border-b border-border/60 py-1.5 last:border-0">
                      <span className="num flex-1 truncate text-[11px]">{t.name}</span>
                      <span className="num text-[11px] text-muted-foreground">{t.score.toFixed(2)}</span>
                      <Tag tone={t.permitted ? "ok" : "fail"}>{t.permitted ? "permitted" : "denied"}</Tag>
                      {t.activated ? <Tag tone="info">activated</Tag> : null}
                    </div>
                  ))}
                </div>
              </Panel>
            </TabsContent>

            <TabsContent value="permissions" className="mt-2">
              <Panel title="Effective permissions">
                <p className="text-[11px] leading-relaxed text-muted-foreground">
                  Discovery and authorization are distinct states. A tool that ranks highly is still denied unless an
                  explicit ALLOW rule matches the tenant, workflow and environment.
                </p>
                <div className="mt-3 space-y-1.5">
                  <div className="num rounded-sm border border-ok/30 bg-ok/8 px-2 py-1.5 text-[11px] text-ok">
                    ALLOW · env=production AND workflow=customer-support AND tool=github.read_pr
                  </div>
                  <div className="num rounded-sm border border-fail/30 bg-fail/8 px-2 py-1.5 text-[11px] text-fail">
                    DENY · tool matches *.write_* AND env=production
                  </div>
                </div>
              </Panel>
            </TabsContent>

            <TabsContent value="activation" className="mt-2">
              <Panel title="Activation set (per request)">
                <div className="space-y-px">
                  {mcpTools
                    .filter((t) => t.activated)
                    .map((t) => (
                      <div key={t.name} className="flex items-center gap-2 py-1.5">
                        <StatusText status="active" />
                        <span className="num text-[11px]">{t.name}</span>
                        <span className="num ml-auto text-[11px] text-muted-foreground">schema 1.2 KB</span>
                      </div>
                    ))}
                </div>
                <p className="mt-2 text-[11px] text-muted-foreground">
                  Activation cost: 2.4 KB of tool schema injected · cache HIT · retrieval 1.8 ms.
                </p>
              </Panel>
            </TabsContent>
          </Tabs>

          <Panel title="Lifecycle">
            <ol className="space-y-0">
              {lifecycle.map((l, i) => (
                <li key={l.step} className="flex items-start gap-2.5">
                  <div className="flex flex-col items-center">
                    <span
                      className={cn(
                        "mt-1.5 size-1.5 rounded-full",
                        l.tone === "ok" ? "bg-ok" : l.tone === "warn" ? "bg-warn" : l.tone === "info" ? "bg-info" : "bg-border-strong",
                      )}
                    />
                    {i < lifecycle.length - 1 && <span className="my-0.5 h-6 w-px bg-border" />}
                  </div>
                  <div className="pb-1">
                    <div className="num text-[11px] tracking-wide">{l.step}</div>
                    <div className="text-[10px] text-muted-foreground">{l.detail}</div>
                  </div>
                </li>
              ))}
            </ol>
          </Panel>
        </div>
      </div>
    </AppShell>
  );
}
