import { describe, it, expect } from "vitest";
import {
  type Workflow,
  NodeKind,
  PortType,
  TransformOperation,
} from "../../types/workflow";
import { validateWorkflow } from "../validation";

function makeWorkflow(overrides: Partial<Workflow> = {}): Workflow {
  return {
    id: "wf_test",
    name: "Test",
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
        target_port: "input",
      },
    ],
    ...overrides,
  };
}

describe("validateWorkflow", () => {
  it("validates a simple Input → Output workflow", () => {
    const errors = validateWorkflow(makeWorkflow());
    expect(errors.filter((e) => e.severity === "error")).toHaveLength(0);
  });

  it("rejects empty workflow", () => {
    const errors = validateWorkflow(makeWorkflow({ nodes: [], edges: [] }));
    expect(errors.some((e) => e.message === "Workflow is empty")).toBe(true);
  });

  it("rejects missing Input node", () => {
    const wf = makeWorkflow({
      nodes: [
        {
          id: "n2",
          kind: NodeKind.Output,
          config: { output_type: PortType.Message },
          inputs: [{ name: "input", port_type: PortType.Message }],
          outputs: [],
        },
      ],
      edges: [],
    });
    const errors = validateWorkflow(wf);
    expect(errors.some((e) => e.message === "Workflow must have an Input node")).toBe(true);
  });

  it("rejects missing Output node", () => {
    const wf = makeWorkflow({
      nodes: [
        {
          id: "n1",
          kind: NodeKind.Input,
          config: { input_type: PortType.Message },
          inputs: [],
          outputs: [{ name: "output", port_type: PortType.Message }],
        },
      ],
      edges: [],
    });
    const errors = validateWorkflow(wf);
    expect(errors.some((e) => e.message === "Workflow must have an Output node")).toBe(true);
  });

  it("rejects duplicate node IDs", () => {
    const wf = makeWorkflow({
      nodes: [
        {
          id: "dup",
          kind: NodeKind.Input,
          config: { input_type: PortType.Message },
          inputs: [],
          outputs: [{ name: "output", port_type: PortType.Message }],
        },
        {
          id: "dup",
          kind: NodeKind.Output,
          config: { output_type: PortType.Message },
          inputs: [{ name: "input", port_type: PortType.Message }],
          outputs: [],
        },
      ],
      edges: [],
    });
    const errors = validateWorkflow(wf);
    expect(errors.some((e) => e.message?.includes("Duplicate node ID"))).toBe(true);
  });

  it("rejects cycles", () => {
    const llm1 = {
      id: "llm1",
      kind: NodeKind.Llm,
      config: { stream: true },
      inputs: [
        { name: "messages", port_type: PortType.Message },
        { name: "tools", port_type: PortType.ToolCall },
      ],
      outputs: [
        { name: "response", port_type: PortType.Message },
        { name: "tool_calls", port_type: PortType.ToolCall },
      ],
    };
    const llm2 = {
      id: "llm2",
      kind: NodeKind.Llm,
      config: { stream: true },
      inputs: [
        { name: "messages", port_type: PortType.Message },
        { name: "tools", port_type: PortType.ToolCall },
      ],
      outputs: [
        { name: "response", port_type: PortType.Message },
        { name: "tool_calls", port_type: PortType.ToolCall },
      ],
    };
    const input = {
      id: "n1",
      kind: NodeKind.Input,
      config: { input_type: PortType.Message },
      inputs: [],
      outputs: [{ name: "output", port_type: PortType.Message }],
    };
    const output = {
      id: "n2",
      kind: NodeKind.Output,
      config: { output_type: PortType.Message },
      inputs: [{ name: "input", port_type: PortType.Message }],
      outputs: [],
    };
    const wf = makeWorkflow({
      nodes: [input, output, llm1, llm2],
      edges: [
        { source_node: "n1", source_port: "output", target_node: "llm1", target_port: "messages" },
        { source_node: "llm1", source_port: "response", target_node: "llm2", target_port: "messages" },
        { source_node: "llm2", source_port: "response", target_node: "llm1", target_port: "messages" }, // cycle
        { source_node: "llm1", source_port: "response", target_node: "n2", target_port: "input" },
      ],
    });
    const errors = validateWorkflow(wf);
    expect(errors.some((e) => e.message === "Workflow contains a cycle")).toBe(true);
  });

  it("warns about unreachable nodes", () => {
    const input = {
      id: "n1",
      kind: NodeKind.Input,
      config: { input_type: PortType.Message },
      inputs: [],
      outputs: [{ name: "output", port_type: PortType.Message }],
    };
    const output = {
      id: "n2",
      kind: NodeKind.Output,
      config: { output_type: PortType.Message },
      inputs: [{ name: "input", port_type: PortType.Message }],
      outputs: [],
    };
    const orphan = {
      id: "orphan",
      kind: NodeKind.Transform,
      config: { operation: TransformOperation.Passthrough },
      inputs: [{ name: "input", port_type: PortType.Message }],
      outputs: [{ name: "output", port_type: PortType.Message }],
    };
    const wf = makeWorkflow({
      nodes: [input, output, orphan],
      edges: [
        { source_node: "n1", source_port: "output", target_node: "n2", target_port: "input" },
      ],
    });
    const errors = validateWorkflow(wf);
    expect(
      errors.some(
        (e) =>
          e.node_id === "orphan" &&
          e.severity === "warning" &&
          e.message.includes("unreachable"),
      ),
    ).toBe(true);
  });

  it("validates LLM node requires model", () => {
    const input = {
      id: "n1",
      kind: NodeKind.Input,
      config: { input_type: PortType.Message },
      inputs: [],
      outputs: [{ name: "output", port_type: PortType.Message }],
    };
    const llm = {
      id: "llm1",
      kind: NodeKind.Llm,
      config: { stream: true }, // no model
      inputs: [
        { name: "messages", port_type: PortType.Message },
        { name: "tools", port_type: PortType.ToolCall },
      ],
      outputs: [
        { name: "response", port_type: PortType.Message },
        { name: "tool_calls", port_type: PortType.ToolCall },
      ],
    };
    const output = {
      id: "n2",
      kind: NodeKind.Output,
      config: { output_type: PortType.Message },
      inputs: [{ name: "input", port_type: PortType.Message }],
      outputs: [],
    };
    const wf = makeWorkflow({
      nodes: [input, output, llm],
      edges: [
        { source_node: "n1", source_port: "output", target_node: "llm1", target_port: "messages" },
        { source_node: "llm1", source_port: "response", target_node: "n2", target_port: "input" },
      ],
    });
    const errors = validateWorkflow(wf);
    expect(
      errors.some(
        (e) =>
          e.node_id === "llm1" &&
          e.message?.includes("model is required"),
      ),
    ).toBe(true);
  });
});