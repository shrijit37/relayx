// Custom node component, rendered per node kind

import { Handle, Position } from "@xyflow/react";
import type { PortDef } from "../types/workflow";
import { getNodeTypeInfo } from "../lib/node-registry";
import {
  ExecutionState,
} from "../types/execution";
import type { RFNodeData } from "../lib/workflow-serialization";
import { useWorkflowStore } from "../store/workflow-store";

const STATE_INDICATORS: Record<ExecutionState, string> = {
  [ExecutionState.Idle]: "○",
  [ExecutionState.Queued]: "…",
  [ExecutionState.Running]: "●",
  [ExecutionState.Streaming]: "▸",
  [ExecutionState.Success]: "✓",
  [ExecutionState.Error]: "✕",
  [ExecutionState.Cancelled]: "⊘",
};

const STATE_COLORS: Record<ExecutionState, string> = {
  [ExecutionState.Idle]: "#888",
  [ExecutionState.Queued]: "#888",
  [ExecutionState.Running]: "#4ea1ff",
  [ExecutionState.Streaming]: "#4ea1ff",
  [ExecutionState.Success]: "#4caf50",
  [ExecutionState.Error]: "#f44336",
  [ExecutionState.Cancelled]: "#9e9e9e",
};

function PortHandles({
  ports,
  side,
}: {
  ports: PortDef[];
  side: "input" | "output";
}) {
  return (
    <>
      {ports.map((port, idx) => (
        <Handle
          key={port.name}
          id={port.name}
          type={side === "input" ? "target" : "source"}
          position={side === "input" ? Position.Left : Position.Right}
          className="workflow-handle"
          title={`${port.name} (${port.port_type})`}
          style={
            ports.length > 1
              ? {
                  top: `${((idx + 1) / (ports.length + 1)) * 100}%`,
                }
              : undefined
          }
        />
      ))}
    </>
  );
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function WorkflowNode(props: any) {
  const data = props.data as RFNodeData;
  const node = data.node;
  const info = getNodeTypeInfo(node.kind);
  const selectNode = useWorkflowStore((s) => s.selectNode);
  const selectedNodeId = useWorkflowStore((s) => s.selectedNodeId);

  const isSelected = selectedNodeId === node.id;
  const state = data.executionState ?? ExecutionState.Idle;

  return (
    <div
      className={`workflow-node node-${node.kind.toLowerCase()} ${
        isSelected ? "selected" : ""
      }`}
      onClick={(e) => {
        e.stopPropagation();
        selectNode(node.id);
      }}
    >
      <div className="node-header">
        <span className="node-icon">{info.icon}</span>
        <span className="node-title">{info.label}</span>
        <span
          className="node-state"
          style={{ color: STATE_COLORS[state] }}
        >
          {STATE_INDICATORS[state]}
        </span>
      </div>
      <div className="node-port-labels">
        {node.inputs.length > 0 && (
          <div className="port-labels-side">
            {node.inputs.map((p: PortDef) => (
              <span key={p.name}>
                {p.name} <em>{p.port_type}</em>
              </span>
            ))}
          </div>
        )}
        {node.outputs.length > 0 && (
          <div className="port-labels-side port-labels-right">
            {node.outputs.map((p: PortDef) => (
              <span key={p.name}>
                {p.name} <em>{p.port_type}</em>
              </span>
            ))}
          </div>
        )}
      </div>
      <PortHandles ports={node.inputs} side="input" />
      <PortHandles ports={node.outputs} side="output" />
    </div>
  );
}