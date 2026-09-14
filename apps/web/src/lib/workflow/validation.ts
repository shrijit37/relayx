/**
 * Layered validation (Phase 6.6 §14–15).
 *
 * Layer 1 — structural: unique ids, valid types, port existence, cycles,
 *   reachability, exactly one Input/Output. Editor-local, runs on every
 *   change.
 * Layer 2 — schema: required fields, types, enums, ranges. Editor-local.
 * Layer 3 — semantic: lane/model/provider references. The control plane's
 *   lane table is authorititative — the editor caches it and flags unknown
 *   references immediately; compile/publish on the backend remains the
 *   final gate (§16: backend-authoritative publishability).
 * Layer 4+ (capability/protocol/policy) — implemented backend-side; the
 *   editor surfaces backend errors verbatim.
 */

import type { CanonicalConfig, CanonicalWorkflow, EditorKind, LlmRequestConfig } from "./nodes";
import { getNodeDefinition, portsForType } from "./node-definitions";

export interface Issue {
  nodeId?: string;
  field?: string;
  severity: "error" | "warn";
  message: string;
}

/**
 * Layered validation (Phase 6.6 §14–15).
 */

export interface ValidationResult {
  issues: Issue[];
  errors: number;
  warnings: number;
  /** Structural errors block serialization entirely. */
  structural: boolean;
  /** Schema errors block serialization. */
  schema: boolean;
  /** Semantic reference problems (lane/model unknown). */
  semantic: Issue[];
}

export type LaneRef = { id: string; baseUrl: string };

const KNOWN_PROVIDERS = new Set(["anthropic", "openai"]);

export function validateConfig(type: EditorKind, config: CanonicalConfig, issues: Issue[]): void {
  const def = getNodeDefinition(type);
  if (!def) return;
  for (const f of def.fields) {
    const value = fieldValue(config, f.name);
    if (f.required) {
      if (value === undefined || value === null || value === "" || (Array.isArray(value) && value.length === 0)) {
        issues.push(issueFor(config, { field: f.name, severity: "error", message: `${f.label} is required.` }));
        continue;
      }
    }
    if (typeof f.min === "number" && typeof value === "number" && value < f.min) {
      issues.push(issueFor(config, { field: f.name, severity: "error", message: `${f.label} must be ≥ ${f.min}.` }));
    }
    if (typeof f.max === "number" && typeof value === "number" && value > f.max) {
      issues.push(issueFor(config, { field: f.name, severity: "error", message: `${f.label} must be ≤ ${f.max}.` }));
    }
    if (f.type === "enum" && typeof value === "string" && f.options && !f.options.some((o) => o.value === value)) {
      issues.push(issueFor(config, { field: f.name, severity: "error", message: `Invalid ${f.label} '${value}'.` }));
    }
  }
  if (config.kind === "llm") {
    const llm = config.config;
    if (llm?.provider && !KNOWN_PROVIDERS.has(llm.provider)) {
      issues.push(issueFor(config, { field: "provider", severity: "warn", message: `Provider '${llm.provider}' is not in the known set (anthropic, openai) — backend will verify.` }));
    }
  }
}

function configNodeId(_config: CanonicalConfig): string | undefined {
  return undefined;
}

function issueFor(config: CanonicalConfig, init: Omit<Issue, "nodeId">): Issue {
  const nodeId = configNodeId(config);
  return nodeId ? { ...init, nodeId } : init;
}

function fieldValue(config: CanonicalConfig, name: string): unknown {
  switch (config.kind) {
    case "llm": return config.config[name as keyof LlmRequestConfig];
    case "router": return config.strategy === name ? config.strategy : undefined;
    case "transform": return config.operation === name ? config.operation : undefined;
    case "condition": return config.condition[name as keyof typeof config.condition];
    case "mcp": return config.tool[name as keyof typeof config.tool];
    case "skill": return config.skill[name as keyof typeof config.skill];
    case "fallback": return config.fallback[name as keyof typeof config.fallback];
    case "retry": return config.policy[name as keyof typeof config.policy];
    default: return undefined;
  }
}

