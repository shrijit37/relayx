/**
 * Serializer: canonical model ⇄ persisted Workflow JSON (Phase 6.6 §9).
 *
 * Deterministic mapping with these invariants:
 * - deserialize(serialize(wf)) == wf for all semantic fields (positions are
 *   explicitly stored coordinates, not recomputed).
 * - No inference from titles, no fabricated values, no silent node dropping
 *   (display-only kinds become explicit `unsupported` nodes that block
 *   publish rather than vanishing).
 * - Edge labels are presentation; branch semantics come from explicit
 *   source ports (condition "true"/"false").
 */

import type { Edge, Node as RFNode } from "@xyflow/react";
import { getNodeDefinition, portsForType } from "./node-definitions";
import { validateStructural, type Issue } from "./validation";
import type {
  CanonicalConfig, CanonicalEdge, CanonicalNode, CanonicalWorkflow,
  ConditionOperator, EditorKind, InputVariable, LlmRequestConfig,
} from "./nodes";
import { WORKFLOW_SCHEMA_VERSION } from "./nodes";

// ─── Persisted schema (mirror of workflow_schema::*) ───────────────────────

export type SchemaNodeKind =
  | "input" | "output" | "llm" | "router" | "transform" | "condition"
  | "mcp" | "skill" | "fallback" | "retry" | "custom" | "unsupported";

export type LlmSchemaConfig = {
  kind: "llm";
  protocol?: string;
  provider?: string;
  model?: string;
  temperature?: number;
  max_tokens?: number;
  stream: boolean;
  lane_id?: string;
}

export type SchemaNodeConfig =
  | { kind: "input"; value?: unknown; input_type?: string; description?: string; variables?: InputVariable[] }
  | { kind: "output"; value?: unknown }
  | LlmSchemaConfig
  | { kind: "router"; strategy?: "first_match" | "round_robin" }
  | { kind: "transform"; operation?: "passthrough" | "extract" | "merge" | "filter" }
  | { kind: "condition"; condition: string; field: string; operator: string; value: unknown }
  | { kind: "mcp"; server_ref: string; tool_name: string; deferred: boolean }
  | { kind: "skill"; skill_ref: string; progressive: boolean }
  | { kind: "fallback"; providers: { lane_id: string; model?: string; protocol?: string }[]; rounds: number }
  | { kind: "retry"; max_attempts: number; delay_ms: number; on_timeout: boolean; on_provider_error: boolean; target: LlmSchemaConfig }
  | { kind: "custom"; payload: unknown }
  | { kind: "unsupported"; editor_kind: string; reason: string };

export interface SchemaNode {
  id: string;
  kind: SchemaNodeKind;
  config: SchemaNodeConfig;
  inputs: { name: string; port_type: string }[];
  outputs: { name: string; port_type: string }[];
  /** Explicit stored position — the canonical position of the node. */
  position?: { x: number; y: number };
  /** Editor-only metadata; never parsed back into semantics. */
  presentation?: { title?: string; notes?: string };
}

export interface SchemaEdge {
  id: string;
  source_node: string;
  source_port: string;
  target_node: string;
  target_port: string;
  condition?: { field: string; operator: string; value: unknown };
}

export interface WorkflowJson {
  id: string;
  name: string;
  version: number;
  /** Persisted schema version. Omitted/v1 = legacy (migrated on load). */
  schema_version?: number;
  nodes: SchemaNode[];
  edges: SchemaEdge[];
}

/** Editor kinds with no executable runtime semantic yet → explicit
 *  `unsupported` schema nodes (never silently dropped). */
const KIND_TO_SCHEMA: Record<EditorKind, SchemaNodeKind> = {
  input: "input", output: "output", provider: "llm", route: "router",
  transform: "transform", condition: "condition", fallback: "fallback",
  retry: "retry", mcp: "mcp", skill: "skill",
  lane: "unsupported", endpoint: "unsupported", tool: "unsupported",
  agent: "unsupported", policy: "unsupported", observability: "unsupported",
};

