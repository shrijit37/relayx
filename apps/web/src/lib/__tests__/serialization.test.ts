import { describe, it, expect } from "vitest";
import {
  type Workflow,
  NodeKind,
  PortType,
} from "../../types/workflow";
import {
  workflowToReactFlow,
  reactFlowToWorkflow,
} from "../workflow-serialization";

function sampleWorkflow(): Workflow {
  return {
    id: "wf_001",
    name: "Test WF",
    version: 1,
    nodes: [
      {
        id: "n1",
        kind: NodeKind.Input,
        config: { input_type: PortType.Message },
        inputs: [],
        outputs: [{ name: "output", port_type: PortType.Message }],
      },
      {
        id: "n2",
        kind: NodeKind.Llm,
        config: { model: "gpt-4o", stream: true },
        inputs: [
          { name: "messages", port_type: PortType.Message },
          { name: "tools", port_type: PortType.ToolCall },
        ],
        outputs: [
          { name: "response", port_type: PortType.Message },
          { name: "tool_calls", port_type: PortType.ToolCall },
        ],
      },
      {
        id: "n3",
        kind: NodeKind.Output,
        config: { output_type: PortType.Message },
        inputs: [{ name: "input", port_type: PortType.Message }],
        outputs: [],
      },
    ],
    edges: [
      {
        source_node: "n1",
        source_port: "output",
        target_node: "n2",
        target_port: "messages",
      },
      {
        source_node: "n2",
        source_port: "response",
        target_node: "n3",
        target_port: "input",
      },
    ],
  };
}

describe("serialization", () => {
  it("workflowToReactFlow creates correct RF nodes and edges", () => {
    const { nodes, edges } = workflowToReactFlow(sampleWorkflow());
    expect(nodes).toHaveLength(3);
    expect(edges).toHaveLength(2);
    expect(nodes[0]!.id).toBe("n1");
    expect(nodes[0]!.type).toBe("Input");
    expect(edges[0]!.source).toBe("n1");
    expect(edges[0]!.target).toBe("n2");
  });

  it("round-trips: workflow -> reactflow -> workflow", () => {
    const original = sampleWorkflow();
    const { nodes, edges } = workflowToReactFlow(original);
    const roundTripped = reactFlowToWorkflow(nodes, edges, {
      id: original.id,
      name: original.name,
      version: original.version,
    });
    expect(roundTripped.id).toBe(original.id);
    expect(roundTripped.nodes).toHaveLength(original.nodes.length);
    expect(roundTripped.edges).toHaveLength(original.edges.length);
    for (let i = 0; i < original.nodes.length; i++) {
      expect(roundTripped.nodes[i]!.id).toBe(original.nodes[i]!.id);
      expect(roundTripped.nodes[i]!.kind).toBe(original.nodes[i]!.kind);
    }
    for (let i = 0; i < original.edges.length; i++) {
      expect(roundTripped.edges[i]!.source_node).toBe(
        original.edges[i]!.source_node,
      );
      expect(roundTripped.edges[i]!.target_node).toBe(
        original.edges[i]!.target_node,
      );
    }
  });
});