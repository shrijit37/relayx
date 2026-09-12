/**
 * HTTP-level API tests: the Fastify app routes through repositories to a
 * real Postgres, publishing through a mock gateway (same harness as the
 * publish service tests). Asserts the REST contract, not just the service.
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
  db = await freshDb("api");
  gatewayApp = await mockGateway({ mustValidate: true });
  await gatewayApp.listen({ port: 0, host: "127.0.0.1" });
  const gw = gatewayApp!;
  const gwBase = `http://127.0.0.1:${(gw.server.address() as { port: number }).port}`;
  app = await buildApp({
    pool: db.pool,
    gateway: new GatewayClient(gwBase),
  });
  await app.listen({ port: 0, host: "127.0.0.1" });
});

afterEach(async () => {
  await app?.close();
  await gatewayApp?.close();
  await db?.close();
});

const base = () => {
  const p = app!.server.address() as { port: number };
  return `http://127.0.0.1:${p.port}`;
};

const helperWf = (lane: string) => ({
  id: "wf-http",
  name: "http llm",
  version: 1,
  nodes: [
    { id: "in", kind: "input", config: {}, inputs: [], outputs: [{ name: "out", port_type: "message" }] },
    {
      id: "llm",
      kind: "llm",
      config: { lane_id: lane, stream: true, model: "gpt-4" },
      inputs: [{ name: "in", port_type: "message" }],
      outputs: [{ name: "out", port_type: "message" }],
    },
    { id: "out", kind: "output", config: {}, inputs: [{ name: "in", port_type: "message" }], outputs: [] },
  ],
  edges: [
    { source_node: "in", source_port: "out", target_node: "llm", target_port: "in", condition: null },
    { source_node: "llm", source_port: "out", target_node: "out", target_port: "in", condition: null },
  ],
});

describe("REST API", () => {
  test("workflow CRUD + version creation", async () => {
    const res = await fetch(`${base()}/workflows`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: "echo", project_id: "proj_default" }),
    });
    expect(res.status).toBe(201);
    const wf = (await res.json()) as { id: string; name: string; status: string };
    expect(wf.name).toBe("echo");
    expect(wf.status).toBe("draft");

    // List.
    const listRes = await fetch(`${base()}/workflows`);
    const list = (await listRes.json()) as Array<{ id: string }>;
    expect(list.some((w) => w.id === wf.id)).toBe(true);

    // Version.
    const verRes = await fetch(`${base()}/workflows/${wf.id}/versions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a") }),
    });
    expect(verRes.status).toBe(201);
    const versions = (await (await fetch(`${base()}/workflows/${wf.id}/versions`)).json()) as Array<{ version: number }>;
    expect(versions.length).toBe(1);
    expect(versions[0]!.version).toBe(1);
  });

  test("provider CRUD", async () => {
    const res = await fetch(`${base()}/providers`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        name: "anthropic",
        protocol: "anthropic",
        base_url: "https://api.anthropic.com",
        model: "claude-sonnet-5",
      }),
    });
    expect(res.status).toBe(201);
    const created = (await res.json()) as { id: string; protocol: string };
    expect(created.protocol).toBe("anthropic");

    // Credentials never leak: no `credential_ref` on providers.
    const raw = (await (await fetch(`${base()}/providers`)).json()) as Array<Record<string, unknown>>;
    expect(raw[0]!["credential_ref"]).toBeUndefined();

    // List + delete.
    const listRes = await fetch(`${base()}/providers`);
    expect((await listRes.json()).length).toBe(1);
    const delRes = await fetch(`${base()}/providers/${created.id}`, { method: "DELETE" });
    expect(delRes.status).toBe(204);
  });

  test("lane CRUD with credential references (never raw secrets)", async () => {
    const res = await fetch(`${base()}/lanes`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        id: "lane-a",
        name: "primary",
        project_id: "proj_default",
        endpoint: "/v1/chat/completions",
        base_url: "http://127.0.0.1:9001",
        egress: "direct",
        policies: ["prod"],
        credential_ref: { ref: "RELAYX_ANTHROPIC_KEY", provider: "env" },
      }),
    });
    expect(res.status).toBe(201);
    const lane = (await res.json()) as { id: string; credential_ref: { ref: string } };
    expect(lane.id).toBe("lane-a");
    expect(lane.credential_ref.ref).toBe("RELAYX_ANTHROPIC_KEY");
    // The raw secret itself (e.g. "sk-...") never appears.
    expect(JSON.stringify(lane)).not.toContain("sk-");
  });

  test("validate + compile through the API yields a plan hash", async () => {
    // Seed a lane, workflow, version.
    await fetch(`${base()}/lanes`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        id: "lane-a",
        name: "primary",
        project_id: "proj_default",
        endpoint: "/chat",
        base_url: "http://127.0.0.1:9001",
        egress: "direct",
        policies: [],
      }),
    });
    const wfRes = await fetch(`${base()}/workflows`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: "echo", project_id: "proj_default" }),
    });
    const wf = (await wfRes.json()) as { id: string };
    await fetch(`${base()}/workflows/${wf.id}/versions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a") }),
    });

    // Validate (through the mock gateway).
    const valRes = await fetch(`${base()}/workflows/${wf.id}/validate`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 1 }),
    });
    expect(valRes.status).toBe(200);
    const val = (await valRes.json()) as { status: string; plan_hash: string };
    expect(val.status).toBe("compiled");
    expect(val.plan_hash).toContain("sha256:");

    // The version's status was persisted as compiled.
    const versions = (await (await fetch(`${base()}/workflows/${wf.id}/versions`)).json()) as Array<{ status: string; plan_hash: string | null }>;
    expect(versions[0]!.status).toBe("compiled");
    expect(versions[0]!.plan_hash).toBeTruthy();
  });

  test("publish through the API → ACTIVE + publication record", async () => {
    await fetch(`${base()}/lanes`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        id: "lane-a",
        name: "primary",
        project_id: "proj_default",
        endpoint: "/chat",
        base_url: "http://127.0.0.1:9001",
        egress: "direct",
        policies: [],
      }),
    });
    const wfRes = await fetch(`${base()}/workflows`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: "echo", project_id: "proj_default" }),
    });
    const wf = (await wfRes.json()) as { id: string };
    await fetch(`${base()}/workflows/${wf.id}/versions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a") }),
    });

    const pubRes = await fetch(`${base()}/workflows/${wf.id}/publish`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 1 }),
    });
    expect(pubRes.status).toBe(200);
    const pub = (await pubRes.json()) as { status: string; snapshot_version: number; workflow_version: number };
    expect(pub.status).toBe("published");
    expect(pub.snapshot_version).toBe(1);
    expect(pub.workflow_version).toBe(1);

    // workflow status ACTIVE.
    const wfAfter = (await (await fetch(`${base()}/workflows/${wf.id}`)).json()) as { status: string };
    expect(wfAfter.status).toBe("active");
  });
});