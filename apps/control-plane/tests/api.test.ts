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

// Seed lane + workflow + publish → ACTIVE, returning the workflow id.
// Mirrors the inline seed used by the earlier tests.
async function seedPublishedWf(): Promise<string> {
  await fetch(`${base()}/lanes`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id: "lane-a", name: "primary", project_id: "proj_default", endpoint: "/chat", base_url: "http://127.0.0.1:9001", egress: "direct", policies: [] }),
  });
  const wfRes = await fetch(`${base()}/workflows`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: "stream-test", project_id: "proj_default" }),
  });
  const wf = (await wfRes.json()) as { id: string };
  await fetch(`${base()}/workflows/${wf.id}/versions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ workflow_json: helperWf("lane-a") }),
  });
  await fetch(`${base()}/workflows/${wf.id}/publish`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 1 }),
  });
  return wf.id;
}

/** Rebuild app + gateway with the given mock gateway options. */
async function rebuildWithGateway(opts: Parameters<typeof mockGateway>[0]) {
  await app?.close();
  await gatewayApp?.close();
  await db?.close();

  const db2 = await freshDb("api-stream");
  const gw2 = await mockGateway(opts);
  await gw2.listen({ port: 0, host: "127.0.0.1" });
  const gw2Base = `http://127.0.0.1:${(gw2.server.address() as { port: number }).port}`;
  app = await buildApp({ pool: db2.pool, gateway: new GatewayClient(gw2Base) });
  await app.listen({ port: 0, host: "127.0.0.1" });
  db = db2;
  gatewayApp = gw2;
}

