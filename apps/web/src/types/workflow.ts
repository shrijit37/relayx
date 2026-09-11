// Canonical workflow types — mirrors crates/workflow-schema/src/lib.rs

export type NodeId = string;
export type PortName = string;

export enum NodeKind {
  Input = "Input",
  Output = "Output",
  Llm = "Llm",
  Router = "Router",
  Transform = "Transform",
  Condition = "Condition",
  Mcp = "Mcp",
  Skill = "Skill",
}

export enum PortType {
  Message = "Message",
  Stream = "Stream",
  ToolCall = "ToolCall",
  ToolResult = "ToolResult",
  Json = "Json",
  Bool = "Bool",
}

export enum RouterStrategy {
  FirstMatch = "FirstMatch",
  RoundRobin = "RoundRobin",
  LoadBased = "LoadBased",
}

export enum TransformOperation {
  Passthrough = "Passthrough",
  Extract = "Extract",
  Merge = "Merge",
  Filter = "Filter",
}

export enum ConditionOp {
  Equal = "Equal",
  NotEqual = "NotEqual",
  GreaterThan = "GreaterThan",
  LessThan = "LessThan",
  Contains = "Contains",
  NotContains = "NotContains",
  IsEmpty = "IsEmpty",
  IsNotEmpty = "IsNotEmpty",
}

// --- Node configs (tagged by kind) ---

export interface InputConfig {
  input_type: PortType;
}

export interface OutputConfig {
  output_type: PortType;
}

export interface LlmConfig {
  protocol?: string;
  model?: string;
  temperature?: number;
  max_tokens?: number;
  stream: boolean;
  lane_id?: string;
}

export interface RouterConfig {
  strategy: RouterStrategy;
}

export interface TransformConfig {
  operation: TransformOperation;
}

export interface ConditionConfig {
  field: string;
  operator: ConditionOp;
  value: string;
  condition: string;
}

export interface McpConfig {
  server_ref: string;
  tool_name: string;
  deferred: boolean;
}

export interface SkillConfig {
  skill_ref: string;
  progressive: boolean;
}

export type NodeConfig =
  | InputConfig
  | OutputConfig
  | LlmConfig
  | RouterConfig
  | TransformConfig
  | ConditionConfig
  | McpConfig
  | SkillConfig;

// --- Port & Edge ---

export interface PortDef {
  name: PortName;
  port_type: PortType;
}

export interface EdgeCondition {
  field: string;
  operator: ConditionOp;
  value: string;
}

export interface WorkflowEdge {
  source_node: NodeId;
  source_port: PortName;
  target_node: NodeId;
  target_port: PortName;
  condition?: EdgeCondition;
}

// --- Node & Workflow ---

export interface WorkflowNode {
  id: NodeId;
  kind: NodeKind;
  config: NodeConfig;
  inputs: PortDef[];
  outputs: PortDef[];
}

export interface Workflow {
  id: string;
  name: string;
  version: number;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
}

// --- Default configs per node kind ---

export const DEFAULT_CONFIGS: Record<NodeKind, () => NodeConfig> = {
  [NodeKind.Input]: () => ({ input_type: PortType.Message } as InputConfig),
  [NodeKind.Output]: () => ({ output_type: PortType.Message } as OutputConfig),
  [NodeKind.Llm]: () =>
    ({
      stream: true,
      temperature: 0.7,
      max_tokens: 4096,
    }) as LlmConfig,
  [NodeKind.Router]: () =>
    ({ strategy: RouterStrategy.FirstMatch }) as RouterConfig,
  [NodeKind.Transform]: () =>
    ({ operation: TransformOperation.Passthrough }) as TransformConfig,
  [NodeKind.Condition]: () =>
    ({
      field: "",
      operator: ConditionOp.Equal,
      value: "",
      condition: "",
    }) as ConditionConfig,
  [NodeKind.Mcp]: () =>
    ({
      server_ref: "",
      tool_name: "",
      deferred: false,
    }) as McpConfig,
  [NodeKind.Skill]: () =>
    ({
      skill_ref: "",
      progressive: true,
    }) as SkillConfig,
};

// --- Default ports per node kind ---

export const DEFAULT_PORTS: Record<
  NodeKind,
  { inputs: PortDef[]; outputs: PortDef[] }
> = {
  [NodeKind.Input]: {
    inputs: [],
    outputs: [{ name: "output", port_type: PortType.Message }],
  },
  [NodeKind.Output]: {
    inputs: [{ name: "input", port_type: PortType.Message }],
    outputs: [],
  },
  [NodeKind.Llm]: {
    inputs: [
      { name: "messages", port_type: PortType.Message },
      { name: "tools", port_type: PortType.ToolCall },
    ],
    outputs: [
      { name: "response", port_type: PortType.Message },
      { name: "tool_calls", port_type: PortType.ToolCall },
    ],
  },
  [NodeKind.Router]: {
    inputs: [{ name: "input", port_type: PortType.Message }],
    outputs: [
      { name: "route_1", port_type: PortType.Message },
      { name: "route_2", port_type: PortType.Message },
      { name: "default", port_type: PortType.Message },
    ],
  },
  [NodeKind.Transform]: {
    inputs: [{ name: "input", port_type: PortType.Message }],
    outputs: [{ name: "output", port_type: PortType.Message }],
  },
  [NodeKind.Condition]: {
    inputs: [{ name: "input", port_type: PortType.Message }],
    outputs: [
      { name: "true", port_type: PortType.Message },
      { name: "false", port_type: PortType.Message },
    ],
  },
  [NodeKind.Mcp]: {
    inputs: [
      { name: "input", port_type: PortType.Message },
      { name: "tool_result", port_type: PortType.ToolResult },
    ],
    outputs: [
      { name: "output", port_type: PortType.Message },
      { name: "tool_call", port_type: PortType.ToolCall },
    ],
  },
  [NodeKind.Skill]: {
    inputs: [{ name: "input", port_type: PortType.Message }],
    outputs: [{ name: "output", port_type: PortType.Message }],
  },
};
