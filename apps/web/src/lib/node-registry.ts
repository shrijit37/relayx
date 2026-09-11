import { NodeKind, PortType, type PortDef } from "../types/workflow";

export interface NodeTypeInfo {
  kind: NodeKind;
  label: string;
  description: string;
  icon: string;
  category: "flow" | "llm" | "integration";
  defaultInputs: PortDef[];
  defaultOutputs: PortDef[];
}

const P = PortType;

export const NODE_REGISTRY: NodeTypeInfo[] = [
  {
    kind: NodeKind.Input,
    label: "Input",
    description: "Receives initial workflow input",
    icon: "▶",
    category: "flow",
    defaultInputs: [],
    defaultOutputs: [{ name: "output", port_type: P.Message }],
  },
  {
    kind: NodeKind.Output,
    label: "Output",
    description: "Produces final workflow output",
    icon: "⏹",
    category: "flow",
    defaultInputs: [{ name: "input", port_type: P.Message }],
    defaultOutputs: [],
  },
  {
    kind: NodeKind.Llm,
    label: "LLM",
    description: "Calls an LLM provider via protocol engine",
    icon: "🤖",
    category: "llm",
    defaultInputs: [
      { name: "messages", port_type: P.Message },
      { name: "tools", port_type: P.ToolCall },
    ],
    defaultOutputs: [
      { name: "response", port_type: P.Message },
      { name: "tool_calls", port_type: P.ToolCall },
    ],
  },
  {
    kind: NodeKind.Router,
    label: "Router",
    description: "Routes to one of several downstream paths",
    icon: "🔀",
    category: "flow",
    defaultInputs: [{ name: "input", port_type: P.Message }],
    defaultOutputs: [
      { name: "route_1", port_type: P.Message },
      { name: "route_2", port_type: P.Message },
      { name: "default", port_type: P.Message },
    ],
  },
  {
    kind: NodeKind.Transform,
    label: "Transform",
    description: "Transforms data between nodes",
    icon: "⚙",
    category: "flow",
    defaultInputs: [{ name: "input", port_type: P.Message }],
    defaultOutputs: [{ name: "output", port_type: P.Message }],
  },
  {
    kind: NodeKind.Condition,
    label: "Condition",
    description: "Branches based on a condition",
    icon: "◇",
    category: "flow",
    defaultInputs: [{ name: "input", port_type: P.Message }],
    defaultOutputs: [
      { name: "true", port_type: P.Message },
      { name: "false", port_type: P.Message },
    ],
  },
  {
    kind: NodeKind.Mcp,
    label: "MCP",
    description: "Invokes an MCP tool",
    icon: "🔧",
    category: "integration",
    defaultInputs: [
      { name: "input", port_type: P.Message },
      { name: "tool_result", port_type: P.ToolResult },
    ],
    defaultOutputs: [
      { name: "output", port_type: P.Message },
      { name: "tool_call", port_type: P.ToolCall },
    ],
  },
  {
    kind: NodeKind.Skill,
    label: "Skill",
    description: "Loads and applies a Skill",
    icon: "⚡",
    category: "integration",
    defaultInputs: [{ name: "input", port_type: P.Message }],
    defaultOutputs: [{ name: "output", port_type: P.Message }],
  },
];

export function getNodeTypeInfo(kind: NodeKind): NodeTypeInfo {
  return NODE_REGISTRY.find((n) => n.kind === kind) ?? NODE_REGISTRY[0]!;
}

export function canConnect(
  sourceType: string,
  targetType: string,
): boolean {
  const COMPAT: Record<string, string[]> = {
    Message: ["Message", "Stream"],
    Stream: ["Stream", "Message"],
    ToolCall: ["ToolCall", "ToolResult"],
    ToolResult: ["ToolResult", "ToolCall"],
    Json: ["Json", "Message"],
    Bool: ["Bool"],
  };
  return COMPAT[sourceType]?.includes(targetType) ?? false;
}