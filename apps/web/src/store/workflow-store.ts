import { create } from "zustand";
import {
  type OnNodesChange,
  type OnEdgesChange,
  type Connection,
  applyNodeChanges,
  applyEdgeChanges,
  addEdge,
  MarkerType,
} from "@xyflow/react";
import {
  type Workflow,
  NodeKind,
} from "../types/workflow";
import {
  type RFNodeType,
  type RFNodeData,
  type RFEdgeType,
  createNode,
  workflowToReactFlow,
  reactFlowToWorkflow,
} from "../lib/workflow-serialization";
import { validateWorkflow, type ValidationError } from "../lib/validation";
import {
  ExecutionState,
  type RunResult,
} from "../types/execution";
import { nanoid } from "nanoid";

function makeId(): string {
  return `node_${nanoid(10)}`;
}

function makeWorkflowId(): string {
  return `wf_${nanoid(8)}`;
}

interface WorkflowStore {
  // Workflow metadata
  workflowId: string;
  workflowName: string;
  workflowVersion: number;

  // React Flow state
  nodes: RFNodeType[];
  edges: RFEdgeType[];
  onNodesChange: OnNodesChange<RFNodeType>;
  onEdgesChange: OnEdgesChange<RFEdgeType>;
  onConnect: (connection: Connection) => void;

  // Node operations
  addNode: (kind: NodeKind, position?: { x: number; y: number }) => void;
  duplicateNode: (nodeId: string) => void;
  deleteNode: (nodeId: string) => void;
  updateNodeConfig: (nodeId: string, config: Record<string, unknown>) => void;

  // Selection
  selectedNodeId: string | null;
  selectNode: (nodeId: string | null) => void;

  // Persistence
  dirty: boolean;
  saveWorkflow: () => string;
  loadWorkflow: (json: string) => void;
  newWorkflow: () => void;

  // Validation
  errors: ValidationError[];
  runValidation: () => ValidationError[];

  // Execution
  executionState: ExecutionState;
  runResult: RunResult | null;
  runWorkflow: () => Promise<void>;
  cancelExecution: () => void;
}

