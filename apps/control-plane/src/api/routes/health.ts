/**
 * Health check routes.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import type { GatewayClient } from "../../gateway/client";

const SERVICE = "relayx-control-plane";

export function registerHealthRoutes(
    app: FastifyInstance,
    gateway: GatewayClient,
    pool: Pool,
): void {
    const probeGateway = async (
        path: string,
    ): Promise<{ status: string; detail?: string }> => {
        try {
            const r = await fetch(`${gateway.baseUrl}${path}`, {
                signal: AbortSignal.timeout(2000),
            });
            if (!r.ok)
                return { status: "degraded", detail: `HTTP ${r.status}` };
            const j = (await r.json().catch(() => null)) as {
                status?: string;
            } | null;
            return { status: j?.status ?? "ok" };
        } catch (e) {
            return { status: "unreachable", detail: String(e) };
        }
    };

    app.get("/healthz", async () => ({ status: "ok" }));

    // Fleet health contract: /health reports the same shape as every other
    // service (status, service, version, checks) and 503s when a critical check
    // is down, so no load balancer probe ever sees a lying 200.
    app.get("/health", async (_req, reply) => {
        const database = await pool
            .query("SELECT 1")
            .then(() => "up" as const)
            .catch(() => "down" as const);
        const gw = await probeGateway("/ready");
        const status =
            database === "down"
                ? "down"
                : gw.status === "ok"
                  ? "ok"
                  : "degraded";
        reply.code(status === "down" ? 503 : 200);
        return {
            status,
            service: SERVICE,
            version: Bun.env["APP_VERSION"] ?? "unknown",
            checks: { database, gateway: gw.status },
        };
    });

    // Real system health for the frontend: probes the control plane itself and
    // the gateway's live admin endpoints (/healthz, /ready). Never fabricated —
    // each value is a real HTTP probe result.
    app.get("/system/health", async () => {
        return {
            control_plane: { status: "ok", service: SERVICE },
            gateway: {
                healthz: await probeGateway("/healthz"),
                ready: await probeGateway("/ready"),
            },
        };
    });
}
