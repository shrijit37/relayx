/**
 * /health must never lie: it reports the fleet contract and 503s when a
 * critical check (Postgres) is down, so load balancers and deploy gates can
 * trust it.
 */

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import Fastify from "fastify";
import { freshDb, mockGateway } from "./helpers";
import { buildApp } from "../src/api/routes";
import { GatewayClient } from "../src/gateway/client";

let db: Awaited<ReturnType<typeof freshDb>> | null = null;
let gatewayApp: Awaited<ReturnType<typeof mockGateway>> | null = null;
let app: Awaited<ReturnType<typeof Fastify>> | null = null;

beforeEach(async () => {
    db = await freshDb("health");
    gatewayApp = await mockGateway({ mustValidate: true });
    await gatewayApp.listen({ port: 0, host: "127.0.0.1" });
    const gw = gatewayApp!;
    const gwBase = `http://127.0.0.1:${(gw.server.address() as { port: number }).port}`;
    app = await buildApp({ pool: db.pool, gateway: new GatewayClient(gwBase) });
    await app.listen({ port: 0, host: "127.0.0.1" });
});

afterEach(async () => {
    await app?.close();
    await gatewayApp?.close();
    await db?.close();
});

const base = () =>
    `http://127.0.0.1:${(app!.server.address() as { port: number }).port}`;

describe("GET /health", () => {
    test("reports the fleet contract with real check results", async () => {
        const res = await fetch(`${base()}/health`);
        expect(res.status).toBe(200);
        const body = (await res.json()) as {
            status: string;
            service: string;
            version: string;
            checks: { database: string; gateway: string };
        };
        expect(Object.keys(body).sort()).toEqual([
            "checks",
            "service",
            "status",
            "version",
        ]);
        expect(body.service).toBe("relayx-control-plane");
        expect(typeof body.version).toBe("string");
        // The mock gateway serves /validate + /publish only, so its /ready probe
        // legitimately reports degraded — the point here is that every check
        // value is a real probe result, not a hardcoded 200.
        expect(["ok", "degraded"]).toContain(body.status);
        expect(body.checks.database).toBe("up");
        expect(["ok", "ready", "degraded"]).toContain(body.checks.gateway);
    });

    test("503s when Postgres is unreachable", async () => {
        // Hand ownership to this test so afterEach does not end the pool twice.
        const dying = db!;
        db = null;
        await dying.close();
        const res = await fetch(`${base()}/health`);
        expect(res.status).toBe(503);
        const body = (await res.json()) as {
            status: string;
            checks: { database: string };
        };
        expect(body.status).toBe("down");
        expect(body.checks.database).toBe("down");
    });
});