const SCHEMA_TO_KIND: Record<SchemaNodeKind, EditorKind> = {
  input: "input", output: "output", llm: "provider", router: "route",
  transform: "transform", condition: "condition", mcp: "mcp", skill: "skill",
  fallback: "fallback", retry: "retry", custom: "tool", unsupported: "tool",
};

/** Editor condition vocabulary → Rust ConditionOp enum (snake_case). */
const CONDITION_OP_TO_RUST: Record<string, string> = {
  equals: "equal",
  not_equals: "not_equal",
  greater_than: "greater_than",
  less_than: "less_than",
  contains: "contains",
  not_contains: "not_contains",
  is_empty: "is_empty",
  is_not_empty: "is_not_empty",
};

/** Inverse: Rust ConditionOp → editor vocabulary. */
const RUST_OP_TO_EDITOR: Record<string, string> = {
  equal: "equals",
  not_equal: "not_equals",
  greater_than: "greater_than",
  less_than: "less_than",
  contains: "contains",
  not_contains: "not_contains",
  is_empty: "is_empty",
  is_not_empty: "is_not_empty",
};

// ─── Canonical → persisted ─────────────────────────────────────────────────

function toSchemaNode(n: CanonicalNode, issues: Issue[]): SchemaNode | null {
  const kind = KIND_TO_SCHEMA[n.type];
  const def = getNodeDefinition(n.type);
  const inputs = n.ports.filter((p) => p.direction === "input").map((p) => ({ name: p.name, port_type: p.portType }));
  const outputs = n.ports.filter((p) => p.direction === "output").map((p) => ({ name: p.name, port_type: p.portType }));

  if (kind === "unsupported" || !def?.executable) {
    issues.push({
      nodeId: n.id, severity: "error",
      message: `Node '${n.id}' (${n.type}) is not executable; it is serialized as unsupported and blocks publish.`,
    });
    return {
      id: n.id, kind: "unsupported",
      config: { kind: "unsupported", editor_kind: n.type, reason: def?.note ?? "not executable yet" },
      inputs, outputs, position: n.position, presentation: n.presentation,
    } as SchemaNode;
  }

  const config = configFor(n, issues);
  if (!config) return null;
  const out: SchemaNode = {
    id: n.id, kind, config, inputs, outputs,
    position: n.position,
  };
  if (n.presentation) out.presentation = n.presentation;
  return out;
}

function llmToSchema(c: LlmRequestConfig): LlmSchemaConfig {
  const out: LlmSchemaConfig = { kind: "llm", stream: c.stream };
  if (c.provider !== undefined) out.provider = c.provider;
  if (c.model !== undefined) out.model = c.model;
  if (c.lane !== undefined) out.lane_id = c.lane;
  if (c.protocol !== undefined) out.protocol = c.protocol;
  if (c.temperature !== undefined) out.temperature = c.temperature;
  if (c.maxTokens !== undefined) out.max_tokens = c.maxTokens;
  return out;
}

