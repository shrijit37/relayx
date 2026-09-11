import {
  type Workflow,
  type WorkflowNode,
  NodeKind,
} from "../types/workflow";

export interface ValidationError {
  node_id?: string;
  message: string;
  severity: "error" | "warning";
}

export function validateWorkflow(workflow: Workflow): ValidationError[] {
  const errors: ValidationError[] = [];
  const nodeIds = new Set(workflow.nodes.map((n) => n.id));

  if (workflow.nodes.length === 0) {
    errors.push({
      message: "Workflow is empty",
      severity: "error",
    });
    return errors;
  }

  // Check for duplicate node IDs
  const seen = new Set<string>();
  for (const node of workflow.nodes) {
    if (seen.has(node.id)) {
      errors.push({
        node_id: node.id,
        message: `Duplicate node ID: ${node.id}`,
        severity: "error",
      });
    }
    seen.add(node.id);
  }

  // Validate edges reference existing nodes and ports
  for (const edge of workflow.edges) {
    if (!nodeIds.has(edge.source_node)) {
      errors.push({
        message: `Edge references unknown source node: ${edge.source_node}`,
        severity: "error",
      });
    }
    if (!nodeIds.has(edge.target_node)) {
      errors.push({
        message: `Edge references unknown target node: ${edge.target_node}`,
        severity: "error",
      });
    }
  }

  // Must have exactly one Input node
  const inputNodes = workflow.nodes.filter((n) => n.kind === NodeKind.Input);
  if (inputNodes.length === 0) {
    errors.push({
      message: "Workflow must have an Input node",
      severity: "error",
    });
  } else if (inputNodes.length > 1) {
    errors.push({
      message: "Workflow must have exactly one Input node",
      severity: "error",
    });
  }

  // Must have exactly one Output node
  const outputNodes = workflow.nodes.filter((n) => n.kind === NodeKind.Output);
  if (outputNodes.length === 0) {
    errors.push({
      message: "Workflow must have an Output node",
      severity: "error",
    });
  } else if (outputNodes.length > 1) {
    errors.push({
      message: "Workflow must have exactly one Output node",
      severity: "error",
    });
  }

  // Check for cycles (DFS)
  if (hasCycle(workflow.nodes, workflow.edges)) {
    errors.push({
      message: "Workflow contains a cycle",
      severity: "error",
    });
  }

  // Check reachability from Input
  if (inputNodes.length === 1) {
    const reachable = getReachable(
      inputNodes[0]!,
      workflow.nodes,
      workflow.edges,
    );
    for (const node of workflow.nodes) {
      if (!reachable.has(node.id) && node.kind !== NodeKind.Input) {
        errors.push({
          node_id: node.id,
          message: `Node "${node.id}" is unreachable from Input`,
          severity: "warning",
        });
      }
    }
  }

  // Check node-specific config issues
  for (const node of workflow.nodes) {
    validateNodeConfig(node, errors);
  }

  return errors;
}

function validateNodeConfig(
  node: WorkflowNode,
  errors: ValidationError[],
): void {
  switch (node.kind) {
    case NodeKind.Llm: {
      const cfg = node.config as { model?: string; stream: boolean };
      if (!cfg.model) {
        errors.push({
          node_id: node.id,
          message: "LLM node: model is required",
          severity: "error",
        });
      }
      break;
    }
    case NodeKind.Mcp: {
      const cfg = node.config as {
        server_ref: string;
        tool_name: string;
      };
      if (!cfg.server_ref) {
        errors.push({
          node_id: node.id,
          message: "MCP node: server reference is required",
          severity: "error",
        });
      }
      if (!cfg.tool_name) {
        errors.push({
          node_id: node.id,
          message: "MCP node: tool name is required",
          severity: "error",
        });
      }
      break;
    }
    case NodeKind.Skill: {
      const cfg = node.config as { skill_ref: string };
      if (!cfg.skill_ref) {
        errors.push({
          node_id: node.id,
          message: "Skill node: skill reference is required",
          severity: "error",
        });
      }
      break;
    }
    case NodeKind.Condition: {
      const cfg = node.config as {
        field: string;
        value: string;
      };
      if (!cfg.field) {
        errors.push({
          node_id: node.id,
          message: "Condition node: field is required",
          severity: "error",
        });
      }
      if (!cfg.value) {
        errors.push({
          node_id: node.id,
          message: "Condition node: comparison value is required",
          severity: "error",
        });
      }
      break;
    }
  }
}

function hasCycle(
  nodes: WorkflowNode[],
  edges: { source_node: string; target_node: string }[],
): boolean {
  const adj = new Map<string, string[]>();
  for (const node of nodes) adj.set(node.id, []);
  for (const edge of edges) {
    adj.get(edge.source_node)?.push(edge.target_node);
  }

  const WHITE = 0,
    GRAY = 1,
    BLACK = 2;
  const color = new Map<string, number>();
  for (const node of nodes) color.set(node.id, WHITE);

  for (const node of nodes) {
    if (color.get(node.id) !== WHITE) continue;
    const stack: { id: string; entering: boolean }[] = [
      { id: node.id, entering: true },
    ];
    while (stack.length > 0) {
      const cur = stack.pop()!;
      if (cur.entering) {
        if (color.get(cur.id) === GRAY) return true;
        color.set(cur.id, GRAY);
        stack.push({ id: cur.id, entering: false });
        for (const next of adj.get(cur.id) ?? []) {
          if (color.get(next) !== BLACK) {
            stack.push({ id: next, entering: true });
          }
        }
      } else {
        color.set(cur.id, BLACK);
      }
    }
  }
  return false;
}

function getReachable(
  start: WorkflowNode,
  nodes: WorkflowNode[],
  edges: { source_node: string; target_node: string }[],
): Set<string> {
  const adj = new Map<string, string[]>();
  for (const node of nodes) adj.set(node.id, []);
  for (const edge of edges) {
    adj.get(edge.source_node)?.push(edge.target_node);
  }

  const visited = new Set<string>();
  const queue = [start.id];
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (visited.has(id)) continue;
    visited.add(id);
    for (const next of adj.get(id) ?? []) {
      if (!visited.has(next)) queue.push(next);
    }
  }
  return visited;
}
