/**
 * Node definitions (Phase 6.6 §6).
 *
 * A NodeDefinition describes what a node type MEANS: ports, config schema,
 * defaults and validation rules. Node instances (CanonicalNode) store
 * values; definitions never hold mutable workflow state.
 */

import type {
  CanonicalConfig,
  CanonicalNode,
  ConditionOperator,
  EditorKind,
  InputVariable,
  LlmRequestConfig,
} from "./nodes";

export type FieldType =
  | "string" | "number" | "integer" | "boolean" | "enum"
  | "reference" | "provider" | "model" | "lane";

export interface FieldOption {
  value: string;
  label: string;
}

export interface FieldDef {
  name: string;
  label: string;
  type: FieldType;
  required?: boolean;
  default?: unknown;
  min?: number;
  max?: number;
  step?: number;
  options?: FieldOption[];
  /** Option source (control-plane tables) for reference fields. */
  reference?: "lanes" | "providers" | "models";
  placeholder?: string;
  help?: string;
  /** Show this field only when another field has this value (| separated). */
  dependsOn?: { field: string; value: string };
}

export interface PortDef {
  name: string;
  direction: "input" | "output";
  portType: CanonicalNode["ports"][number]["portType"];
  required?: boolean;
  cardinality?: "single" | "multi";
}

export interface NodeDefinition {
  type: EditorKind;
  schemaVersion: number;
  label: string;
  inputs: PortDef[];
  outputs: PortDef[];
  fields: FieldDef[];
  /** Default config for a freshly created node. */
  defaults: () => CanonicalConfig;
  /** Display-only title derived from config (never the reverse). */
  displayTitle: (c: CanonicalConfig) => string;
  displayLines?: (c: CanonicalConfig) => string[];
  /** Whether the runtime can execute this kind (Phase 6.6 §3.2). */
  executable: boolean;
  note?: string;
}

export const CONDITION_OPERATORS: { value: ConditionOperator; label: string }[] = [
  { value: "equals", label: "equals" },
  { value: "not_equals", label: "not equals" },
  { value: "greater_than", label: "greater than" },
  { value: "less_than", label: "less than" },
  { value: "contains", label: "contains" },
  { value: "not_contains", label: "not contains" },
  { value: "is_empty", label: "is empty" },
  { value: "is_not_empty", label: "is not empty" },
];

export const STREAM_PROTOCOLS = [
  { value: "openai_chat", label: "OpenAI Chat" },
  { value: "openai_responses", label: "OpenAI Responses" },
  { value: "anthropic", label: "Anthropic Messages" },
];

const LLM_FIELDS: FieldDef[] = [
  { name: "provider", label: "Provider", type: "provider", reference: "providers", placeholder: "e.g. anthropic" },
  { name: "model", label: "Model", type: "model", reference: "models", placeholder: "e.g. claude-sonnet" },
  { name: "lane", label: "Lane", type: "lane", reference: "lanes", required: true, placeholder: "lane id" },
  { name: "protocol", label: "Protocol", type: "enum", options: STREAM_PROTOCOLS, placeholder: "lane default" },
  { name: "temperature", label: "Temperature", type: "number", min: 0, max: 2, step: 0.1, default: 0.2 },
  { name: "maxTokens", label: "Max tokens", type: "integer", min: 1, default: 1024 },
  { name: "stream", label: "Streaming", type: "boolean", default: true },
];

const EMPTY_LLM: LlmRequestConfig = { temperature: 0.2, maxTokens: 1024, stream: true };

const INPUT_PORT_TYPES = [
  { value: "message", label: "Message" },
  { value: "stream", label: "Stream" },
  { value: "json", label: "JSON" },
  { value: "tool_call", label: "Tool call" },
  { value: "tool_result", label: "Tool result" },
  { value: "bool", label: "Boolean" },
];

const INPUT_FIELDS: FieldDef[] = [
  { name: "inputType", label: "Input type", type: "enum", default: "message", options: INPUT_PORT_TYPES,
    help: "The type of data this workflow accepts as input." },
  { name: "description", label: "Description", type: "string", placeholder: "What this workflow accepts as input",
    help: "Human-readable; does not affect execution." },
];

export const VARIABLE_TYPES = [
  { value: "string", label: "String" },
  { value: "number", label: "Number" },
  { value: "boolean", label: "Boolean" },
  { value: "object", label: "Object" },
  { value: "array", label: "Array" },
];

