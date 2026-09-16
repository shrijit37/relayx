/**
 * Control-plane integration tests against the real Postgres (5433) + a mock
 * gateway. Proves the full publish pipeline, atomicity on failure, rollback,
 * and lifecycle state transitions.
 */

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { freshDb, mockGateway, publishDeps } from "./helpers";
import { createPublishService } from "../src/domain/publish";
import type { GatewayResult } from "../src/gateway/client";
import * as repo from "../src/db/repositories";

let db: Awaited<ReturnType<typeof freshDb>> | null = null;
let gatewayApp: Awaited<ReturnType<typeof mockGateway>> | null = null;

type Gateway = { validate(p: unknown): Promise<GatewayResult>; publish(p: unknown): Promise<GatewayResult> };

function makeGatewayClient(base: string): Gateway {
  return {
    validate: (p) => fetchJson(base, "/validate", p),
    publish: (p) => fetchJson(base, "/publish", p),
  };
}

async function fetchJson(base: string, path: string, body: unknown): Promise<GatewayResult> {
  const resp = await fetch(`${base}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = (await resp.json()) as {
    status?: string;
    error?: string;
    snapshot_version?: number;
    workflows?: Array<{ workflow_id: string; plan_hash: string; version: number }>;
  };
  if (!resp.ok || data.status === "error") {
    return { ok: false, error: data.error ?? `HTTP ${resp.status}` };
  }
  if (!data.workflows) return { ok: false, error: "no workflows in response" };
  return {
    ok: true,
    snapshot_version: data.snapshot_version ?? 0,
    workflows: data.workflows,
  };
}

const sampleWorkflow = (laneId: string, id = "wf-test") => ({
  id,
  name: "llm wf",
  version: 1,
  nodes: [
    { id: "in", kind: "input", config: {}, inputs: [], outputs: [{ name: "out", port_type: "message" }] },
    {
      id: "llm",
      kind: "llm",
      config: { lane_id: laneId, stream: true, model: "gpt-4" },
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

beforeEach(async () => {
  db = await freshDb("cp");
  gatewayApp = await mockGateway({ mustValidate: true });
  await gatewayApp.listen({ port: 0, host: "127.0.0.1" });
});

afterEach(async () => {
  await gatewayApp?.close();
  await db?.close();
});

describe("publish pipeline", () => {
  test("validate → compile → publish → active", async () => {
    const gw = gatewayApp!;
    const base = `http://127.0.0.1:${(gw.server.address() as { port: number }).port}`;
    const gateway = makeGatewayClient(base);
    const service = createPublishService(publishDeps(db!.pool, gateway));

    // Seed a project + lane.
    await db!.pool.query("INSERT INTO projects (id,name) VALUES ('p1','p1')");
    await repo.lanes.create(db!.pool, {
      id: "lane-a",
      project_id: "p1",
      provider_id: null,
      endpoint: "/v1/chat/completions",
      base_url: "http://127.0.0.1:9001",
      egress: "direct",
      proxy_url: null,
      policies: [],
      credential_ref: null,
    });

    // Create workflow + version.
    const wf = await repo.workflows.create(db!.pool, "p1", "echo");
    await repo.workflows.createVersion(db!.pool, wf.id, 1, sampleWorkflow("lane-a"));

    const result = await service.publish({
      workflowId: wf.id,
      version: 1,
      workflowJson: sampleWorkflow("lane-a"),
    });

    expect(result.status).toBe("published");
    // snapshot_version is the global monotonic counter (not workflow version).
    expect(typeof result.snapshot_version).toBe("number");
    expect(result.snapshot_version).toBeGreaterThanOrEqual(1);

    // DB now ACTIVE + publication record + plan hash.
    const versions = await repo.workflows.listVersions(db!.pool, wf.id);
    expect(versions[0]!.status).toBe("active");
    expect(versions[0]!.plan_hash).toBeTruthy();

    const active = await repo.workflows.getActiveVersion(db!.pool, wf.id);
    expect(active).not.toBeNull();
    expect(active!.workflow_version).toBe(1);

    const pubs = await repo.publications.listByWorkflow(db!.pool, wf.id);
    expect(pubs.length).toBe(1);
    expect(pubs[0]!.status).toBe("succeeded");
  });

  test("failed publish leaves previous runtime + version intact", async () => {
    const gw = gatewayApp!;
    const base = `http://127.0.0.1:${(gw.server.address() as { port: number }).port}`;
    const gateway = makeGatewayClient(base);
    const service = createPublishService(publishDeps(db!.pool, gateway));

    await db!.pool.query("INSERT INTO projects (id,name) VALUES ('p1','p1')");
    await repo.lanes.create(db!.pool, {
      id: "lane-a",
      project_id: "p1",
      provider_id: null,
      endpoint: "/chat",
      base_url: "http://127.0.0.1:9001",
      egress: "direct",
      proxy_url: null,
      policies: [],
      credential_ref: null,
    });
    const wf = await repo.workflows.create(db!.pool, "p1", "echo");
    // v1 published OK.
    await repo.workflows.createVersion(db!.pool, wf.id, 1, sampleWorkflow("lane-a"));
    const ok = await service.publish({ workflowId: wf.id, version: 1, workflowJson: sampleWorkflow("lane-a") });
    expect(ok.status).toBe("published");

    // v2 fails (unknown lane).
    await repo.workflows.createVersion(db!.pool, wf.id, 2, sampleWorkflow("missing-lane"));
    const bad = await service.publish({ workflowId: wf.id, version: 2, workflowJson: sampleWorkflow("missing-lane") });
    expect(bad.status).toBe("error");

    // Active runtime still v1; v2 remains draft; a failed publication record exists.
    const active = await repo.workflows.getActiveVersion(db!.pool, wf.id);
    expect(active!.workflow_version).toBe(1);
    const v2 = (await repo.workflows.listVersions(db!.pool, wf.id)).find((v) => v.version === 2)!;
    expect(v2.status).toBe("draft");
    const pubs = await repo.publications.listByWorkflow(db!.pool, wf.id);
    expect(pubs.some((p) => p.status === "failed")).toBe(true);
  });

  test("rollback republishes the previous valid version", async () => {
    const gw = gatewayApp!;
    const base = `http://127.0.0.1:${(gw.server.address() as { port: number }).port}`;
    const gateway = makeGatewayClient(base);
    const service = createPublishService(publishDeps(db!.pool, gateway));

    await db!.pool.query("INSERT INTO projects (id,name) VALUES ('p1','p1')");
    await repo.lanes.create(db!.pool, {
      id: "lane-a", project_id: "p1", provider_id: null, endpoint: "/chat",
      base_url: "http://127.0.0.1:9001", egress: "direct", proxy_url: null, policies: [], credential_ref: null,
    });
    const wf = await repo.workflows.create(db!.pool, "p1", "echo");
    await repo.workflows.createVersion(db!.pool, wf.id, 1, sampleWorkflow("lane-a"));
    await repo.workflows.createVersion(db!.pool, wf.id, 2, { ...sampleWorkflow("lane-a"), name: "v2" });

    await service.publish({ workflowId: wf.id, version: 1, workflowJson: sampleWorkflow("lane-a") });
    await service.publish({ workflowId: wf.id, version: 2, workflowJson: { ...sampleWorkflow("lane-a"), name: "v2" } });

    const active = await repo.workflows.getActiveVersion(db!.pool, wf.id);
    expect(active!.workflow_version).toBe(2);

    // Rollback → republish v1 (previous valid).
    const service2 = createPublishService(publishDeps(db!.pool, gateway));
    // Simulate the route handler rollback flow.
    const rollback = () =>
      service2.publish({
        workflowId: wf.id,
        version: 1,
        workflowJson: sampleWorkflow("lane-a"),
      });
    const result = await rollback();
    expect(result.status).toBe("published");

    const activeAfter = await repo.workflows.getActiveVersion(db!.pool, wf.id);
    expect(activeAfter!.workflow_version).toBe(1);
  });
});

