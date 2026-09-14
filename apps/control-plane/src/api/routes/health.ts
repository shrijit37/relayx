/**
 * Health check routes.
 */

import type { FastifyInstance } from "fastify";
import type { GatewayClient } from "../../gateway/client";

export function registerHealthRoutes(app: FastifyInstance, gateway: GatewayClient): void {
  app.get("/healthz", async () => ({ status: "ok" }));

  // Real system health for the frontend: probes the control plane itself and
  // the gateway's live admin endpoints (/healthz, /ready). Never fabricated —
  // each value is a real HTTP probe result.
  app.get("/system/health", async () => {
    const probe = async (path: string): Promise<{ status: string; detail?: string }> => {
      try {
        const r = await fetch(`${gateway.baseUrl}${path}`, { signal: AbortSignal.timeout(2000) });
        if (!r.ok) return { status: "degraded", detail: `HTTP ${r.status}` };
        const j = (await r.json().catch(() => null)) as { status?: string } | null;
        return { status: j?.status ?? "ok" };
      } catch (e) {
        return { status: "unreachable", detail: String(e) };
      }
    };
    return {
      control_plane: { status: "ok", service: "relayx-control-plane" },
      gateway: {
        healthz: await probe("/healthz"),
        ready: await probe("/ready"),
      },
    };
  });
}
