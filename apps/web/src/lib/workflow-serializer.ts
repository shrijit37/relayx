/**
 * React Flow → Workflow JSON serializer.
 *
 * Maps editor canvas state (RelayNode[] + Edge[]) into the canonical
 * Workflow JSON the Rust `workflow-schema` crate consumes. React Flow remains
 * the editor format; this is the only place editor state becomes workflow
 * definition.
 *
 * Rules:
 * - every executable node kind maps to a schema NodeKind
 * - `lane` nodes are NOT schema nodes — a lane node feeding an LLM-ish node
 *   (`llm`/`provider`/`retry`/`fallback`) becomes that node's `lane_id`
 * - display-only kinds (mcp, skill, policy, observability, tool, endpoint,
 *   agent) are dropped with a warning (the runtime does not execute them yet)
 * - the workflow must contain exactly one Input and one Output; anything else
 *   is `errors`, not silently accepted
 */

import type { Edge } from "@xyflow/react";
import type { NodeKind as EditorKind, RelayNode } from "@/components/relay/workflow/nodes";

/** Schema NodeKind values (mirror of workflow_schema::NodeKind). */
export type SchemaNodeKind =
  | "input"
  | "output"
  | "llm"
  | "router"
  | "transform"
  | "condition"
  | "mcp"
  | "skill"
  | "fallback"
  | "retry"
  | "custom";

/** Schema node config (mirror of workflow_schema::NodeConfig). */
export type SchemaNodeConfig =
  | { kind: "input"; value?: unknown }
  | { kind: "output"; value?: unknown }
  | {
      kind: "llm";
      protocol?: string;
      model?: string;
      temperature?: number;
      max_tokens?: number;
      stream: boolean;
      lane_id?: string;
    }
  | { kind: "router"; strategy?: "first_match" | "round_robin" | "load_based" }
  | { kind: "transform"; operation?: "passthrough" | "extract" | "merge" | "filter" }
  | {
      kind: "condition";
      condition: string;
      field: string;
      operator: "equal" | "not_equal" | "greater_than" | "less_than" | "contains" | "not_contains" | "is_empty" | "is_not_empty";
      value: unknown;
    }
  | { kind: "mcp"; server_ref: string; tool_name: string; deferred: boolean }
  | { kind: "skill"; skill_ref: string; progressive: boolean }
  | { kind: "fallback"; providers: { lane_id: string; model: string; protocol?: string }[]; rounds: number }
  | { kind: "retry"; max_attempts: number; delay_ms: number; on_timeout: boolean; on_provider_error: boolean; target: SchemaNodeConfig };

export interface SchemaNode {
  id: string;
  kind: SchemaNodeKind;
  config: SchemaNodeConfig;
  inputs: { name: string; port_type: string }[];
  outputs: { name: string; port_type: string }[];
}

export interface SchemaEdge {
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
  nodes: SchemaNode[];
  edges: SchemaEdge[];
}

export interface SerializeResult {
  workflow: WorkflowJson | null;
  errors: string[];
  warnings: string[];
  /** lane nodes folded into consuming nodes: lane id → fed count */
  lanesFolded: number;
  /**
   * Lane id → base URL map for the lanes the workflow references. Empty when
   * the editor carries no URL for a folded lane — callers must still provide
   * the URLs at publish time or the backend will reject the workflow.
   */
  lanes: Record<string, string>;
}

/** Editor kinds that carry no executable schema semantic. */
const DROPPED_KINDS = new Set<EditorKind>([
  "policy",
  "observability",
  "tool",
  "endpoint",
  "agent",
]);

/** Editor kinds that map to a schema LLM node (lane folding applies). */
const LLM_LIKE = new Set<EditorKind>(["provider", "route"]);

/** Editor kind → schema node kind for direct-executing kinds. */
function kindFor(editor: EditorKind): SchemaNodeKind | null {
  switch (editor) {
    case "input":
      return "input";
    case "output":
      return "output";
    case "transform":
      return "transform";
    case "condition":
      return "condition";
    case "fallback":
      return "fallback";
    case "retry":
      return "retry";
    case "mcp":
      return "mcp";
    case "skill":
      return "skill";
    case "provider":
      return "llm";
    case "route":
      return "router";
    default:
      return null;
  }
}