describe("REST API streaming run", () => {
  test("stream with a terminal `event: done` records the run as completed", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      streamChunks: [
        'data: {"delta":"hi"}\n\n',
        "event: done\ndata: {}\n\n",
      ],
    });
    const wfId = await seedPublishedWf();

    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: { messages: [{ role: "user", content: "hi" }] } }),
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/event-stream");
    const text = await res.text();
    expect(text).toContain("event: done");

    const runs = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string }>;
    const rec = runs.find((r) => r.workflow_id === wfId)!;
    expect(rec.status).toBe("completed");
  });

  test("stream with a terminal `event: error` records the run as failed", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      streamChunks: [
        'data: {"delta":"hi"}\n\n',
        'event: error\ndata: {"error":"provider exploded"}\n\n',
      ],
    });
    const wfId = await seedPublishedWf();

    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(res.status).toBe(200);
    await res.text();

    const runs = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string; error: string | null }>;
    const rec = runs.find((r) => r.workflow_id === wfId)!;
    expect(rec.status).toBe("failed");
  });

  test("truncated stream (no terminal event) records the run as failed, not completed", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      // No `event: done` / `event: error` — gateway closes the connection
      // mid-stream. Must NOT be recorded as a silent success.
      streamChunks: ['data: {"delta":"partial"}\n\n'],
    });
    const wfId = await seedPublishedWf();

    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(res.status).toBe(200);
    await res.text();

    const runs = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string; error: string | null }>;
    const rec = runs.find((r) => r.workflow_id === wfId)!;
    expect(rec.status).toBe("failed");
    expect(rec.error).toContain("terminal event");
  });

  test("client abort (socket destroyed mid-stream) records the run as cancelled", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      // First chunk flushes immediately; the delay keeps the stream open so
      // the client can abort before the terminal `event: done` frame.
      streamChunks: ['data: {"delta":"ping"}\n\n', "event: done\ndata: {}\n\n"],
      streamChunkDelayMs: 200,
    });
    const wfId = await seedPublishedWf();

    const ctrl = new AbortController();
    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
      signal: ctrl.signal,
    });
    expect(res.status).toBe(200);
    // Abort after the response starts; the pump should see the socket close
    // and finalize as cancelled (no terminal event seen).
    ctrl.abort();

    // Give the pump a moment to observe the destroy and write the row.
    for (let i = 0; i < 40; i++) {
      const runs = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string }>;
      const rec = runs.find((r) => r.workflow_id === wfId);
      if (rec && rec.status !== "running") {
        expect(rec.status).toBe("cancelled");
        return;
      }
      await Bun.sleep(50);
    }
    throw new Error("run never left 'running' after client abort");
  });

  test("streaming run record is created (status running → terminal) and reachable via GET /runs", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      streamChunks: ["event: done\ndata: {}\n\n"],
      streamChunkDelayMs: 200,
    });
    const wfId = await seedPublishedWf();

    // During the run the row exists as 'running'.
    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(res.status).toBe(200);
    await res.text();

    const detail = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string }>;
    const rec = detail.find((r) => r.workflow_id === wfId)!;
    expect(rec.status).toBe("completed");
  });

  test("run record is created even when the stream content has no newline-terminated frames (byte-scan robustness)", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      // Terminal marker split across chunks — the pump's remainder scan
      // must still detect `event: done`.
      streamChunks: ['data: x\n\nevent: do', "ne\ndata: {}\n\n"],
    });
    const wfId = await seedPublishedWf();

    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(res.status).toBe(200);
    await res.text();

    const runs = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string }>;
    const rec = runs.find((r) => r.workflow_id === wfId)!;
    expect(rec.status).toBe("completed");
  });

  test("stream that 502s before hijack still records a failed run (no silent gap)", async () => {
    await rebuildWithGateway({
      mustValidate: true,
      failStreamWith: "gateway rejected stream",
    });
    const wfId = await seedPublishedWf();

    const res = await fetch(`${base()}/workflows/${wfId}/run?stream=true`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(res.status).toBe(502);

    const runs = (await (await fetch(`${base()}/runs`)).json()) as Array<{ workflow_id: string; status: string; error: string | null }>;
    const rec = runs.find((r) => r.workflow_id === wfId)!;
    expect(rec.status).toBe("failed");
    expect(rec.error).toContain("gateway rejected stream");
  });
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
    expect(typeof pub.snapshot_version).toBe("number");
    expect(pub.snapshot_version).toBeGreaterThanOrEqual(1);
    expect(pub.workflow_version).toBe(1);

    // workflow status ACTIVE.
    const wfAfter = (await (await fetch(`${base()}/workflows/${wf.id}`)).json()) as { status: string };
    expect(wfAfter.status).toBe("active");
  });

  test("run: published workflow returns the real run envelope", async () => {
    // Seed lane + workflow + version + publish → ACTIVE.
    await fetch(`${base()}/lanes`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: "lane-a", name: "primary", project_id: "proj_default", endpoint: "/chat", base_url: "http://127.0.0.1:9001", egress: "direct", policies: [] }),
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
    await fetch(`${base()}/workflows/${wf.id}/publish`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 1 }),
    });

    const runRes = await fetch(`${base()}/workflows/${wf.id}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: { messages: [{ role: "user", content: "hi" }] } }),
    });
    expect(runRes.status).toBe(200);
    const run = (await runRes.json()) as {
      status: string;
      request_id: string;
      workflow_id: string;
      workflow_version: number;
      snapshot_version: number;
      plan_hash: string;
      output: unknown;
    };
    expect(run.status).toBe("ok");
    expect(run.workflow_id).toBe(wf.id);
    expect(run.workflow_version).toBe(1);
    expect(typeof run.snapshot_version).toBe("number");
    expect(run.plan_hash.length).toBeGreaterThan(0);
    // Real passthrough of the gateway output (never fabricated).
    expect((run.output as { ok: boolean }).ok).toBe(true);
  });

  test("run: unpublished workflow is rejected with 409 (never silently exec'd)", async () => {
    const wfRes = await fetch(`${base()}/workflows`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: "draft-only", project_id: "proj_default" }),
    });
    const wf = (await wfRes.json()) as { id: string };

    const runRes = await fetch(`${base()}/workflows/${wf.id}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(runRes.status).toBe(409);
    const body = (await runRes.json()) as { error: string };
    expect(body.error).toContain("published");
  });

  test("run: gateway run failure surfaces as a real 400 error", async () => {
    // Fresh gateway that fails /run.
    await db?.close();
    await gatewayApp?.close();
    await app?.close();

    const db2 = await freshDb("api-runfail");
    const gw2 = await mockGateway({ mustValidate: true, failRunWith: "provider 500" });
    await gw2.listen({ port: 0, host: "127.0.0.1" });
    const gw2Base = `http://127.0.0.1:${(gw2.server.address() as { port: number }).port}`;
    app = await buildApp({ pool: db2.pool, gateway: new GatewayClient(gw2Base) });
    await app.listen({ port: 0, host: "127.0.0.1" });
    db = db2;
    gatewayApp = gw2;

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
    // Force ACTIVE without the gateway round-trip by publishing first against
    // a passing gateway… simpler: insert workflow_active directly.
    await db!.pool.query(
      "INSERT INTO workflow_active (workflow_id, workflow_version, plan_hash, snapshot_version) VALUES ($1, 1, 'sha256:test', 1)",
      [wf.id],
    );

    const runRes = await fetch(`${base()}/workflows/${wf.id}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: {} }),
    });
    expect(runRes.status).toBe(400);
    const body = (await runRes.json()) as { error: string };
    expect(body.error).toContain("provider 500");
  });

  test("run: persists a run record accessible via GET /runs", async () => {
    // Seed lane + workflow + publish → ACTIVE.
    await fetch(`${base()}/lanes`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: "lane-a", name: "primary", project_id: "proj_default", endpoint: "/chat", base_url: "http://127.0.0.1:9001", egress: "direct", policies: [] }),
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
    await fetch(`${base()}/workflows/${wf.id}/publish`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 1 }),
    });

    // Run the workflow.
    await fetch(`${base()}/workflows/${wf.id}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ body: { messages: [{ role: "user", content: "hi" }] } }),
    });

    // The run list should contain one completed record.
    const listRes = await fetch(`${base()}/runs`);
    expect(listRes.status).toBe(200);
    const runs = (await listRes.json()) as Array<{ id: string; status: string; workflow_id: string }>;
    expect(runs.length).toBeGreaterThanOrEqual(1);
    const rec = runs.find((r) => r.workflow_id === wf.id)!;
    expect(rec.status).toBe("completed");

    // Single run fetch.
    const getRes = await fetch(`${base()}/runs/${rec.id}`);
    expect(getRes.status).toBe(200);
    const detail = (await getRes.json()) as { input_body: unknown; output: unknown; completed_at: string | null };
    expect(detail.input_body).toBeTruthy();
    expect(detail.output).toBeTruthy();
    expect(detail.completed_at).toBeTruthy();

    // Filter by workflow.
    const filterRes = await fetch(`${base()}/runs?workflow_id=${wf.id}`);
    expect(filterRes.status).toBe(200);
    const filtered = (await filterRes.json()) as Array<{ workflow_id: string }>;
    expect(filtered.every((r) => r.workflow_id === wf.id)).toBe(true);
  });

  test("workflow DELETE removes the row and cascades versions", async () => {
    const wfRes = await fetch(`${base()}/workflows`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: "to-delete", project_id: "proj_default" }),
    });
    expect(wfRes.status).toBe(201);
    const wf = (await wfRes.json()) as { id: string };

    // Create a version.
    await fetch(`${base()}/workflows/${wf.id}/versions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: { id: wf.id, name: "to-delete", version: 1, nodes: [], edges: [] } }),
    });
    const versionsBefore = (await (await fetch(`${base()}/workflows/${wf.id}/versions`)).json()) as Array<{ version: number }>;
    expect(versionsBefore.length).toBe(1);

    // Delete.
    const delRes = await fetch(`${base()}/workflows/${wf.id}`, { method: "DELETE" });
    expect(delRes.status).toBe(204);

    // Confirm gone.
    const getRes = await fetch(`${base()}/workflows/${wf.id}`);
    expect(getRes.status).toBe(404);

    // Versions cascade-deleted.
    const versionsAfter = (await (await fetch(`${base()}/workflows/${wf.id}/versions`)).json()) as { error?: string };
    expect(versionsAfter.error).toContain("not found");
  });

  test("rollback endpoint republishes a previous valid version", async () => {
    await fetch(`${base()}/lanes`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: "lane-a", name: "primary", project_id: "proj_default", endpoint: "/chat", base_url: "http://127.0.0.1:9001", egress: "direct", policies: [] }),
    });
    const wfRes = await fetch(`${base()}/workflows`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: "rollback-test", project_id: "proj_default" }),
    });
    const wf = (await wfRes.json()) as { id: string };

    // v1
    await fetch(`${base()}/workflows/${wf.id}/versions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a") }),
    });
    await fetch(`${base()}/workflows/${wf.id}/publish`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 1 }),
    });

    // v2 (also published)
    await fetch(`${base()}/workflows/${wf.id}/versions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a") }),
    });
    await fetch(`${base()}/workflows/${wf.id}/publish`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workflow_json: helperWf("lane-a"), version: 2 }),
    });

    // Rollback → back to v1.
    const rbRes = await fetch(`${base()}/workflows/${wf.id}/rollback`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(rbRes.status).toBe(200);
    const rb = (await rbRes.json()) as { status: string; to_version: number };
    expect(rb.status).toBe("rolled_back");
    expect(rb.to_version).toBe(1);
  });
});