function configFor(n: CanonicalNode, issues: Issue[]): SchemaNodeConfig | null {
  const c = n.config;
  switch (c.kind) {
    case "input": {
      const config: Extract<SchemaNodeConfig, { kind: "input" }> = { kind: "input" };
      if (c.value !== undefined) config.value = c.value;
      if (c.inputType && c.inputType !== "message") config.input_type = c.inputType;
      if (c.description) config.description = c.description;
      if (c.variables && c.variables.length > 0) config.variables = c.variables;
      return config;
    }
    case "output": return { kind: "output", value: c.value };
    case "llm": {
      const cfg = llmToSchema(c.config);
      if (cfg.lane_id === undefined) issues.push({ nodeId: n.id, field: "lane", severity: "error", message: "LLM requires a lane; select one before saving." });
      if (cfg.model === undefined && !cfg.lane_id) issues.push({ nodeId: n.id, field: "model", severity: "error", message: "Model is required." });
      if (cfg.provider === undefined && !cfg.model) issues.push({ nodeId: n.id, field: "provider", severity: "error", message: "Provider is required." });
      return cfg;
    }
    case "router": return { kind: "router", strategy: c.strategy };
    case "transform": return { kind: "transform", operation: c.operation };
    case "condition": {
      const cond = c.condition;
      const value = cond.valueType === "number" ? Number(cond.value) : cond.valueType === "boolean" ? cond.value === "true" : cond.value === "" ? undefined : cond.value;
      if (!cond.field) issues.push({ nodeId: n.id, field: "field", severity: "error", message: "Condition field is required." });
      const needsValue = !["is_empty", "is_not_empty"].includes(cond.operator);
      if (needsValue && (cond.value === undefined || cond.value === null || cond.value === "")) {
        issues.push({ nodeId: n.id, field: "value", severity: "error", message: "Condition value is required for this operator." });
      }
      return {
        kind: "condition",
        // Rust ConditionConfig requires a `condition` expression string and
        // snake_case operator vocab (equal/not_equal) — the editor's UI vocab
        // (equals/not_equals) maps to it here, deterministically.
        condition: `${cond.field} ${cond.operator} ${cond.value}`.trim(),
        field: cond.field,
        operator: CONDITION_OP_TO_RUST[cond.operator] ?? cond.operator,
        value: value as unknown,
      };
    }
    case "mcp": {
      if (!c.tool.capabilityRef) issues.push({ nodeId: n.id, field: "capabilityRef", severity: "error", message: "MCP requires a capability reference (mcp://server/tool)." });
      const [server, ...rest] = c.tool.capabilityRef.replace(/^mcp:\/\//, "").split("/");
      return {
        kind: "mcp",
        server_ref: c.tool.serverRef ?? server ?? "",
        tool_name: c.tool.toolName ?? rest.join("/") ?? "",
        deferred: c.tool.deferred,
      };
    }
    case "skill": {
      if (!c.skill.skillRef) issues.push({ nodeId: n.id, field: "skillRef", severity: "error", message: "Skill reference is required." });
      return { kind: "skill", skill_ref: c.skill.skillRef, progressive: c.skill.progressive };
    }
    case "fallback": {
      if (c.fallback.providers.length === 0) issues.push({ nodeId: n.id, severity: "error", message: "Fallback needs at least one provider lane." });
      const providers = c.fallback.providers.map((p) => {
        const e: { lane_id: string; model: string; protocol?: string } = { lane_id: p.lane, model: p.model ?? "" };
        if (p.model === undefined || p.model === "") {
          issues.push({ nodeId: n.id, field: `provider.${p.lane}.model`, severity: "error", message: `Fallback lane '${p.lane}' requires a model override (Rust FallbackProvider.model is required).` });
        }
        return e;
      });
      return { kind: "fallback", providers, rounds: c.fallback.rounds };
    }
    case "retry": {
      if (c.policy.maxAttempts < 1) issues.push({ nodeId: n.id, field: "maxAttempts", severity: "error", message: "Max attempts must be ≥ 1." });
      if (c.target.lane === undefined) issues.push({ nodeId: n.id, field: "target.lane", severity: "error", message: "Retry target requires a lane." });
      return {
        kind: "retry",
        max_attempts: c.policy.maxAttempts,
        delay_ms: c.policy.delayMs,
        on_timeout: c.policy.onTimeout,
        on_provider_error: c.policy.onProviderError,
        target: llmToSchema(c.target),
      };
    }
  }
}

/** Canonical → persisted Workflow JSON. Deterministic; all issues are
 *  collected (never silently dropped nodes or fabricated configs). */
export function toWorkflowJson(wf: CanonicalWorkflow): { workflow: WorkflowJson; issues: Issue[] } {
  const issues: Issue[] = [...validateStructural(wf)];
  const nodes: SchemaNode[] = [];
  for (const n of wf.nodes) {
    const sn = toSchemaNode(n, issues);
    if (sn) nodes.push(sn);
  }
  const edges = wf.edges.map((e) => {
    const se: SchemaEdge = {
      id: e.id, source_node: e.source, source_port: e.sourcePort,
      target_node: e.target, target_port: e.targetPort,
    };
    if (e.label === "if") se.condition = { field: "__edge__", operator: "equals", value: true };
    return se;
  });
  return {
    workflow: {
      id: wf.id, name: wf.name, version: wf.version,
      schema_version: WORKFLOW_SCHEMA_VERSION, nodes, edges,
    },
    issues,
  };
}

// ─── Persisted → canonical ─────────────────────────────────────────────────

function llmFromSchema(c: LlmSchemaConfig): LlmRequestConfig {
  const out: LlmRequestConfig = { stream: c.stream };
  if (c.provider !== undefined) out.provider = c.provider;
  if (c.model !== undefined) out.model = c.model;
  if (c.lane_id !== undefined) out.lane = c.lane_id;
  if (c.protocol !== undefined) out.protocol = c.protocol;
  if (c.temperature !== undefined) out.temperature = c.temperature;
  if (c.max_tokens !== undefined) out.maxTokens = c.max_tokens;
  return out;
}

function configFromSchema(sn: SchemaNode, issues: Issue[]): CanonicalConfig | null {
  const c = sn.config;
  switch (c.kind) {
    case "input": return {
      kind: "input",
      inputType: c.input_type ?? "message",
      description: c.description ?? "",
      variables: c.variables ?? [],
      value: c.value,
    };
    case "output": return { kind: "output", value: c.value };
    case "llm": return { kind: "llm", config: llmFromSchema(c) };
    case "router": return { kind: "router", strategy: c.strategy ?? "round_robin" };
    case "transform": return { kind: "transform", operation: c.operation ?? "passthrough" };
    case "condition": return {
      kind: "condition",
      condition: {
        field: c.field ?? "",
        operator: (RUST_OP_TO_EDITOR[c.operator as string] ?? c.operator ?? "equals") as ConditionOperator,
        value: c.value === undefined || c.value === null ? "" : String(c.value),
        valueType: typeof c.value === "number" ? "number" : typeof c.value === "boolean" ? "boolean" : "string",
      },
    };
    case "mcp": return {
      kind: "mcp",
      tool: { capabilityRef: c.server_ref ? `mcp://${[c.server_ref, c.tool_name].filter(Boolean).join("/")}` : "", serverRef: c.server_ref, toolName: c.tool_name, deferred: c.deferred },
    };
    case "skill": return { kind: "skill", skill: { skillRef: c.skill_ref, progressive: c.progressive } };
    case "fallback": return { kind: "fallback", fallback: { providers: c.providers.map((p) => ({ lane: p.lane_id, ...(p.model ? { model: p.model } : {}) })), rounds: c.rounds } };
    case "retry": return { kind: "retry", policy: { maxAttempts: c.max_attempts, delayMs: c.delay_ms, onTimeout: c.on_timeout, onProviderError: c.on_provider_error }, target: llmFromSchema(c.target) };
    case "custom": {
      issues.push({ nodeId: sn.id, severity: "error", message: `Node '${sn.id}' is a custom runtime node; the editor cannot edit it.` });
      return null;
    }
    case "unsupported": {
      const ek = c.editor_kind as EditorKind | undefined;
      if (!ek || !(ek in KIND_TO_SCHEMA)) {
        issues.push({ nodeId: sn.id, severity: "error", message: `Node '${sn.id}' has unsupported kind '${String(c.editor_kind)}'; it cannot be represented.` });
        return null;
      }
      issues.push({ nodeId: sn.id, severity: "error", message: `Node '${sn.id}' (${ek}) is not executable; resolve or remove it before publishing.` });
      const base = configForKind(ek);
      return base;
    }
    default: {
      // Never silently drop a node: an unmappable config kind is reported.
      issues.push({ nodeId: sn.id, severity: "error", message: `Node '${sn.id}' has unmappable config kind '${String((c as { kind?: unknown }).kind)}'; it cannot be represented.` });
      return null;
    }
  }
}

/** Typed default config for a kind (used when rehydrating unsupported nodes
 *  and for new nodes). */
export function configForKind(kind: EditorKind): CanonicalConfig {
  const def = getNodeDefinition(kind);
  return def ? def.defaults() : { kind: "mcp", tool: { capabilityRef: "", deferred: true } };
}

function typeFromSchema(kind: SchemaNodeKind): EditorKind | null {
  if (kind === "custom") return null;
  // Schema kinds → editor kinds; editor kinds (stored by the legacy canvas)
  // pass through; anything else degrades to a display-only kind instead of
  // crashing the canvas on an unmapped `data.kind`.
  return SCHEMA_TO_KIND[kind] ?? (kind in KIND_TO_SCHEMA ? (kind as EditorKind) : "tool");
}

/** Persisted → canonical. Positions come from explicit stored coordinates.
 *  v1/legacy JSON without coordinates gets a deterministic layout (and its
 *  schema_version is upgraded on next save). */
export function fromWorkflowJson(wf: WorkflowJson): { workflow: CanonicalWorkflow; issues: Issue[] } {
  const issues: Issue[] = [];
  const nodes: CanonicalNode[] = [];
  const seen = new Set<string>();
  const legacy = !wf.schema_version || wf.schema_version < 2;
  let seq = 0;
  for (const sn of wf.nodes ?? []) {
    const type = typeFromSchema(sn.kind);
    if (!type) {
      issues.push({ nodeId: sn.id, severity: "error", message: `Node '${sn.id}' has unmappable kind '${sn.kind}'.` });
      continue;
    }
    if (seen.has(sn.id)) {
      issues.push({ nodeId: sn.id, severity: "error", message: `Duplicate node id '${sn.id}'.` });
      continue;
    }
    seen.add(sn.id);
    const config = configFromSchema(sn, issues);
    if (!config) continue;
    nodes.push({
      id: sn.id, type, version: 1,
      position: sn.position ?? (legacy ? { x: (seq % 3) * 280, y: Math.floor(seq++ / 3) * 220 } : { x: 0, y: 0 }),
      ports: portsForType(type),
      config,
      ...(sn.presentation ? { presentation: sn.presentation } : {}),
    } as CanonicalNode);
  }
  const edges: CanonicalEdge[] = (wf.edges ?? []).map((e, ei) => {
    const ce: CanonicalEdge = {
      id: e.id ?? `e-${ei}`,
      source: e.source_node,
      sourcePort: e.source_port,
      target: e.target_node,
      targetPort: e.target_port,
    };
    if (e.condition) ce.label = "if";
    return ce;
  });
  return {
    workflow: {
      id: wf.id ?? "workflow",
      name: wf.name ?? "untitled",
      version: wf.version ?? 1,
      schemaVersion: WORKFLOW_SCHEMA_VERSION,
      nodes, edges,
    },
    issues,
  };
}

// ─── React Flow view adapter ───────────────────────────────────────────────

export type RelayNodeDataView = {
  kind: EditorKind;
  title: string;
  lines: string[];
  [key: string]: unknown;
}

/** Canonical node → React Flow node (view; positions are the canonical
 *  positions; presentation metadata is derived, never the reverse). The view
 *  carries a reference to its canonical node so the inspector edits REAL
 *  config, not defaults. */
export function toViewNode(n: CanonicalNode): RFNode<RelayNodeDataView, "relay"> {
  const def = getNodeDefinition(n.type);
  const title = n.presentation?.title ?? (def ? def.displayTitle(n.config) : n.type);
  const lines = def?.displayLines ? def.displayLines(n.config) : [];
  return {
    id: n.id, type: "relay", position: n.position,
    data: {
      kind: n.type, title, lines,
      canonicalId: n.id,
      canonicalConfig: n.config,
    },
  };
}

/** React Flow node → canonical node. The view carries no semantic state:
 *  positions are captured back into the model, and config comes from the
 *  inspector-edited `canonicalConfig` (falling back to a typed default). */
export function fromViewNode(v: RFNode<RelayNodeDataView, "relay">): CanonicalNode {
  const type = (v.data?.kind ?? "tool") as EditorKind;
  const def = getNodeDefinition(type);
  const explicit = (v.data as { canonicalConfig?: CanonicalConfig }).canonicalConfig;
  let config: CanonicalConfig;
  let presentationTitle: string | undefined;
  if (def && v.data) {
    config = explicit ?? def.defaults();
    // Presentation is only persisted when it differs from the derived title —
    // a title equal to the display-derived value is not a custom title. This
    // avoids redundant `presentation` bloat on every save/load round-trip.
    presentationTitle = v.data.title !== def.displayTitle(config) ? v.data.title : undefined;
  } else {
    config = { kind: "mcp", tool: { capabilityRef: "", deferred: true } };
    presentationTitle = v.data?.title;
  }
  return {
    id: v.id, type, version: 1,
    position: v.position ?? { x: 0, y: 0 },
    ports: portsForType(type),
    config,
    ...(presentationTitle ? { presentation: { title: presentationTitle } } : {}),
  } as unknown as CanonicalNode;
}

/** React Flow edges → canonical edges (explicit ports). */
export function toCanonicalEdges(edges: Edge[]): CanonicalEdge[] {
  return edges.map((e) => {
    const ce: CanonicalEdge = {
      id: e.id,
      source: e.source,
      sourcePort: e.sourceHandle ?? "out",
      target: e.target,
      targetPort: e.targetHandle ?? "in",
    };
    if (e.label !== undefined) ce.label = String(e.label);
    return ce;
  });
}

/** Canonical edges → React Flow edges (no semantic reconstruction). */
export function fromCanonicalEdges(edges: CanonicalEdge[]): Edge[] {
  return edges.map((e) => {
    const rf: Edge = { id: e.id, source: e.source, target: e.target, type: "deletable" };
    if (e.sourcePort !== "out") rf.sourceHandle = e.sourcePort;
    if (e.targetPort !== "in") rf.targetHandle = e.targetPort;
    if (e.label) rf.label = e.label;
    return rf;
  });
}

// ─── Public view-level API (Phase 6.6 §9) ──────────────────────────────────
// The single lossless pairing used by the editor: canvas(view) ⇄ persisted
// JSON, both through the canonical model. `serializeWorkflow` cannot invent
// config or silently drop nodes — it returns explicit `errors` and a null
// workflow instead.

export interface SerializeResult {
  workflow: WorkflowJson | null;
  errors: string[];
  warnings: string[];
}

export interface DeserializeResult {
  nodes: RFNode<RelayNodeDataView, "relay">[];
  edges: Edge[];
  /** Load-time refusals (unmappable/unsupported nodes) — the caller must
   *  surface these and/or block further action, never ignore them. */
  errors: string[];
  warnings: string[];
}

/** Canvas view → canonical → persisted WorkflowJson. Deterministic; errors
 *  (missing required config, unsupported/display-only nodes) block the
 *  result — never a fabricated workflow. */
export function serializeWorkflow(
  nodes: RFNode<RelayNodeDataView, "relay">[],
  edges: Edge[],
  meta?: { id?: string; name?: string; version?: number },
): SerializeResult {
  const wf: CanonicalWorkflow = {
    id: meta?.id ?? "workflow",
    name: meta?.name ?? "untitled",
    version: meta?.version ?? 1,
    schemaVersion: WORKFLOW_SCHEMA_VERSION,
    nodes: nodes.map(fromViewNode),
    edges: toCanonicalEdges(edges),
  };
  const { workflow, issues } = toWorkflowJson(wf);
  const errors = issues.filter((i) => i.severity === "error").map((i) => i.message);
  const warnings = issues.filter((i) => i.severity === "warn").map((i) => i.message);
  if (errors.length > 0 || workflow.nodes.length === 0) {
    return { workflow: null, errors, warnings };
  }
  return { workflow, errors, warnings };
}

/** Persisted WorkflowJson → canonical → canvas view (explicit positions,
 *  canonicalConfig carried for the Inspector; edges keep their ports). */
export function deserializeWorkflow(wf: WorkflowJson): DeserializeResult {
  const { workflow, issues } = fromWorkflowJson(wf);
  return {
    nodes: workflow.nodes.map(toViewNode),
    edges: fromCanonicalEdges(workflow.edges),
    errors: issues.filter((i) => i.severity === "error").map((i) => i.message),
    warnings: issues.filter((i) => i.severity === "warn").map((i) => i.message),
  };
}
