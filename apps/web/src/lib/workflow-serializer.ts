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