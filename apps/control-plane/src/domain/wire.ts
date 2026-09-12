/**
 * Wire types exchanged with the gateway admin API (`/validate`, `/publish`).
 * Mirrors `WireSnapshot`/`WireWorkflow`/`WireLane` in
 * `apps/gateway/src/observability/mod.rs`. The gateway is the only compiler —
 * these are the compile+publish contract.
 */

export type WireLane = {
  base_url: string;
  /** Resolved Authorization header value; never a credential reference. */
  authorization?: string | null;
};

export type WireWorkflow = {
  id: string;
  workflow: Record<string, unknown>;
  lanes: Record<string, WireLane>;
};

export type WireSnapshot = {
  snapshot_version: number;
  workflows: WireWorkflow[];
};