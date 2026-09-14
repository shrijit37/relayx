/**
 * Phase 6.6 serializer tests.
 *
 * Old behavior under test changed ON PURPOSE (lossy semantics removed):
 * - title no longer determines model/provider/lane (no inference)
 * - lane nodes are no longer "folded in" by edge hunting — lane identity is
 *   explicit `config.lane` (a lane NODE on canvas is display-only and blocks
 *   publish rather than vanishing)
 * - display-only node kinds are serialized as explicit `unsupported` nodes
 *   (never silently dropped)
 * - no fabricated `"unknown"`/`"default"` config values
 *
 * These tests cover the canonical-path guarantees. The runtime-facing shape
 * (provider/model/lane_id/condition field+operator+value, explicit ports and
 * positions) is what the control plane and Rust schema consume.
 * Positions migrate deterministically for legacy v1 JSON (no stored
 * coordinates): stable column layout, never title/topology parsing.
 */

import { describe, expect, test } from "bun:test";
import type { Edge } from "@xyflow/react";
import type { RelayNode } from "@/components/relay/workflow/nodes";
import {
  deserializeWorkflow,
  serializeWorkflow,
} from "@/lib/workflow";

function node(id: string, kind: RelayNode["data"]["kind"], title = id): RelayNode {
  return {
    id,
    type: "relay",
    position: { x: 0, y: 0 },
    data: { kind, title, lines: [] },
  };
}

function edge(id: string, source: string, target: string, sourceHandle?: string): Edge {
  return { id, source, target, ...(sourceHandle ? { sourceHandle } : {}) };
}

