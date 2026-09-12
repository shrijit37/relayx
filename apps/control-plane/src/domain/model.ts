/**
 * Control-plane domain model.
 *
 * These are the durable, backend-authoritative types that cross the
 * API/DB boundary. They are deliberately separate from:
 *  - the wire DTOs the frontend sees (handled in routes.ts)
 *  - the Rust `workflow_schema::Workflow` (serialized as `workflow_json` raw)
 *
 * `WorkflowLifecycle` is the only legal state transition table. A publication
 * is the only path into ACTIVE; every ACTIVE version is an immutable,
 * known-good runtime artifact.
 */

export const WORKFLOW_STATUS = [
  "draft",
  "validated",
  "compiled",
  "published",
  "active",
] as const;

export type WorkflowStatus = (typeof WORKFLOW_STATUS)[number];

/** The workflow itself lives in the DB as canonical Workflow JSON. */
export type WorkflowJson = {
  id: string;
  name: string;
  version: number;
  nodes: unknown[];
  edges: unknown[];
};

export type CredentialRef = {
  /** Reference id resolved by the secret backend (Phase 7). Never the secret. */
  ref: string;
  /** Backend hint for resolution, e.g. "env" or "vault". Low-cardinality. */
  provider: "env" | "inline" | "vault";
};

export type ProviderRecord = {
  id: string;
  project_id: string;
  name: string;
  protocol: string;
  base_url: string;
  model: string;
};

export type LaneRecord = {
  id: string;
  project_id: string;
  provider_id: string | null;
  endpoint: string;
  /** Pre-resolved wire URL (base_url authority). */
  base_url: string;
  /** Egress policy tag; unused by the runtime in this phase. */
  egress: string;
  /** Policy tags; unused by the runtime in this phase. */
  policies: string[];
  /** Credential reference resolved at publish time; never the raw secret. */
  credential_ref: CredentialRef | null;
};

export type PolicyRecord = {
  id: string;
  project_id: string;
  name: string;
  rules: unknown;
};

export type PublicationRecord = {
  id: string;
  workflow_id: string;
  workflow_version: number;
  plan_hash: string;
  snapshot_version: number;
  status: "succeeded" | "failed";
  error: string | null;
  published_at: string;
};

/**
 * Legal lifecycle transitions.
 *
 * - A version starts DRAFT.
 * - `VALIDATED` = schema + lane validation passed (compile dry-run on the gateway).
 * - `COMPILED` = a plan hash has been recorded (gateway compile succeeded).
 * - `PUBLISHED` = the bundle was atomically published to the gateway.
 * - `ACTIVE` = the highest published version served by the runtime.
 *
 * Everything is one-directional; there is no arbitrary jump. Rollback is a
 * *new* publication of a previous version, never a status mutation.
 */
export const LIFECYCLE: Record<WorkflowStatus, readonly WorkflowStatus[]> = {
  draft: ["validated"],
  validated: ["compiled"],
  compiled: ["published"],
  published: ["published", "active"],
  active: ["active"],
};

export function canTransit(from: WorkflowStatus, to: WorkflowStatus): boolean {
  return LIFECYCLE[from].includes(to);
}