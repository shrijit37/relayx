import { describe, expect, test } from "bun:test";
import { compareVersions } from "./version-compare";
import type { WorkflowJson } from "./workflow";

const baseWf: WorkflowJson = {
  id: "test-wf",
  name: "test",
  version: 1,
  nodes: [
    { id: "in", kind: "input", config: { kind: "input" }, inputs: [], outputs: [{ name: "out", port_type: "message" }] },
    { id: "llm", kind: "llm", config: { kind: "llm", model: "gpt-4", stream: true }, inputs: [{ name: "in", port_type: "message" }], outputs: [{ name: "out", port_type: "message" }] },
    { id: "out", kind: "output", config: { kind: "output" }, inputs: [{ name: "in", port_type: "message" }], outputs: [] },
  ],
  edges: [
    { id: "e1", source_node: "in", source_port: "out", target_node: "llm", target_port: "in" },
    { id: "e2", source_node: "llm", source_port: "out", target_node: "out", target_port: "in" },
  ],
};

function cloneJson(wf: WorkflowJson): WorkflowJson {
  return JSON.parse(JSON.stringify(wf)) as WorkflowJson;
}

describe("version-compare", () => {
  test("identical versions produce no diffs", () => {
    const result = compareVersions(baseWf, baseWf);
    expect(result.addedCount).toBe(0);
    expect(result.removedCount).toBe(0);
    expect(result.changedCount).toBe(0);
    expect(result.nodes.length).toBe(0);
    expect(result.edges.length).toBe(0);
  });

  test("detects added node", () => {
    const v2 = cloneJson(baseWf);
    v2.nodes.push({
      id: "condition-1",
      kind: "condition",
      config: { kind: "condition", condition: "field == value", field: "field", operator: "equals", value: "value" },
      inputs: [{ name: "in", port_type: "message" }],
      outputs: [
        { name: "true", port_type: "message" },
        { name: "false", port_type: "message" },
      ],
    });

    const result = compareVersions(baseWf, v2);
    expect(result.addedCount).toBe(1);
    const added = result.nodes.find((n) => n.kind === "added");
    expect(added).toBeDefined();
    if (added && added.type === "node") expect(added.nodeId).toBe("condition-1");
  });

  test("detects removed node", () => {
    const v2 = cloneJson(baseWf);
    v2.nodes = v2.nodes.filter((n) => n.id !== "llm");
    v2.edges = v2.edges.filter((e) => e.source_node !== "llm" && e.target_node !== "llm");

    const result = compareVersions(baseWf, v2);
    expect(result.removedCount).toBeGreaterThanOrEqual(1);
    const removed = result.nodes.find((n) => n.kind === "removed");
    expect(removed).toBeDefined();
    if (removed && removed.type === "node") expect(removed.nodeId).toBe("llm");
  });

  test("detects changed config", () => {
    const v2 = cloneJson(baseWf);
    const llmNode = v2.nodes.find((n) => n.id === "llm");
    if (llmNode) {
      // Schema-node config is flat: { kind: "llm", model, stream }.
      (llmNode.config as Record<string, unknown>)["model"] = "claude-opus-4";
    }

    const result = compareVersions(baseWf, v2);
    expect(result.changedCount).toBe(1);
    const changed = result.nodes.find((n) => n.kind === "changed");
    expect(changed).toBeDefined();
    if (changed && changed.type === "node") {
      expect(changed.fields).toBeDefined();
      // The canonical LLM config nests the request config under
      // config.config, so the field surfaces as config.config.
      expect(changed.fields!.some((f) => f.field === "config.config")).toBe(true);
    }
  });

  test("detects added edge", () => {
    const v2 = cloneJson(baseWf);
    v2.edges.push({
      id: "e3",
      source_node: "in",
      source_port: "out",
      target_node: "out",
      target_port: "in",
    });

    const result = compareVersions(baseWf, v2);
    expect(result.addedCount).toBe(1);
    const addedEdge = result.edges.find((e) => e.kind === "added");
    expect(addedEdge).toBeDefined();
  });

  test("detects removed edge", () => {
    const v2 = cloneJson(baseWf);
    v2.edges = v2.edges.filter((_, i) => i !== 0);

    const result = compareVersions(baseWf, v2);
    expect(result.removedCount).toBe(1);
    const removedEdge = result.edges.find((e) => e.kind === "removed");
    expect(removedEdge).toBeDefined();
  });

  test("no false positives from position/presentation differences", () => {
    const v1 = cloneJson(baseWf);
    const v2 = cloneJson(baseWf);
    v1.nodes[0]!.position = { x: 0, y: 0 };
    v2.nodes[0]!.position = { x: 100, y: 200 };
    v1.nodes[0]!.presentation = { title: "A" };
    v2.nodes[0]!.presentation = { title: "B" };

    const result = compareVersions(v1, v2);
    expect(result.addedCount).toBe(0);
    expect(result.removedCount).toBe(0);
    expect(result.changedCount).toBe(0);
  });
});