const DEFAULT_IN = [{ name: "in", port_type: "message" }];
const DEFAULT_OUT = [{ name: "out", port_type: "message" }];

/** Infer a model name from a provider/llm node's display title.
 *  "Anthropic · Claude Sonnet" → "claude-sonnet"; fall back to the raw title. */
function modelFromTitle(title: string): string | undefined {
  const match = /·\s*([\w.\-:]+)/.exec(title);
  if (match && match[1]) return match[1];
  return /([\w.\-]+\d+(?:\.\d+)?(?:-\w+)?)/.exec(title)?.[1] ?? undefined;
}

function configFor(node: RelayNode, laneId: string | undefined): SchemaNodeConfig | null {
  const title = node.data.title || "Unconfigured";
  const editorKind = node.data.kind;
  switch (editorKind) {
    case "input":
      return { kind: "input" };
    case "output":
      return { kind: "output" };
    case "transform":
      return { kind: "transform", operation: "merge" };
    case "condition":
      // The editor does not store a machine-readable condition
      // (field/operator/value). Fabricating one from the display title would
      // compile a condition that always evaluates against a field the input
      // almost never has — a guaranteed-broken runtime graph. Return `null`
      // and let the serializer report it as an error (invalid editor state).
      return null;
    case "mcp":
      return { kind: "mcp", server_ref: title, tool_name: "unknown", deferred: true };
    case "skill":
      return { kind: "skill", skill_ref: title, progressive: true };
    case "route":
      return { kind: "router", strategy: "round_robin" };
    case "retry":
      return {
        kind: "retry",
        max_attempts: 2,
        delay_ms: 1000,
        on_timeout: true,
        on_provider_error: true,
        target: withLaneId({ kind: "llm", stream: false }, laneId),
      };
    case "fallback":
      return {
        kind: "fallback",
        providers: laneId
          ? [{ lane_id: laneId, model: modelFromTitle(title) ?? "default" }]
          : [],
        rounds: 1,
      };
    case "provider": {
      const model = modelFromTitle(title);
      return withLaneId(
        { kind: "llm", model: model ?? "default", stream: true },
        laneId,
      );
    }
    default:
      return null;
  }
}

/** Attach a lane id to an LLM config without violating exactOptionalPropertyTypes. */
function withLaneId(
  base: { kind: "llm"; model?: string; stream: boolean },
  laneId: string | undefined,
): SchemaNodeConfig {
  if (laneId === undefined) return base;
  return { ...base, lane_id: laneId };
}

/**
 * Serialize a React Flow canvas into canonical Workflow JSON.
 *
 * Lane nodes are consumed by LLM-like downstream nodes. Nodes without an
 * executable schema mapping produce warnings and are dropped; structural
 * problems (no Input, no Output, unknown nodes on the canvas) are errors.
 *
 * `laneUrls` maps a lane id (usually the lane node's title) to its base URL.
 * Any folded lane missing from it produces a warning and an empty URL in
 * `result.lanes` — the caller must resolve it before publishing.
 */
// ── Deserializer ────────────────────────────────────────────────────────

/** Reverse map: schema node kind → editor node kind. */
const KIND_REVERSE: Record<SchemaNodeKind, EditorKind | null> = {
  input: "input",
  output: "output",
  llm: "provider",
  router: "route",
  transform: "transform",
  condition: "condition",
  mcp: "mcp",
  skill: "skill",
  fallback: "fallback",
  retry: "retry",
  custom: null,
};

const SEQUENCE_GAP = 280;

/** Determine editor kind from a schema node config. */
function editorKindFromConfig(config: SchemaNodeConfig): EditorKind {
  switch (config.kind) {
    case "llm": return "provider";
    case "router": return "route";
    default: return config.kind as EditorKind;
  }
}

