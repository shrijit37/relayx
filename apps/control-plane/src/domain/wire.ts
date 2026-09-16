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
  /** Egress mode: "direct" (default, gateway IP) or "masked" (via proxy_url). */
  egress: string;
  /** Proxy URL for masked egress (http://… for HTTP CONNECT, socks5://… for SOCKS5). */
  proxy_url?: string | null;
};

export type WireWorkflow = {
  id: string;
  workflow: Record<string, unknown>;
  lanes: Record<string, WireLane>;
  /** The ACTIVE version this workflow was compiled from (run identity). */
  version: number;
};

export type WireExtension = {
  kind: string;
  version: number;
};

export type WireSnapshot = {
  snapshot_version: number;
  workflows: WireWorkflow[];
  extensions?: WireExtension[];
};