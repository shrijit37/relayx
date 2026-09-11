// Serialization: React Flow state <-> canonical Workflow

import {
  type Node,
  type Edge,
  MarkerType,
} from "@xyflow/react";
import {
  type Workflow,
  type WorkflowNode,
  type WorkflowEdge,
  type EdgeCondition,
  NodeKind,
  DEFAULT_CONFIGS,
  DEFAULT_PORTS,
} from "../types/workflow";

// The data payload attached to every React Flow node.
// Kept simple so it stays compatible with React Flow's Record<string, unknown>.
export interface RFNodeData {
  label: string;
  node: WorkflowNode;
  executionState?: ExecutionState;
}

import type { ExecutionState } from "../types/execution";

export type RFNodeType = Node;
export type RFEdgeType = Edge;

const NODE_NAMES: Record<NodeKind, string> = {
  [NodeKind.Input]: "Input",
  [NodeKind.Output]: "Output",
  [NodeKind.Llm]: "LLM",
  [NodeKind.Router]: "Router",
  [NodeKind.Transform]: "Transform",
  [NodeKind.Condition]: "Condition",
  [NodeKind.Mcp]: "MCP",
  [NodeKind.Skill]: "Skill",
};

export function createNode(
  kind: NodeKind,
  id: string,
  position: { x: number; y: number },
): RFNodeType {
  const config = DEFAULT_CONFIGS[kind]();
  const { inputs, outputs } = DEFAULT_PORTS[kind];
  const node: WorkflowNode = {
    id,
    kind,
    config,
    inputs,
    outputs,
  };
  return {
    id,
    position,
    type: kind,
    data: {
      label: NODE_NAMES[kind],
      node,
    },
  };
}

/** Convert canonical Workflow -> React Flow nodes + edges */
export function workflowToReactFlow(
  workflow: Workflow,
): { nodes: RFNodeType[]; edges: RFEdgeType[] } {
  const nodes: RFNodeType[] = workflow.nodes.map((node) => ({
    id: node.id,
    position: { x: 0, y: 0 },
    type: node.kind,
    data: {
      label: NODE_NAMES[node.kind],
      node,
    },
  }));

  layoutInColumn(nodes);

  const edges: RFEdgeType[] = workflow.edges.map((edge) => ({
    id: `${edge.source_node}:${edge.source_port}->${edge.target_node}:${edge.target_port}`,
    source: edge.source_node,
    sourceHandle: edge.source_port,
    target: edge.target_node,
    targetHandle: edge.target_port,
    markerEnd: { type: MarkerType.ArrowClosed },
  }));

  return { nodes, edges };
}

/** Convert React Flow nodes+edges -> canonical Workflow */
export function reactFlowToWorkflow(
  nodes: RFNodeType[],
  edges: RFEdgeType[],
  meta: { id: string; name: string; version: number },
): Workflow {
  const workflowNodes: WorkflowNode[] = nodes.map((rfNode) => {
    const data = rfNode.data as unknown as RFNodeData | undefined;
    if (!data?.node) {
      throw new Error(`Node ${rfNode.id} is missing canonical workflow data`);
    }
    return data.node;
  });

  const workflowEdges: WorkflowEdge[] = edges.map((edge) => {
    const cond = edge.data?.condition as EdgeCondition | undefined;
    return {
      source_node: edge.source,
      source_port: edge.sourceHandle ?? "output",
      target_node: edge.target,
      target_port: edge.targetHandle ?? "input",
      condition: cond,
    };
  });

  return {
    id: meta.id,
    name: meta.name,
    version: meta.version,
    nodes: workflowNodes,
    edges: workflowEdges,
  };
}

function layoutInColumn(nodes: RFNodeType[]): void {
  const GAP = 140;
  let y = 0;
  for (const node of nodes) {
    node.position = { x: 0, y };
    y += GAP;
  }
}

export function serializeWorkflow(workflow: Workflow): string {
  return JSON.stringify(workflow, null, 2);
}

export function deserializeWorkflow(json: string): Workflow {
  const parsed = JSON.parse(json) as Workflow;
  if (
    !parsed.id ||
    !Array.isArray(parsed.nodes) ||
    !Array.isArray(parsed.edges)
  ) {
    throw new Error("Invalid workflow JSON: missing id, nodes, or edges");
  }
  return parsed;
}

export { NODE_NAMES };
export type { PortDef } from "../types/workflow";