export const useWorkflowStore = create<WorkflowStore>((set, get) => {
  const initialId = makeWorkflowId();
  const inputNode = createNode(NodeKind.Input, makeId(), { x: 0, y: 0 });
  const outputNode = createNode(NodeKind.Output, makeId(), { x: 0, y: 200 });

  return {
    workflowId: initialId,
    workflowName: "Untitled Workflow",
    workflowVersion: 1,
    nodes: [inputNode, outputNode],
    edges: [
      {
        id: `${inputNode.id}->${outputNode.id}`,
        source: inputNode.id,
        sourceHandle: "output",
        target: outputNode.id,
        targetHandle: "input",
        markerEnd: { type: MarkerType.ArrowClosed },
      },
    ],
    selectedNodeId: null,
    dirty: false,
    errors: [],
    executionState: ExecutionState.Idle,
    runResult: null,

    onNodesChange: (changes) => {
      set((s) => ({
        nodes: applyNodeChanges(changes, s.nodes) as RFNodeType[],
        dirty: true,
      }));
    },

    onEdgesChange: (changes) => {
      set((s) => ({
        edges: applyEdgeChanges(changes, s.edges),
        dirty: true,
      }));
    },

    onConnect: (connection) => {
      set((s) => ({
        edges: addEdge(
          {
            ...connection,
            markerEnd: { type: MarkerType.ArrowClosed },
          },
          s.edges,
        ),
        dirty: true,
      }));
    },

    addNode: (kind, position) => {
      const id = makeId();
      const pos = position ?? { x: 250, y: 100 + get().nodes.length * 120 };
      const node = createNode(kind, id, pos);
      set((s) => ({
        nodes: [...s.nodes, node],
        dirty: true,
        selectedNodeId: id,
      }));
    },

    duplicateNode: (nodeId) => {
      const s = get();
      const source = s.nodes.find((n) => n.id === nodeId);
      if (!source) return;
      const id = makeId();
      const wfNode = (source.data as unknown as RFNodeData).node;
      const newNode: RFNodeType = {
        ...source,
        id,
        position: {
          x: (source.position.x ?? 0) + 50,
          y: (source.position.y ?? 0) + 50,
        },
        data: {
          ...source.data,
          node: {
            ...wfNode,
            id,
          },
        },
      };
      set((s) => ({
        nodes: [...s.nodes, newNode],
        dirty: true,
        selectedNodeId: id,
      }));
    },

    deleteNode: (nodeId) => {
      set((s) => ({
        nodes: s.nodes.filter((n) => n.id !== nodeId),
        edges: s.edges.filter(
          (e) => e.source !== nodeId && e.target !== nodeId,
        ),
        selectedNodeId:
          s.selectedNodeId === nodeId ? null : s.selectedNodeId,
        dirty: true,
      }));
    },

    updateNodeConfig: (nodeId, configUpdate) => {
      set((s) => ({
        nodes: s.nodes.map((n) => {
          if (n.id !== nodeId) return n;
          const wfNode = (n.data as unknown as RFNodeData).node;
          return {
            ...n,
            data: {
              ...n.data,
              node: {
                ...wfNode,
                config: { ...wfNode.config, ...configUpdate },
              },
            },
          };
        }),
        dirty: true,
      }));
    },

    selectNode: (nodeId) => set({ selectedNodeId: nodeId }),

    saveWorkflow: () => {
      const s = get();
      const wf = reactFlowToWorkflow(s.nodes, s.edges, {
        id: s.workflowId,
        name: s.workflowName,
        version: s.workflowVersion,
      });
      const json = JSON.stringify(wf, null, 2);
      set({ dirty: false });
      return json;
    },

    loadWorkflow: (json) => {
      const wf = JSON.parse(json) as Workflow;
      const { nodes, edges } = workflowToReactFlow(wf);
      set({
        workflowId: wf.id,
        workflowName: wf.name,
        workflowVersion: wf.version,
        nodes: nodes as RFNodeType[],
        edges,
        dirty: false,
        selectedNodeId: null,
      });
    },

    newWorkflow: () => {
      const id = makeWorkflowId();
      const inputNode = createNode(NodeKind.Input, makeId(), {
        x: 0,
        y: 0,
      });
      const outputNode = createNode(NodeKind.Output, makeId(), {
        x: 0,
        y: 200,
      });
      set({
        workflowId: id,
        workflowName: "Untitled Workflow",
        workflowVersion: 1,
        nodes: [inputNode, outputNode],
        edges: [
          {
            id: `${inputNode.id}->${outputNode.id}`,
            source: inputNode.id,
            sourceHandle: "output",
            target: outputNode.id,
            targetHandle: "input",
            markerEnd: { type: MarkerType.ArrowClosed },
          },
        ],
        selectedNodeId: null,
        dirty: false,
        errors: [],
        executionState: ExecutionState.Idle,
        runResult: null,
      });
    },

    runValidation: () => {
      const s = get();
      const wf = reactFlowToWorkflow(s.nodes, s.edges, {
        id: s.workflowId,
        name: s.workflowName,
        version: s.workflowVersion,
      });
      const errors = validateWorkflow(wf);
      set({ errors });
      return errors;
    },

    runWorkflow: async () => {
      const s = get();
      const errors = s.runValidation();
      if (errors.some((e) => e.severity === "error")) {
        return;
      }

      // Set execution state
      set({ executionState: ExecutionState.Running });

      // Simulate execution: mark first node running, then cascade
      // ponytail: in-memory simulation, replace with backend API call when available
      const sortedNodes = topologicalSort(s.nodes, s.edges);

      for (const nodeId of sortedNodes) {
        set((st) => ({
          nodes: st.nodes.map((n) =>
            n.id === nodeId
              ? { ...n, data: { ...n.data, executionState: ExecutionState.Running } }
              : n,
          ),
        }));

        await new Promise((resolve) => setTimeout(resolve, 300 + Math.random() * 200));

        set((st) => ({
          nodes: st.nodes.map((n) =>
            n.id === nodeId
              ? { ...n, data: { ...n.data, executionState: ExecutionState.Success } }
              : n,
          ),
        }));
      }

      const result: RunResult = {
        run_id: `run_${nanoid(8)}`,
        workflow_id: s.workflowId,
        state: ExecutionState.Success,
        node_states: new Map(),
        started_at: Date.now(),
        finished_at: Date.now(),
        output: "Workflow executed successfully",
      };
      set({
        executionState: ExecutionState.Success,
        runResult: result,
      });
    },

    cancelExecution: () => {
      set((s) => ({
        executionState: ExecutionState.Cancelled,
        nodes: s.nodes.map((n) => ({
          ...n,
          data: { ...n.data, executionState: ExecutionState.Idle },
        })),
      }));
    },
  };
});

function topologicalSort(
  nodes: RFNodeType[],
  edges: RFEdgeType[],
): string[] {
  const inDegree = new Map<string, number>();
  const adj = new Map<string, string[]>();
  for (const n of nodes) {
    inDegree.set(n.id, 0);
    adj.set(n.id, []);
  }
  for (const e of edges) {
    const cur = inDegree.get(e.target) ?? 0;
    inDegree.set(e.target, cur + 1);
    adj.get(e.source)?.push(e.target);
  }

  const queue: string[] = [];
  for (const [id, deg] of inDegree) {
    if (deg === 0) queue.push(id);
  }

  const sorted: string[] = [];
  while (queue.length > 0) {
    const id = queue.shift()!;
    sorted.push(id);
    for (const next of adj.get(id) ?? []) {
      const deg = (inDegree.get(next) ?? 1) - 1;
      inDegree.set(next, deg);
      if (deg === 0) queue.push(next);
    }
  }
  return sorted;
}