describe("workflow lifecycle", () => {
  test("version status transitions draft → compiled → active", async () => {
    const gw = gatewayApp!;
    const base = `http://127.0.0.1:${(gw.server.address() as { port: number }).port}`;
    const gateway = makeGatewayClient(base);
    const service = createPublishService(publishDeps(db!.pool, gateway));

    await db!.pool.query("INSERT INTO projects (id,name) VALUES ('p1','p1')");
    await repo.lanes.create(db!.pool, {
      id: "lane-a", project_id: "p1", provider_id: null, endpoint: "/chat",
      base_url: "http://127.0.0.1:9001", egress: "direct", proxy_url: null, policies: [], credential_ref: null,
    });
    const wf = await repo.workflows.create(db!.pool, "p1", "echo");

    // v1 created → draft.
    await repo.workflows.createVersion(db!.pool, wf.id, 1, sampleWorkflow("lane-a"));
    let versions = await repo.workflows.listVersions(db!.pool, wf.id);
    expect(versions[0]!.status).toBe("draft");

    // Publish → compiled → active (recordCompiled sets compiled, recordPublished sets active).
    await service.publish({ workflowId: wf.id, version: 1, workflowJson: sampleWorkflow("lane-a") });
    versions = await repo.workflows.listVersions(db!.pool, wf.id);
    expect(versions[0]!.status).toBe("active");
  });
});