/** Derive a display title from a schema node config. */
function titleFromConfig(node: SchemaNode): string {
  const c = node.config;
  switch (c.kind) {
    case "llm": return c.model ? `Model · ${c.model}` : "LLM";
    case "router": return "Model Router";
    case "transform": return "Transform";
    case "condition": return "Condition";
    case "mcp": return c.server_ref || "MCP";
    case "skill": return c.skill_ref || "Skill";
    case "fallback": return "Fallback";
    case "retry": return `Retry · ${c.max_attempts} attempts`;
    case "input": return "HTTP Request";
    case "output": return "Streaming Response";
    default: return "Node";
  }
}

/** Derive display lines from a schema node config. */
function linesFromConfig(node: SchemaNode): string[] {
  const c = node.config;
  switch (c.kind) {
    case "llm": {
      const parts: string[] = [];
      if (c.protocol) parts.push(c.protocol);
      if (c.stream) parts.push("streaming");
      if (c.lane_id) parts.push(`lane: ${c.lane_id}`);
      return parts.length ? parts : ["llm"];
    }
    case "router": return c.strategy ? [`strategy: ${c.strategy}`] : ["router"];
    case "transform": return c.operation ? [`operation: ${c.operation}`] : ["transform"];
    case "condition": return [`${c.field} ${c.operator} ${c.value}`];
    case "mcp": return [c.tool_name, c.deferred ? "deferred" : "immediate"];
    case "skill": return [c.progressive ? "progressive" : "direct"];
    case "fallback": return [`${c.providers.length} fallback(s)`, `${c.rounds} round(s)`];
    case "retry": return [`delay: ${c.delay_ms}ms`, `on timeout: ${c.on_timeout}`];
    case "input": return ["ingress"];
    case "output": return ["egress"];
    default: return [];
  }
}

function laneConfig(laneId: string, nodes: SchemaNode[]): { kind: "lane"; title: string; lines: string[] } {
  const lane = nodes.find((n) => n.config.kind === "llm" && n.config.lane_id === laneId);
  const model = lane && lane.config.kind === "llm" ? lane.config.model : undefined;
  return { kind: "lane", title: laneId, lines: model ? [`feeds ${model}`] : [] };
}

export interface DeserializeResult {
  nodes: RelayNode[];
  edges: Edge[];
  warnings: string[];
}

/**
 * Convert canonical Workflow JSON back into React Flow nodes + edges.
 *
 * Unfolds `lane_id` from LLM configs back into synthetic lane nodes.
 * Positions are computed from a topological left-to-right layout.
 * Unknown or unmappable kinds produce warnings and are skipped.
 */
