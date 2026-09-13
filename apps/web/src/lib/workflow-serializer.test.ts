import { describe, expect, test } from "bun:test";
import type { Edge } from "@xyflow/react";
import type { RelayNode } from "@/components/relay/workflow/nodes";
import { deserializeWorkflow, serializeWorkflow } from "@/lib/workflow-serializer";

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

describe("serializeWorkflow", () => {
  test("serializes input → route → output with lane folding", () => {
    const nodes: RelayNode[] = [
      node("in", "input", "HTTP Request"),
      node("route", "route", "Model Router · claude-sonnet"),
      node("lane", "lane", "anthropic-us"),
      node("out", "output", "SSE Response"),
    ];
    const edges: Edge[] = [
      edge("e1", "in", "route"),
      edge("e2", "lane", "route"),
      edge("e3", "route", "out"),
    ];

    const result = serializeWorkflow(nodes, edges, { id: "wf-root", name: "root", version: 3 });
    expect(result.errors).toEqual([]);
    const wf = result.workflow!;
    expect(wf.id).toBe("wf-root");
    expect(wf.version).toBe(3);
    expect(wf.nodes.map((n) => n.kind)).toEqual(["input", "router", "output"]);
    expect(result.lanesFolded).toBe(1);
  });

  test("lane folds into a provider node config", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("provider", "provider", "OpenAI · gpt-4o"),
      node("lane", "lane", "openai-direct"),
      node("out", "output"),
    ];
    const edges: Edge[] = [
      edge("e1", "in", "provider"),
      edge("e2", "lane", "provider"),
      edge("e3", "provider", "out"),
    ];

    const wf = serializeWorkflow(nodes, edges).workflow!;
    const provider = wf.nodes.find((n) => n.id === "provider")!;
    expect(provider.config.kind).toBe("llm");
    if (provider.config.kind === "llm") {
      expect(provider.config.lane_id).toBe("openai-direct");
      expect(provider.config.model).toBe("gpt-4o");
      expect(provider.config.stream).toBe(true);
    }
  });

  test("rejects workflows without input/output", () => {
    const nodes: RelayNode[] = [node("a", "provider"), node("b", "output")];
    const edges: Edge[] = [edge("e1", "a", "b")];
    const result = serializeWorkflow(nodes, edges);
    expect(result.workflow).toBeNull();
    expect(result.errors.join()).toContain("no Input node");
  });

  test("drops display-only nodes with a warning", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("policy", "policy", "production-standard"),
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "policy"), edge("e2", "policy", "out")];
    const result = serializeWorkflow(nodes, edges);
    expect(result.workflow).not.toBeNull();
    expect(result.warnings.some((w) => w.includes("policy"))).toBe(true);
    expect(result.workflow!.nodes.map((n) => n.kind)).not.toContain("mcp");
  });

  test("rejects unconfigured condition nodes (no machine condition)", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("cond", "condition", "latency < 150ms"),
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "cond"), edge("e2", "cond", "out", "true")];
    const result = serializeWorkflow(nodes, edges);
    // The editor stores no field/operator/value for a condition — emitting a
    // fabricated one would compile a workflow that always fails at runtime.
    // Rejecting the editor state is the honest outcome.
    expect(result.workflow).toBeNull();
    expect(result.errors.join()).toContain("condition");
  });

  test("deserializes simple input → output workflow", () => {
    const wf = {
      id: "simple",
      name: "Simple",
      version: 1,
      nodes: [
        { id: "input", kind: "input" as const, config: { kind: "input" as const }, inputs: [{ name: "in", port_type: "message" }], outputs: [{ name: "out", port_type: "message" }] },
        { id: "output", kind: "output" as const, config: { kind: "output" as const }, inputs: [{ name: "in", port_type: "message" }], outputs: [{ name: "out", port_type: "message" }] },
      ],
      edges: [{ source_node: "input", source_port: "out", target_node: "output", target_port: "in" }],
    };
    const result = deserializeWorkflow(wf);
    expect(result.warnings).toEqual([]);
    expect(result.nodes.map((n) => n.id)).toEqual(["input", "output"]);
    expect(result.edges).toHaveLength(1);
    expect(result.edges[0]!.source).toBe("input");
    expect(result.edges[0]!.target).toBe("output");
  });

  test("unfold lane_id back into synthetic lane node", () => {
    const wf = {
      id: "lane-test",
      name: "Lane test",
      version: 1,
      nodes: [
        { id: "in", kind: "input" as const, config: { kind: "input" as const }, inputs: [], outputs: [] },
        { id: "provider", kind: "llm" as const, config: { kind: "llm" as const, model: "claude-sonnet", stream: true, lane_id: "us-vpn" }, inputs: [], outputs: [] },
        { id: "out", kind: "output" as const, config: { kind: "output" as const }, inputs: [], outputs: [] },
      ],
      edges: [
        { source_node: "in", source_port: "out", target_node: "provider", target_port: "in" },
        { source_node: "provider", source_port: "out", target_node: "out", target_port: "in" },
      ],
    };
    const result = deserializeWorkflow(wf);
    expect(result.warnings).toEqual([]);
    const lane = result.nodes.find((n) => n.data.kind === "lane");
    expect(lane).toBeDefined();
    expect(lane!.data.title).toBe("us-vpn");
    expect(lane!.data.lines.some((l) => l.includes("claude-sonnet"))).toBe(true);
  });

  test("deserializes node config kinds correctly", () => {
    const wf = {
      id: "kinds",
      name: "Kinds",
      version: 1,
      nodes: [
        { id: "input", kind: "input" as const, config: { kind: "input" as const }, inputs: [], outputs: [] },
        { id: "router", kind: "router" as const, config: { kind: "router" as const, strategy: "round_robin" as const }, inputs: [], outputs: [] },
        { id: "retry", kind: "retry" as const, config: { kind: "retry" as const, max_attempts: 3, delay_ms: 500, on_timeout: true, on_provider_error: true, target: { kind: "llm" as const, stream: false } }, inputs: [], outputs: [] },
        { id: "out", kind: "output" as const, config: { kind: "output" as const }, inputs: [], outputs: [] },
      ],
      edges: [
        { source_node: "input", source_port: "out", target_node: "router", target_port: "in" },
        { source_node: "router", source_port: "out", target_node: "retry", target_port: "in" },
        { source_node: "retry", source_port: "out", target_node: "out", target_port: "in" },
      ],
    };
    const result = deserializeWorkflow(wf);
    const kinds = result.nodes.map((n) => [n.id, n.data.kind]);
    expect(kinds).toEqual([["input", "input"], ["router", "route"], ["retry", "retry"], ["out", "output"]]);
    expect(result.warnings).toEqual([]);
  });

  test("malformed workflow JSON: unknown kind is skipped with a warning, input/output retained", () => {
    const wf = {
      id: "bad",
      name: "Bad",
      version: 1,
      nodes: [
        { id: "input", kind: "input" as const, config: { kind: "input" as const }, inputs: [], outputs: [] },
        { id: "wtf", kind: "quantum" as never, config: { kind: "quantum" as never }, inputs: [], outputs: [] },
        { id: "output", kind: "output" as const, config: { kind: "output" as const }, inputs: [], outputs: [] },
      ],
      edges: [
        { source_node: "input", source_port: "out", target_node: "wtf", target_port: "in" },
        { source_node: "wtf", source_port: "out", target_node: "output", target_port: "in" },
      ],
    };
    const result = deserializeWorkflow(wf);
    // Unmappable node dropped with a warning; the real input/output survive.
    expect(result.warnings.some((w) => w.includes("quantum"))).toBe(true);
    const ids = result.nodes.map((n) => n.id);
    expect(ids).toContain("input");
    expect(ids).toContain("output");
    expect(ids).not.toContain("wtf");
  });

  test("missing fields: empty nodes/edges yields empty canvas, no throw", () => {
    const result = deserializeWorkflow({ id: "e", name: "E", version: 1, nodes: [], edges: [] });
    expect(result.warnings).toEqual([]);
    expect(result.nodes).toEqual([]);
    expect(result.edges).toEqual([]);
  });

  test("lane preservation: lane_id survives serialize → deserialize round-trip", () => {
    const nodes: RelayNode[] = [
      node("in", "input", "HTTP"),
      node("provider", "provider", "OpenAI · gpt-4o"),
      node("lane", "lane", "openai-direct"),
      node("out", "output", "SSE"),
    ];
    const edges: Edge[] = [
      edge("e1", "in", "provider"),
      edge("e2", "lane", "provider"),
      edge("e3", "provider", "out"),
    ];
    const ser = serializeWorkflow(nodes, edges, { id: "lane-roundtrip", name: "LR", version: 9 });
    expect(ser.workflow).not.toBeNull();
    const deser = deserializeWorkflow(ser.workflow!);
    const lane = deser.nodes.find((n) => n.data.kind === "lane");
    expect(lane).toBeDefined();
    expect(lane!.data.title).toBe("openai-direct");
    const provider = deser.nodes.find((n) => n.data.kind === "provider");
    expect(provider).toBeDefined();
  });

  test("version metadata is preserved through the round-trip", () => {
    const nodes: RelayNode[] = [
      node("in", "input"),
      node("out", "output"),
    ];
    const edges: Edge[] = [edge("e1", "in", "out")];
    const ser = serializeWorkflow(nodes, edges, { id: "wf-ver", name: "Ver", version: 42 });
    expect(ser.workflow!.version).toBe(42);
    expect(ser.workflow!.id).toBe("wf-ver");
  });

  test("round-trip: deserialize(serialize(nodes)) preserves nodes and edges", () => {
    const original: RelayNode[] = [
      node("in", "input", "HTTP Request"),
      node("provider", "provider", "Model · claude-sonnet"),
      node("lane", "lane", "anthropic-us"),
      node("out", "output", "SSE Response"),
    ];
    const origEdges: Edge[] = [
      edge("e1", "in", "provider"),
      edge("e2", "lane", "provider"),
      edge("e3", "provider", "out"),
    ];
    const ser = serializeWorkflow(original, origEdges, { id: "roundtrip", name: "Round trip", version: 1 });
    expect(ser.workflow).not.toBeNull();
    const deser = deserializeWorkflow(ser.workflow!);
    expect(deser.warnings).toEqual([]);
    const ids = deser.nodes.map((n) => n.id);
    expect(ids).toContain("in");
    expect(ids).toContain("out");
    expect(ids).toContain("provider");
    // The lane node was unfolded back from lane_id
    expect(ids.some((id) => id.startsWith("lane-"))).toBe(true);
    expect(deser.edges.some((e) => e.source === "in" && e.target === "provider")).toBe(true);
    expect(deser.edges.some((e) => e.source === "provider" && e.target === "out")).toBe(true);
  });
});