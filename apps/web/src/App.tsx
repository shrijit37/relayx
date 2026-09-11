// Main App — brings together palette, canvas, config panel, execution panel

import {
  ReactFlow,
  Controls,
  Background,
  BackgroundVariant,
  type NodeTypes,
  type Node,
  type OnNodesChange,
  type OnEdgesChange,
  type Edge,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import { useWorkflowStore } from "./store/workflow-store";
import { WorkflowNode } from "./components/WorkflowNode";
import { NodePalette } from "./components/NodePalette";
import { ConfigPanel } from "./components/ConfigPanel";
import { ExecutionPanel } from "./components/ExecutionPanel";
import { Header } from "./components/Header";
import { NodeKind } from "./types/workflow";
import { useCallback } from "react";

// Register custom node types — each NodeKind maps to the same component
const nodeTypes: NodeTypes = Object.fromEntries(
  Object.values(NodeKind).map((kind) => [kind, WorkflowNode]),
);

export default function App() {
  const nodes = useWorkflowStore((s) => s.nodes);
  const edges = useWorkflowStore((s) => s.edges);
  const onNodesChange = useWorkflowStore((s) => s.onNodesChange);
  const onEdgesChange = useWorkflowStore((s) => s.onEdgesChange);
  const onConnect = useWorkflowStore((s) => s.onConnect);
  const selectNode = useWorkflowStore((s) => s.selectNode);
  const deleteNode = useWorkflowStore((s) => s.deleteNode);
  const duplicateNode = useWorkflowStore((s) => s.duplicateNode);
  const selectedNodeId = useWorkflowStore((s) => s.selectedNodeId);

  const handlePaneClick = useCallback(() => {
    selectNode(null);
  }, [selectNode]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (
        (e.key === "Delete" || e.key === "Backspace") &&
        selectedNodeId
      ) {
        if (
          (e.target as HTMLElement).tagName === "INPUT" ||
          (e.target as HTMLElement).tagName === "TEXTAREA" ||
          (e.target as HTMLElement).tagName === "SELECT"
        )
          return;
        deleteNode(selectedNodeId);
      }
      if (e.key === "d" && (e.ctrlKey || e.metaKey) && selectedNodeId) {
        e.preventDefault();
        duplicateNode(selectedNodeId);
      }
    },
    [selectedNodeId, deleteNode, duplicateNode],
  );

  return (
    <div className="app-shell" onKeyDown={handleKeyDown} tabIndex={-1}>
      <Header />
      <div className="app-body">
        <aside className="sidebar-left">
          <NodePalette />
        </aside>
        <main className="canvas-area">
          <ReactFlow
            nodes={nodes as Node[]}
            edges={edges as Edge[]}
            onNodesChange={onNodesChange as OnNodesChange}
            onEdgesChange={onEdgesChange as OnEdgesChange}
            onConnect={onConnect}
            onPaneClick={handlePaneClick}
            nodeTypes={nodeTypes}
            fitView
            className="workflow-canvas"
          >
            <Controls />
            <Background variant={BackgroundVariant.Dots} gap={12} size={1} />
          </ReactFlow>
        </main>
        <aside className="sidebar-right">
          <ConfigPanel />
        </aside>
      </div>
      <ExecutionPanel />
    </div>
  );
}