export function deserializeWorkflow(wf: WorkflowJson): DeserializeResult {
  const warnings: string[] = [];
  const nodes: RelayNode[] = [];
  const edges: Edge[] = [];

  // Collect lane ids referenced by any LLM config.
  const referencedLanes = new Set<string>();
  for (const sn of wf.nodes) {
    if (sn.config.kind === "llm" && sn.config.lane_id) referencedLanes.add(sn.config.lane_id);
  }

  // Assign layout columns via topological sort.
  const adj = new Map<string, string[]>();
  const inDeg = new Map<string, number>();
  for (const n of wf.nodes) {
    adj.set(n.id, []);
    inDeg.set(n.id, 0);
  }
  for (const e of wf.edges) {
    adj.get(e.source_node)?.push(e.target_node);
    inDeg.set(e.target_node, (inDeg.get(e.target_node) ?? 0) + 1);
  }
  const queue: string[] = [];
  for (const [id, deg] of inDeg) { if (deg === 0) queue.push(id); }
  const topoOrder: string[] = [];
  while (queue.length) {
    const id = queue.shift()!;
    topoOrder.push(id);
    for (const next of adj.get(id) ?? []) {
      const d = (inDeg.get(next) ?? 1) - 1;
      inDeg.set(next, d);
      if (d === 0) queue.push(next);
    }
  }
  // Any remaining nodes not reachable from sources get appended.
  const inTopo = new Set(topoOrder);
  for (const n of wf.nodes) {
    if (!inTopo.has(n.id)) topoOrder.push(n.id);
  }

  // Compute column per node from topological order (sources before targets),
  // so depth is exact regardless of the node array's storage order.
  const col = new Map<string, number>();
  for (const id of topoOrder) {
    const srcCols = wf.edges.filter((e) => e.target_node === id).map((e) => col.get(e.source_node) ?? -1);
    col.set(id, srcCols.length ? 1 + Math.max(...srcCols) : 0);
  }
  const maxCol = Math.max(0, ...col.values());

  // Track y offset per column.
  const yOffset = new Map<number, number>();

  // Create a synthetic lane node.
  function makeLaneNode(laneId: string, feedCount: number): RelayNode {
    const info = laneConfig(laneId, wf.nodes);
    const c = maxCol + 1; // place lanes one column right of the rightmost schema node
    const y = yOffset.get(c) ?? 0;
    yOffset.set(c, y + SEQUENCE_GAP);
    return {
      id: `lane-${laneId}`,
      type: "relay",
      position: { x: c * SEQUENCE_GAP, y },
      data: { kind: "lane", title: info.title, lines: info.lines, status: "healthy", metaLeft: `feeds ${feedCount} node(s)` },
    };
  }

  const laneNodeCreated = new Set<string>();

  // Create schema nodes.
  const nodesById = new Map<string, SchemaNode>();
  for (const sn of wf.nodes) nodesById.set(sn.id, sn);

  for (const id of topoOrder) {
    const sn = nodesById.get(id);
    if (!sn) continue;
    const ek = KIND_REVERSE[sn.kind] ?? editorKindFromConfig(sn.config);
    if (ek === null) {
      warnings.push(`skipped unmappable node '${id}' (kind: ${sn.kind})`);
      continue;
    }
    const c = col.get(id) ?? 0;
    const y = yOffset.get(c) ?? 0;
    yOffset.set(c, y + SEQUENCE_GAP);
    nodes.push({
      id: sn.id,
      type: "relay",
      position: { x: c * SEQUENCE_GAP, y },
      data: {
        kind: ek,
        title: titleFromConfig(sn),
        lines: linesFromConfig(sn),
        metaLeft: sn.kind,
      },
    });

    // Emit synthetic lane node + edge if this node references a lane.
    if (sn.config.kind === "llm" && sn.config.lane_id) {
      const lid = sn.config.lane_id;
      if (!laneNodeCreated.has(lid)) {
        laneNodeCreated.add(lid);
        const feedCount = wf.nodes.filter((n) => n.config.kind === "llm" && n.config.lane_id === lid).length;
        const laneNode = makeLaneNode(lid, feedCount);
        nodes.push(laneNode);
        // Edge from lane node → this node.
        edges.push({ id: `lane-e-${lid}-${sn.id}`, source: laneNode.id, target: sn.id });
      }
    }
  }

  // Map schema edges → React Flow edges.
  for (const se of wf.edges) {
    // Skip edges to/from nodes we didn't emit (unmapped kinds, lanes).
    if (!nodesById.has(se.source_node) || !nodesById.has(se.target_node)) continue;
    const srcSchema = nodesById.get(se.source_node)!;
    const tgtSchema = nodesById.get(se.target_node)!;
    const srcKind = KIND_REVERSE[srcSchema.kind] ?? editorKindFromConfig(srcSchema.config);
    const tgtKind = KIND_REVERSE[tgtSchema.kind] ?? editorKindFromConfig(tgtSchema.config);
    if (!srcKind || !tgtKind) continue;
    if (srcKind === "lane" || tgtKind === "lane") continue;
    const newEdge: Edge = {
      id: `e-${se.source_node}-${se.source_port}-${se.target_node}`,
      source: se.source_node,
      target: se.target_node,
    };
    if (se.source_port !== "out") {
      newEdge.sourceHandle = se.source_port;
      newEdge.label = se.source_port;
    }
    edges.push(newEdge);
  }

  return { nodes, edges, warnings };
}

