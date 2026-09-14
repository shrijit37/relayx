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
  status: "published" | "validated" | "ok" | "error";
  error?: string;
  snapshot_version?: number;
  workflows?: PlanIdentity[];
};

export type GatewayResult =
  | { ok: true; snapshot_version: number; workflows: PlanIdentity[] }
  | { ok: false; error: string };

/** Raw `/run` success wire shape from the gateway. */
export type RunResponse = {
  status: "ok";
  request_id: string;
  workflow_id: string;
  snapshot_version: number;
  plan_hash: string;
  output: unknown;
};

export type RunResult =
  | {
      ok: true;
      request_id: string;
      workflow_id: string;
      snapshot_version: number;
      plan_hash: string;
      output: unknown;
    }
  | { ok: false; error: string };

export class GatewayClient {
  constructor(
    readonly baseUrl: string,
    private readonly apiKey?: string,
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

  /**
   * Execute a workflow from the current published snapshot (the frontend's
   * Run contract). The gateway returns the real execution envelope —
   * request_id / snapshot_version / plan_hash / output — or a typed error
   * (404 for an unpublished workflow, provider/validation errors mapped
   * through). The success shape differs from validate/publish (`output`
   * instead of `workflows`), so it is interpreted separately.
   */
  async run(payload: unknown): Promise<RunResult> {
    const data = await this.post("/run", payload);
    if (data.status === "error") {
      return { ok: false, error: data.error ?? "gateway run failed" };
    }
    if (data.status === "ok" && "output" in data) {
      return {
        ok: true,
        request_id: (data as RunResponse).request_id,
        snapshot_version: (data as RunResponse).snapshot_version ?? 0,
        workflow_id: (data as RunResponse).workflow_id ?? "",
        plan_hash: (data as RunResponse).plan_hash ?? "",
        output: (data as RunResponse).output,
      };
    }
    return { ok: false, error: `unexpected gateway run status '${(data as GatewayResponse).status ?? "unknown"}'` };
  }

  private async post(path: string, body: unknown): Promise<GatewayResponse | { status: "error"; error: string }> {
    let resp: Response;
    try {
      const headers: Record<string, string> = { "content-type": "application/json" };
      // Mutating admin endpoints are protected by a shared-secret API key;
      // when configured, every request carries it as a Bearer token.
      if (this.apiKey) {
        headers["authorization"] = `Bearer ${this.apiKey}`;
      }
      resp = await this.fetchFn(`${this.baseUrl}${path}`, {
        method: "POST",
        headers,
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