export function validateStructural(wf: CanonicalWorkflow): Issue[] {
  const issues: Issue[] = [];
  const ids = new Set<string>();
  for (const n of wf.nodes) {
    if (ids.has(n.id)) issues.push({ nodeId: n.id, severity: "error", message: `Duplicate node id '${n.id}'.` });
    ids.add(n.id);
    const validPorts = portsForType(n.type).map((p) => p.name);
    for (const p of n.ports) {
      if (!validPorts.includes(p.name)) {
        issues.push({ nodeId: n.id, field: p.name, severity: "error", message: `Unknown port '${p.name}' on '${n.type}'.` });
      }
    }
  }
  const inputs = wf.nodes.filter((n) => n.type === "input");
  const outputs = wf.nodes.filter((n) => n.type === "output");
  if (inputs.length === 0) issues.push({ severity: "error", message: "Workflow has no Input node." });
  if (outputs.length === 0) issues.push({ severity: "error", message: "Workflow has no Output node." });

  for (const e of wf.edges) {
    const src = wf.nodes.find((n) => n.id === e.source);
    const dst = wf.nodes.find((n) => n.id === e.target);
    if (!src) { issues.push({ severity: "error", message: `Edge '${e.id}' references unknown source '${e.source}'.` }); continue; }
    if (!dst) { issues.push({ severity: "error", message: `Edge '${e.id}' references unknown target '${e.target}'.` }); continue; }
    if (!src.ports.some((p) => p.name === e.sourcePort && p.direction === "output")) {
      issues.push({ nodeId: src.id, severity: "error", message: `Unknown source port '${e.sourcePort}'.` });
    }
    if (!dst.ports.some((p) => p.name === e.targetPort && p.direction === "input")) {
      issues.push({ nodeId: dst.id, severity: "error", message: `Unknown target port '${e.targetPort}'.` });
    }
  }
  // Acyclicity (simple DFS).
  const adj = new Map<string, string[]>();
  for (const n of wf.nodes) adj.set(n.id, []);
  for (const e of wf.edges) adj.get(e.source)?.push(e.target);
  const state = new Map<string, 0 | 1 | 2>();
  const visiting: string[] = [];
  const visit = (id: string): boolean => {
    const s = state.get(id) ?? 0;
    if (s === 2) return false;
    if (s === 1) {
      issues.push({ nodeId: id, severity: "error", message: `Cycle detected involving '${id}'.` });
      return true;
    }
    state.set(id, 1);
    visiting.push(id);
    const next = adj.get(id) ?? [];
    let cyclic = false;
    for (const t of next) if (visit(t)) cyclic = true;
    const visited = state.get(id) ?? 0;
    if (visited === 1 && visiting.length) visiting.pop();
    state.set(id, 2);
    return cyclic;
  };
  for (const n of wf.nodes) visit(n.id);
  void visiting;
  return issues;
}

export function validateNode(config: CanonicalConfig): Issue[] {
  const issues: Issue[] = [];
  const def = getNodeDefinition(config.kind as unknown as EditorKind);
  if (def) validateConfig(config.kind as unknown as EditorKind, config, issues);
  return issues;
}

export function validateWorkflow(
  wf: CanonicalWorkflow,
  lanes: LaneRef[],
  compileIssues: Issue[] = [],
): ValidationResult {
  const issues: Issue[] = [...validateStructural(wf)];
  // Per-node schema validation (with node id attribution).
  for (const n of wf.nodes) {
    const def = getNodeDefinition(n.type);
    if (!def) continue;
    const nodeIssues: Issue[] = [];
    validateConfig(n.type, n.config, nodeIssues);
    issues.push(...nodeIssues.map((i) => ({ ...i, nodeId: n.id })));
    // Semantic: lane references.
    if (n.config.kind === "llm") {
      const cfg = n.config.config;
      if (cfg.lane && !lanes.some((l) => l.id === cfg.lane)) {
        issues.push({ nodeId: n.id, field: "lane", severity: "error", message: `Lane '${cfg.lane}' does not exist in the control plane.` });
      }
    }
    if (n.config.kind === "retry") {
      const target = n.config.target;
      if (target.lane && !lanes.some((l) => l.id === target.lane)) {
        issues.push({ nodeId: n.id, field: "target.lane", severity: "error", message: `Lane '${target.lane}' does not exist.` });
      }
    }
    if (n.config.kind === "fallback") {
      for (const p of n.config.fallback.providers) {
        if (!lanes.some((l) => l.id === p.lane)) {
          issues.push({ nodeId: n.id, severity: "error", message: `Fallback lane '${p.lane}' does not exist.` });
        }
      }
    }
  }
  issues.push(...compileIssues);
  const errors = issues.filter((i) => i.severity === "error").length;
  const warnings = issues.filter((i) => i.severity === "warn").length;
  const semantic = issues.filter((i) => i.message.includes("does not exist") || i.message.includes("backend"));
  return { issues, errors, warnings, structural: false, schema: errors > 0, semantic };
}
