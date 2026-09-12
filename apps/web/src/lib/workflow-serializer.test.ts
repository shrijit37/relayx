import { describe, expect, test } from "bun:test";
import type { Edge } from "@xyflow/react";
import type { RelayNode } from "@/components/relay/workflow/nodes";
import { serializeWorkflow } from "@/lib/workflow-serializer";

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
});