const defs: Record<EditorKind, NodeDefinition> = {
  input: {
    type: "input", schemaVersion: 2, label: "Input",
    inputs: [], outputs: [{ name: "out", direction: "output", portType: "message", required: true }],
    fields: INPUT_FIELDS,
    defaults: () => ({ kind: "input", inputType: "message", description: "", variables: [] }),
    displayTitle: (c) => (c.kind === "input" ? `Input · ${c.inputType ?? "message"}` : "Input"),
    displayLines: (c) => c.kind === "input"
      ? [c.inputType ?? "message", ...(c.variables?.length ? [`${c.variables.length} variables`] : [])]
      : [],
    executable: true,
  },
  output: {
    type: "output", schemaVersion: 1, label: "Output",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }], outputs: [],
    fields: [],
    defaults: () => ({ kind: "output" }),
    displayTitle: () => "Streaming Response",
    displayLines: () => ["egress"],
    executable: true,
  },
  provider: {
    type: "provider", schemaVersion: 1, label: "Provider (LLM)",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }],
    outputs: [{ name: "out", direction: "output", portType: "message", required: true }],
    fields: LLM_FIELDS,
    defaults: () => ({ kind: "llm", config: { ...EMPTY_LLM } }),
    displayTitle: (c) => (c.kind === "llm" && c.config.model ? `Model · ${c.config.model}` : "LLM"),
    displayLines: (c) => {
      if (c.kind !== "llm") return [];
      const parts: string[] = [];
      if (c.config.provider) parts.push(c.config.provider);
      if (c.config.lane) parts.push(`lane: ${c.config.lane}`);
      if (c.config.stream) parts.push("streaming");
      return parts.length ? parts : ["llm"];
    },
    executable: true,
  },
  route: {
    type: "route", schemaVersion: 1, label: "Model Router",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }],
    outputs: [{ name: "out", direction: "output", portType: "message", required: true }],
    fields: [
      { name: "strategy", label: "Strategy", type: "enum", required: true, default: "round_robin",
        options: [
          { value: "first_match", label: "first match" },
          { value: "round_robin", label: "round robin" },
        ] },
    ],
    defaults: () => ({ kind: "router", strategy: "round_robin" }),
    displayTitle: () => "Model Router",
    displayLines: (c) => (c.kind === "router" ? [`strategy: ${c.strategy}`] : []),
    executable: true,
  },
  transform: {
    type: "transform", schemaVersion: 1, label: "Transform",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }],
    outputs: [{ name: "out", direction: "output", portType: "message", required: true }],
    fields: [
      { name: "operation", label: "Operation", type: "enum", required: true, default: "passthrough",
        options: [
          { value: "passthrough", label: "passthrough" },
          { value: "extract", label: "extract" },
          { value: "merge", label: "merge" },
          { value: "filter", label: "filter" },
        ] },
    ],
    defaults: () => ({ kind: "transform", operation: "passthrough" }),
    displayTitle: () => "Transform",
    displayLines: (c) => (c.kind === "transform" ? [`operation: ${c.operation}`] : []),
    executable: true,
  },
  condition: {
    type: "condition", schemaVersion: 1, label: "Condition",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }],
    outputs: [
      { name: "true", direction: "output", portType: "message" },
      { name: "false", direction: "output", portType: "message" },
    ],
    fields: [
      { name: "field", label: "Field", type: "string", required: true, placeholder: "e.g. user.intent" },
      { name: "operator", label: "Operator", type: "enum", required: true, default: "equals", options: CONDITION_OPERATORS },
      { name: "value", label: "Value", type: "string", required: true, placeholder: "e.g. refund", dependsOn: { field: "operator", value: "equals|not_equals|greater_than|less_than|contains|not_contains" } },
      { name: "valueType", label: "Value type", type: "enum", required: true, default: "string",
        options: [
          { value: "string", label: "string" },
          { value: "number", label: "number" },
          { value: "boolean", label: "boolean" },
        ] },
    ],
    defaults: () => ({
      kind: "condition",
      condition: { field: "", operator: "equals", value: "", valueType: "string" },
    }),
    displayTitle: () => "Condition",
    displayLines: (c) => (c.kind === "condition" ? [`${c.condition.field || "?"} ${c.condition.operator} ${c.condition.value || "?"}`] : []),
    executable: true,
  },
  mcp: {
    type: "mcp", schemaVersion: 1, label: "MCP Tool",
    inputs: [{ name: "in", direction: "input", portType: "message" }],
    outputs: [{ name: "out", direction: "output", portType: "message" }],
    fields: [
      { name: "capabilityRef", label: "Capability ref", type: "string", required: true, placeholder: "mcp://server/tool" },
      { name: "deferred", label: "Deferred schema", type: "boolean", default: true },
    ],
    defaults: () => ({ kind: "mcp", tool: { capabilityRef: "", deferred: true } }),
    displayTitle: (c) => (c.kind === "mcp" ? (c.tool.capabilityRef || "MCP Tool") : "MCP"),
    executable: true,
    note: "MCP execution ships in a later phase; unconfigured references block publish.",
  },
  skill: {
    type: "skill", schemaVersion: 1, label: "Skill",
    inputs: [{ name: "in", direction: "input", portType: "message" }],
    outputs: [{ name: "out", direction: "output", portType: "message" }],
    fields: [
      { name: "skillRef", label: "Skill ref", type: "string", required: true, placeholder: "skill id" },
      { name: "progressive", label: "Progressive loading", type: "boolean", default: true },
    ],
    defaults: () => ({ kind: "skill", skill: { skillRef: "", progressive: true } }),
    displayTitle: (c) => (c.kind === "skill" ? (c.skill.skillRef || "Skill") : "Skill"),
    executable: true,
    note: "Skill loading ships in a later phase; unconfigured references block publish.",
  },
  fallback: {
    type: "fallback", schemaVersion: 1, label: "Fallback",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }],
    outputs: [{ name: "out", direction: "output", portType: "message", required: true }],
    fields: [
      { name: "rounds", label: "Rounds", type: "integer", min: 0, default: 1 },
    ],
    defaults: () => ({ kind: "fallback", fallback: { providers: [], rounds: 1 } }),
    displayTitle: () => "Fallback",
    displayLines: (c) => (c.kind === "fallback" ? [`${c.fallback.providers.length} fallback(s)`] : []),
    executable: true,
  },
  retry: {
    type: "retry", schemaVersion: 1, label: "Retry",
    inputs: [{ name: "in", direction: "input", portType: "message", required: true }],
    outputs: [{ name: "out", direction: "output", portType: "message", required: true }],
    fields: [
      { name: "maxAttempts", label: "Max attempts", type: "integer", required: true, min: 1, default: 2 },
      { name: "delayMs", label: "Delay (ms)", type: "integer", required: true, min: 0, default: 1000 },
      { name: "onTimeout", label: "Retry on timeout", type: "boolean", default: true },
      { name: "onProviderError", label: "Retry on provider error", type: "boolean", default: true },
      ...LLM_FIELDS.slice(0, 3).map((f) => ({ ...f, name: `target.${f.name}` as const, label: `Target ${f.label}` })),
    ],
    defaults: () => ({
      kind: "retry",
      policy: { maxAttempts: 2, delayMs: 1000, onTimeout: true, onProviderError: true },
      target: { ...EMPTY_LLM },
    }),
    displayTitle: (c) => (c.kind === "retry" ? `Retry · ${c.policy.maxAttempts} attempts` : "Retry"),
    displayLines: (c) => (c.kind === "retry" ? [`delay: ${c.policy.delayMs}ms`] : []),
    executable: true,
  },
  // ── Display-only kinds (Phase 6.6 §3.2: explicit unsupported, never dropped) ──
  lane: displayOnly("lane", "Lane", "display-only until lanes are control-plane references."),
  endpoint: displayOnly("endpoint", "Endpoint", "display-only; runtime endpoints come from lane records."),
  tool: displayOnly("tool", "Tool Activation", "display-only; use MCP with a capability reference."),
  agent: displayOnly("agent", "Agent / Model", "display-only; agent loops are not executed."),
  policy: displayOnly("policy", "Policy", "display-only; policies are control-plane records."),
  observability: displayOnly("observability", "Observability", "display-only; telemetry is configured on the data plane."),
};

function displayOnly(type: EditorKind, label: string, note: string): NodeDefinition {
  return {
    type, schemaVersion: 1, label,
    inputs: [], outputs: [],
    fields: [],
    defaults: (): CanonicalConfig => ({ kind: "mcp", tool: { capabilityRef: "", deferred: true } }),
    displayTitle: () => label,
    displayLines: () => ["not executed"],
    executable: false,
    note,
  };
}

/** Look up the definition for an editor kind. */
export function getNodeDefinition(type: EditorKind): NodeDefinition {
  return defs[type];
}

/** Default config for a newly created node of the given kind. */
export function defaultConfigFor(type: EditorKind): CanonicalConfig {
  return defs[type].defaults();
}

/** Ports implied by a node type (canonical, schema-driven). */
export function portsForType(type: EditorKind): CanonicalNode["ports"] {
  const def = defs[type];
  return [
    ...def.inputs.map((p) => ({ name: p.name, direction: "input" as const, portType: p.portType, cardinality: (p.cardinality ?? "single") as "single" | "multi" })),
    ...def.outputs.map((p) => ({ name: p.name, direction: "output" as const, portType: p.portType, cardinality: (p.cardinality ?? "single") as "single" | "multi" })),
  ];
}