export function serializeWorkflow(
  nodes: RelayNode[],
  edges: Edge[],
  meta?: { id?: string; name?: string; version?: number },
  laneUrls: Record<string, string> = {},
): SerializeResult {
  const errors: string[] = [];
  const warnings: string[] = [];

  const inputCount = nodes.filter((n) => n.data.kind === "input").length;
  const outputCount = nodes.filter((n) => n.data.kind === "output").length;
  if (inputCount === 0) errors.push("workflow has no Input node");
  if (outputCount === 0) errors.push("workflow has no Output node");

  // Fold lane nodes: each lane node targets any downstream node; an
  // LLM-like node takes the lane of its feeding lane node.
  const laneIdsByNode = new Map<string, string>();
  const foldedLanes = new Map<string, string>();
  let lanesFolded = 0;
  for (const n of nodes) {
    if (n.data.kind !== "lane") continue;
    for (const e of edges) {
      if (e.source === n.id) {
        const target = nodes.find((x) => x.id === e.target);
        if (target && LLM_LIKE.has(target.data.kind)) {
          if (!laneIdsByNode.has(target.id)) {
            const laneId = n.data.title || n.id;
            laneIdsByNode.set(target.id, laneId);
            // Preserve the lane's URL if the caller supplied one.
            const url = laneUrls[laneId];
            if (url) {
              foldedLanes.set(laneId, url);
            } else {
              warnings.push(`lane '${laneId}' has no base URL — publish needs a URL for it`);
            }
            lanesFolded += 1;
          }
        }
      }
    }
  }

  // Map edges to schema edges. Port names: input source "out", target "in";
  // a branch (condition/fallback sourceHandle) is the source port.
  const schemaEdges: SchemaEdge[] = [];
  for (const e of edges) {
    const src = nodes.find((n) => n.id === e.source);
    const dst = nodes.find((n) => n.id === e.target);
    if (!src || !dst) {
      errors.push(`edge ${e.id} references a missing node`);
      continue;
    }
    // Skip edges to/from dropped kinds.
    if (DROPPED_KINDS.has(src.data.kind) || DROPPED_KINDS.has(dst.data.kind)) continue;
    schemaEdges.push({
      source_node: src.id,
      source_port: e.sourceHandle ?? "out",
      target_node: dst.id,
      target_port: "in",
    });
  }

  // Map nodes to schema nodes.
  const schemaNodes: SchemaNode[] = [];
  for (const n of nodes) {
    if (n.data.kind === "lane") continue; // folded, not a node
    if (DROPPED_KINDS.has(n.data.kind)) {
      warnings.push(`dropped display-only node '${n.id}' (${n.data.kind}) — not executed by the runtime`);
      continue;
    }
    const kind = kindFor(n.data.kind);
    if (!kind) {
      errors.push(`node '${n.id}' has no executable schema kind (${n.data.kind})`);
      continue;
    }
    const schemaKind = kind; // kindFor already maps provider→llm, route→router
    const laneId = LLM_LIKE.has(n.data.kind) ? laneIdsByNode.get(n.id) : undefined;
    const config = configFor(n, laneId);
    if (!config) {
      errors.push(
        n.data.kind === "condition"
          ? `node '${n.id}' is a Condition without a machine-readable condition — configure field/operator/value in the node inspector`
          : `node '${n.id}' could not be configured`,
      );
      continue;
    }
    const isCondition = n.data.kind === "condition";
    schemaNodes.push({
      id: n.id,
      kind: schemaKind as SchemaNodeKind,
      config,
      inputs: DEFAULT_IN,
      outputs: isCondition
        ? [
            { name: "true", port_type: "message" },
            { name: "false", port_type: "message" },
          ]
        : DEFAULT_OUT,
    });
  }

  const lanes = Object.fromEntries(foldedLanes) as Record<string, string>;

  if (errors.length > 0 || schemaNodes.length === 0) {
    return { workflow: null, errors, warnings, lanesFolded, lanes };
  }

  const workflow: WorkflowJson = {
    id: meta?.id ?? "workflow",
    name: meta?.name ?? "untitled",
    version: meta?.version ?? 1,
    nodes: schemaNodes,
    edges: schemaEdges,
  };

  return { workflow, errors, warnings, lanesFolded, lanes };
}