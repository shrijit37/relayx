/**
 * Gateway admin HTTP client.
 *
 * The control plane talks to the data plane on PUBLICATION cadence only —
 * never during request serving. The gateway remains authoritative for
 * compile + publish; the control plane persists the metadata around it.
 */

export type PlanIdentity = {
  workflow_id: string;
  plan_hash: string;
  version: number;
};

/** The gateway admin API response wire shape. */
export type GatewayResponse = {
  status: "published" | "validated" | "error";
  error?: string;
  snapshot_version?: number;
  workflows?: PlanIdentity[];
};

export type GatewayResult =
  | { ok: true; snapshot_version: number; workflows: PlanIdentity[] }
  | { ok: false; error: string };

export class GatewayClient {
  constructor(
    readonly baseUrl: string,
    private readonly fetchFn: typeof fetch = fetch,
  ) {}

  /** Compile-only dry-run; the active runtime is never touched. */
  async validate(payload: unknown): Promise<GatewayResult> {
    const data = await this.post("/validate", payload);
    return this.interpret(data);
  }

  /** Compile + atomically publish. */
  async publish(payload: unknown): Promise<GatewayResult> {
    const data = await this.post("/publish", payload);
    return this.interpret(data);
  }

  private async post(path: string, body: unknown): Promise<GatewayResponse | { status: "error"; error: string }> {
    let resp: Response;
    try {
      resp = await this.fetchFn(`${this.baseUrl}${path}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
    } catch (e) {
      return { status: "error", error: `gateway unreachable at ${this.baseUrl}: ${String(e)}` };
    }
    const data = (await resp.json().catch(() => null)) as GatewayResponse | null;
    if (!resp.ok || !data || data.status === "error") {
      return { status: "error", error: data?.error ?? `gateway HTTP ${resp.status}` };
    }
    return data;
  }

  private interpret(data: GatewayResponse | { status: "error"; error: string }): GatewayResult {
    // Only the two real success statuses are accepted; a future gateway
    // introducing "partial"/"pending" must NOT be treated as published
    // (review:angle-c). Anything else is a hard error.
    const okStatus = data.status === "published" || data.status === "validated";
    if (!okStatus || !Array.isArray(data.workflows)) {
      return {
        ok: false,
        error: data.status === "error" ? data.error ?? "unknown gateway error" : `unexpected gateway status '${(data as GatewayResponse).status ?? "unknown"}'`,
      };
    }
    return {
      ok: true,
      snapshot_version: data.snapshot_version ?? 0,
      workflows: data.workflows,
    };
  }
}