describe("extension collection", () => {
  test("collectCustomExtensionKinds extracts distinct kinds from workflow JSON", async () => {
    const { collectCustomExtensionKinds } = await import("../src/domain/publish");
    const wf = {
      nodes: [
        { id: "in", kind: "input", config: { kind: "input" } },
        { id: "c1", kind: "custom", config: { kind: "custom", ext_kind: "nordvpn-egress", payload: {} } },
        { id: "c2", kind: "custom", config: { kind: "custom", ext_kind: "nordvpn-egress", payload: { x: 1 } } },
        { id: "c3", kind: "custom", config: { kind: "custom", ext_kind: "key-pool", payload: {} } },
        { id: "out", kind: "output", config: { kind: "output" } },
      ],
    };
    const kinds = collectCustomExtensionKinds(wf);
    expect(kinds.size).toBe(2);
    expect(kinds.has("nordvpn-egress")).toBe(true);
    expect(kinds.has("key-pool")).toBe(true);
  });

  test("collectCustomExtensionKinds returns empty set for workflow with no custom nodes", async () => {
    const { collectCustomExtensionKinds } = await import("../src/domain/publish");
    const wf = {
      nodes: [
        { id: "in", kind: "input", config: { kind: "input" } },
        { id: "out", kind: "output", config: { kind: "output" } },
      ],
    };
    const kinds = collectCustomExtensionKinds(wf);
    expect(kinds.size).toBe(0);
  });

  test("buildCoherentWireNoVersion includes extensions in wire snapshot", async () => {
    const { buildCoherentWireNoVersion } = await import("../src/domain/publish");
    const flows = [
      {
        id: "wf-ext",
        version: 1,
        workflowJson: {
          nodes: [
            { id: "in", kind: "input", config: { kind: "input" }, inputs: [], outputs: [{ name: "out", port_type: "message" }] },
            { id: "c1", kind: "custom", config: { kind: "custom", ext_kind: "my-ext", payload: {} }, inputs: [{ name: "in", port_type: "message" }], outputs: [{ name: "out", port_type: "message" }] },
            { id: "out", kind: "output", config: { kind: "output" }, inputs: [{ name: "in", port_type: "message" }], outputs: [] },
          ],
          edges: [
            { source_node: "in", source_port: "out", target_node: "c1", target_port: "in" },
            { source_node: "c1", source_port: "out", target_node: "out", target_port: "in" },
          ],
        },
        revision: "target" as const,
      },
    ];
    const result = await buildCoherentWireNoVersion(flows, async () => null);
    if ("error" in result) throw new Error(result.error);
    expect(result.extensions).toBeDefined();
    expect(result.extensions!.length).toBe(1);
    expect(result.extensions![0]!.kind).toBe("my-ext");
    expect(result.extensions![0]!.version).toBe(0);
  });

  test("toWireLane rejects masked lane without proxy_url (no silent direct wire lane)", async () => {
    const { buildCoherentWireNoVersion } = await import("../src/domain/publish");
    const flows = [
      {
        id: "wf-masked",
        version: 1,
        workflowJson: {
          nodes: [
            { id: "in", kind: "input", config: { kind: "input" } },
            { id: "llm", kind: "llm", config: { lane_id: "lane-masked", model: "gpt-4" } },
            { id: "out", kind: "output", config: { kind: "output" } },
          ],
          edges: [
            { source_node: "in", source_port: "out", target_node: "llm", target_port: "in" },
            { source_node: "llm", source_port: "out", target_node: "out", target_port: "in" },
          ],
        },
        revision: "target" as const,
      },
    ];
    // A lane row with egress=masked but no proxy_url must fail bundle
    // building with a clear error — the gateway's compile-time rejection is
    // the backstop, but the control plane must never emit a masked wired
    // lane without a tunnel.
    const result = await buildCoherentWireNoVersion(
      flows,
      async () => ({
        id: "lane-masked",
        project_id: "p1",
        provider_id: null,
        endpoint: "/chat",
        base_url: "http://127.0.0.1:9100",
        egress: "masked",
        proxy_url: null,
        policies: [],
        credential_ref: null,
      }),
    );
    expect("error" in result).toBe(true);
    if ("error" in result) {
      expect(result.error).toContain("masked");
      expect(result.error).toContain("proxy_url");
    }
  });
});