describe("phase 6.6 canonical serializer", () => {
  test("input → output round-trips without title inference", () => {
    const nodes: RelayNode[] = [node("in", "input", "My Custom Input Label"), node("out", "output", "My Output")];
    const edges: Edge[] = [edge("e1", "in", "out")];
    const result = serializeWorkflow(nodes, edges, { id: "wf", name: "W", version: 1 });
    expect(result.errors).toEqual([]);
    const wf = result.workflow!;
    expect(wf.schema_version).toBe(2);
    const inNode = wf.nodes.find((n) => n.id === "in")!;
    expect(inNode.config).toEqual({ kind: "input" });
    // Title is presentation-only, preserved in `presentation`, never parsed.
    expect(inNode.presentation?.title).toBe("My Custom Input Label");
  });

  test("title changes do not change provider/model/lane semantics", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      {
        ...node("provider", "provider", "OpenAI · gpt-4o"),
        data: {
          kind: "provider",
          title: "OpenAI · gpt-4o",
          lines: [],
          canonicalConfig: {
            kind: "llm",
            config: { provider: "openai", model: "gpt-4o", lane: "openai-direct", stream: true },
          },
        } as RelayNode["data"],
      },
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "provider"), edge("e2", "provider", "out")];
    const renamed = nodes.map((n) =>
      n.id === "provider" ? { ...n, data: { ...n.data, title: "Totally different title" } } : n,
    );

    const wf1 = serializeWorkflow(nodes, edges).workflow!;
    const wf2 = serializeWorkflow(renamed, edges).workflow!;
    const b = wf1.nodes.find((n) => n.id === "provider")!;
    const a = wf2.nodes.find((n) => n.id === "provider")!;
    // Renaming a title changes presentation only — never config.
    expect(a.config).toEqual(b.config);
    expect(a.presentation?.title).toBe("Totally different title");
  });

  test("no fabricated configuration: no unknown/default model", () => {
    const nodes: RelayNode[] = [node("in", "input"), node("provider", "provider"), node("out", "output")];
    const edges: Edge[] = [edge("e1", "in", "provider"), edge("e2", "provider", "out")];
    const result = serializeWorkflow(nodes, edges);
    expect(result.workflow).toBeNull();
    // The bare provider blocks on ALL missing required fields — lane, model,
    // AND provider — never a fabricated default value.
    expect(result.errors.join()).toContain("lane");
    expect(result.errors.join()).toContain("Model is required");
    expect(result.errors.join()).toContain("Provider is required");
    expect(result.errors.join()).not.toContain("default");
  });

  test("display-only nodes are explicit unsupported — never silently dropped", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("policy", "policy", "production-standard"),
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "policy"), edge("e2", "policy", "out")];
    const result = serializeWorkflow(nodes, edges);
    expect(result.workflow).toBeNull();
    expect(result.errors.some((e) => e.includes("policy"))).toBe(true);
  });

  test("lane node is not folded; lane identity is explicit config.lane", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("provider", "provider", "Model"),
      node("lane", "lane", "anthropic-us"),
      node("out", "output"),
    ];
    const edges: Edge[] = [
      edge("e1", "in", "provider"),
      edge("e2", "lane", "provider"),
      edge("e3", "provider", "out"),
    ];
    // The lane node is a canvas-only reminder, not semantic. Serialization
    // refuses (lane is not executable) instead of folding by edge hunting —
    // the lane_id comes from the provider config, never its title or edges.
    const result = serializeWorkflow(nodes, edges);
    expect(result.workflow).toBeNull();
    expect(result.errors.some((e) => e.includes("lane"))).toBe(true);
  });

  test("serializes fully-configured provider with explicit provider/model/lane", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      {
        ...node("p", "provider", "Anything"),
        data: {
          kind: "provider",
          title: "Anything",
          lines: [],
          canonicalConfig: {
            kind: "llm",
            config: { provider: "anthropic", model: "claude-sonnet", lane: "anthropic-primary", temperature: 0.2, stream: true },
          },
        } as RelayNode["data"],
      },
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "p"), edge("e2", "p", "out")];
    const result = serializeWorkflow(nodes, edges, { id: "wf2", name: "W2", version: 2 });
    expect(result.errors).toEqual([]);
    const p = result.workflow!.nodes.find((n) => n.id === "p")!;
    expect(p.config).toEqual({
      kind: "llm",
      provider: "anthropic",
      model: "claude-sonnet",
      lane_id: "anthropic-primary",
      temperature: 0.2,
      stream: true,
    });
  });

  test("condition node with explicit config round-trips through persisted JSON", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      {
        ...node("cond", "condition"),
        data: {
          kind: "condition",
          title: "Refund?",
          lines: [],
          canonicalConfig: {
            kind: "condition",
            condition: { field: "user.intent", operator: "equals", value: "refund", valueType: "string" },
          },
        } as RelayNode["data"],
      },
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "cond"), edge("e2", "cond", "out", "true")];
    const result = serializeWorkflow(nodes, edges);
    expect(result.errors).toEqual([]);
    const cond = result.workflow!.nodes.find((n) => n.id === "cond")!;
    expect(cond.config).toEqual({
      kind: "condition",
      // Rust ConditionConfig shape: `condition` string + snake_case operator.
      condition: "user.intent equals refund",
      field: "user.intent",
      operator: "equal",
      value: "refund",
    });
  });

  test("legacy v1 workflow (no positions) migrates deterministically", () => {
    const legacy = {
      id: "legacy", name: "Legacy", version: 1,
      nodes: [
        { id: "input", kind: "input", config: { kind: "input" }, inputs: [], outputs: [] },
        { id: "retry", kind: "retry", config: { kind: "retry", max_attempts: 3, delay_ms: 500, on_timeout: true, on_provider_error: true, target: { kind: "llm", stream: false } }, inputs: [], outputs: [] },
        { id: "out", kind: "output", config: { kind: "output" }, inputs: [], outputs: [] },
        { id: "extra", kind: "output", config: { kind: "output" }, inputs: [], outputs: [] },
      ],
      edges: [
        { source_node: "input", source_port: "out", target_node: "retry", target_port: "in" },
        { source_node: "retry", source_port: "out", target_node: "out", target_port: "in" },
      ],
    } as never;
    const result = deserializeWorkflow(legacy);
    expect(result.warnings).toEqual([]);
    expect(result.nodes.map((n) => n.id)).toEqual(["input", "retry", "out", "extra"]);
    // Stable column layout; no title/topology parsing. x wraps 0,280,560,0;
    // y advances only every third node (index 3 → y=220, not index 2).
    expect(result.nodes[0]!.position).toEqual({ x: 0, y: 0 });
    expect(result.nodes[1]!.position).toEqual({ x: 280, y: 0 });
    expect(result.nodes[2]!.position).toEqual({ x: 560, y: 0 });
    expect(result.nodes[3]!.position).toEqual({ x: 0, y: 220 });
    expect(result.edges).toHaveLength(2);
  });

  test("deserialize(serialize(simple input→output)) preserves semantics + positions", () => {
    const nodes: RelayNode[] = [node("in", "input"), node("out", "output")];
    const edges: Edge[] = [edge("e1", "in", "out")];
    const ser = serializeWorkflow(nodes, edges, { id: "rt", name: "RT", version: 9 });
    expect(ser.workflow).not.toBeNull();
    const deser = deserializeWorkflow(ser.workflow!);
    expect(deser.warnings).toEqual([]);
    expect(deser.nodes.map((n) => n.id)).toEqual(["in", "out"]);
    expect(deser.nodes[0]!.position).toEqual({ x: 0, y: 0 });
    expect(deser.edges).toHaveLength(1);
    expect(deser.edges[0]!.source).toBe("in");
    expect(deser.edges[0]!.target).toBe("out");
  });

  test("version metadata is preserved through the round-trip", () => {
    const nodes: RelayNode[] = [node("in", "input"), node("out", "output")];
    const edges: Edge[] = [edge("e1", "in", "out")];
    const ser = serializeWorkflow(nodes, edges, { id: "wf-ver", name: "Ver", version: 42 });
    expect(ser.workflow!.version).toBe(42);
    expect(ser.workflow!.id).toBe("wf-ver");
  });

  test("condition edge keeps its explicit true/false port through save→load", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      {
        ...node("cond", "condition"),
        data: {
          kind: "condition",
          title: "Refund?",
          lines: [],
          canonicalConfig: {
            kind: "condition",
            condition: { field: "user.intent", operator: "equals", value: "refund", valueType: "string" },
          },
        } as RelayNode["data"],
      },
      node("out", "output"),
    ];
    const edges: Edge[] = [
      edge("e1", "in", "cond"),
      { id: "e2", source: "cond", target: "out", sourceHandle: "true" },
    ];
    const ser = serializeWorkflow(nodes, edges, { id: "cond-wf", name: "C", version: 1 });
    expect(ser.errors).toEqual([]);
    const condEdge = ser.workflow!.edges.find((e) => e.id === "e2")!;
    expect(condEdge.source_port).toBe("true");
    expect(condEdge.target_port).toBe("in");
    // The persistent edge round-trips the explicit source port; the default
    // target port ("in") needs no handle.
    const deser = deserializeWorkflow(ser.workflow!);
    const back = deser.edges.find((e) => e.id === "e2")!;
    expect(back.sourceHandle).toBe("true");
    expect(back.targetHandle).toBeUndefined();
  });

  test("condition numeric value round-trips as a number, not a string", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      {
        ...node("cond", "condition"),
        data: {
          kind: "condition",
          title: "Amount?",
          lines: [],
          canonicalConfig: {
            kind: "condition",
            condition: { field: "amount", operator: "greater_than", value: "100", valueType: "number" },
          },
        } as RelayNode["data"],
      },
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "cond"), { id: "e2", source: "cond", target: "out", sourceHandle: "true" }];
    const ser = serializeWorkflow(nodes, edges);
    expect(ser.errors).toEqual([]);
    const cond = ser.workflow!.nodes.find((n) => n.id === "cond")!;
    expect(cond.config).toMatchObject({ kind: "condition", value: 100 });
  });

  test("serialize(deserialize(serialize(x))) is idempotent for LLM semantics", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      {
        ...node("p", "provider", "Anything"),
        data: {
          kind: "provider",
          title: "Anything",
          lines: [],
          canonicalConfig: {
            kind: "llm",
            config: { provider: "anthropic", model: "claude-sonnet", lane: "anthropic-primary", temperature: 0.2, stream: true },
          },
        } as RelayNode["data"],
      },
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "p"), edge("e2", "p", "out")];
    const s1 = serializeWorkflow(nodes, edges, { id: "idem", name: "I", version: 2 });
    expect(s1.errors).toEqual([]);
    const d1 = deserializeWorkflow(s1.workflow!);
    const s2 = serializeWorkflow(d1.nodes as RelayNode[], d1.edges, { id: "idem", name: "I", version: 2 });
    expect(s2.errors).toEqual([]);
    const p1 = s1.workflow!.nodes.find((n) => n.id === "p")!;
    const p2 = s2.workflow!.nodes.find((n) => n.id === "p")!;
    expect(p2.config).toEqual(p1.config);
    // Positions survive too.
    expect(s2.workflow!.nodes.map((n) => n.position)).toEqual(s1.workflow!.nodes.map((n) => n.position));
  });

  test("persisted schema kinds map to editor kinds without a crash", () => {
    // A persisted `llm` node (schema kind) must load as an editable provider,
    // carrying its config through the view — not a title/data crash.
    const persisted = {
      id: "wf", name: "W", version: 1, schema_version: 2,
      nodes: [{ id: "llm1", kind: "llm", config: { kind: "llm", stream: true, lane_id: "anthropic-primary", model: "claude-sonnet", provider: "anthropic" }, inputs: [], outputs: [] }],
      edges: [],
    } as never;
    const result = deserializeWorkflow(persisted);
    const llmView = result.nodes.find((n) => n.id === "llm1")!;
    expect(llmView.data.kind).toBe("provider");
    expect((llmView.data as { canonicalConfig?: { config?: { model?: string } } }).canonicalConfig).toMatchObject({ kind: "llm", config: { model: "claude-sonnet" } });
  });

  test("truly unknown persisted kinds are reported, never invented", () => {
    const persisted = {
      id: "wf", name: "W", version: 1, schema_version: 2,
      nodes: [{ id: "odd", kind: "mystery-kind", config: { kind: "mystery-kind" }, inputs: [], outputs: [] }],
      edges: [],
    } as never;
    // An unmappable kind is a load-time refusal (error), never a crash and
    // never a fabricated node.
    const result = deserializeWorkflow(persisted);
    expect(result.errors.length).toBeGreaterThan(0);
    expect(result.errors.join()).toContain("odd");
    expect(result.nodes).toHaveLength(0);
  });

  test("a workflow with no Input node refuses to serialize", () => {
    const nodes: RelayNode[] = [node("out", "output")];
    const result = serializeWorkflow(nodes, []);
    expect(result.workflow).toBeNull();
    expect(result.errors.join()).toContain("Input");
  });

  test("unconfigured condition node refuses to serialize (no fabricated branch)", () => {
    // A fresh condition node carries defaults (field="", operator="equals",
    // value="") until configured — serializing it would produce a branch the
    // runtime would always evaluate against a fabricated field.
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("cond", "condition"),
      node("out", "output"),
    ];
    const result = serializeWorkflow(nodes, []);
    expect(result.workflow).toBeNull();
    expect(result.errors.join()).toContain("Condition");
  });
});
