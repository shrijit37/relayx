/**
 * The canonical semantic workflow model (Phase 6.6).
 *
 * ONE model is the source of truth for workflow semantics. React Flow nodes
 * are views over this model, the serializer maps model ⇄ persisted
 * WorkflowJson, the inspector edits the model, and the compiler consumes the
 * same model. Titles/lines/badges are presentation only — never parsed back
 * into semantics.
 */

export const WORKFLOW_SCHEMA_VERSION = 2 as const;

/** Editor node kinds (canonical type space). */
export type EditorKind =
  | "input" | "output" | "transform" | "condition" | "route" | "lane"
  | "fallback" | "retry" | "provider" | "endpoint" | "mcp" | "tool"
  | "skill" | "agent" | "policy" | "observability";

export interface CanonicalPort {
  name: string;
  direction: "input" | "output";
  portType: "message" | "stream" | "tool_call" | "tool_result" | "json" | "bool";
  cardinality: "single" | "multi";
}

/** Shared LLM request knobs (provider/model/lane are explicit references,
 *  never derived from a title). */
export interface LlmRequestConfig {
  provider?: string;
  model?: string;
  lane?: string;
  protocol?: string;
  temperature?: number;
  maxTokens?: number;
  stream: boolean;
}

export type ConditionOperator =
  | "equals" | "not_equals" | "greater_than" | "less_than"
  | "contains" | "not_contains" | "is_empty" | "is_not_empty";

export interface ConditionValue {
  field: string;
  operator: ConditionOperator;
  value: string;
  valueType: "string" | "number" | "boolean";
}

export interface RetryPolicy {
  maxAttempts: number;
  delayMs: number;
  onTimeout: boolean;
  onProviderError: boolean;
  /** HTTP status codes that trigger an unconditional retry regardless of
   *  `onTimeout`/`onProviderError` (e.g. [429] for rate limits). */
  retryOn?: number[];
}

export interface FallbackEntryConfig {
  lane: string;
  /** Model override for this provider lane. Rust FallbackProvider.model is
   *  required on the wire — a blank value is a publish-time validation error,
   *  never fabricated. */
  model?: string;
  /** Optional protocol override per entry (mirrors Rust FallbackProvider.protocol).
   *  Omitted = lane default. */
  protocol?: string;
  /**
   * @deprecated future MCP/tool support — NOT on the wire. Setting it is a
   * publish-blocking validation error (never silently dropped).
   */
  capabilityRef?: string;
}

export interface FallbackConfig {
  providers: FallbackEntryConfig[];
  rounds: number;
  /** How providers are selected across requests: "sequential" (always
   *  start at index 0) or "round_robin" (each request starts at the next
   *  provider, spreading traffic across egress IPs). */
  strategy?: "sequential" | "round_robin";
  /** HTTP status codes that trigger immediate failover to the next provider
   *  in the current round (e.g. [429] for rate-limit rotation). */
  retryOn?: number[];
}

export interface McpToolRef {
  /** Canonical capability reference, e.g. "mcp://server/tool". */
  capabilityRef: string;
  serverRef?: string;
  toolName?: string;
  deferred: boolean;
}

export interface SkillRef {
  skillRef: string;
  progressive: boolean;
}

/** Expected input variable definition. */
export interface InputVariable {
  name: string;
  type: "string" | "number" | "boolean" | "object" | "array";
  description?: string;
  required?: boolean;
}

/** Per-kind semantic configuration. Mirrors workflow_schema::NodeConfig. */
export type CanonicalConfig =
  | { kind: "input"; inputType?: string; description?: string; variables?: InputVariable[]; value?: unknown }
  | { kind: "output"; value?: unknown }
  | { kind: "llm"; config: LlmRequestConfig }
  | { kind: "router"; strategy: "first_match" | "round_robin" }
  | { kind: "transform"; operation: "passthrough" | "extract" | "merge" | "filter" }
  | { kind: "condition"; condition: ConditionValue }
  | { kind: "mcp"; tool: McpToolRef }
  | { kind: "skill"; skill: SkillRef }
  | { kind: "fallback"; fallback: FallbackConfig }
  | { kind: "retry"; policy: RetryPolicy; target: LlmRequestConfig };

/** The canonical node every surface operates on. */
export interface CanonicalNode {
  id: string;
  type: EditorKind;
  version: number;
  position: { x: number; y: number };
  ports: CanonicalPort[];
  config: CanonicalConfig;
  /** Display-only. Never parsed by the serializer or runtime. */
  presentation?: {
    title?: string;
    notes?: string;
  };
}

/** Canonical connection with explicit ports. */
export interface CanonicalEdge {
  id: string;
  source: string;
  sourcePort: string;
  target: string;
  targetPort: string;
  label?: string;
}

export interface CanonicalWorkflow {
  id: string;
  name: string;
  version: number;
  schemaVersion: typeof WORKFLOW_SCHEMA_VERSION;
  nodes: CanonicalNode[];
  edges: CanonicalEdge[];
}
