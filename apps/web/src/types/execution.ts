import type { NodeId } from "./workflow";

export enum ExecutionState {
  Idle = "idle",
  Queued = "queued",
  Running = "running",
  Streaming = "streaming",
  Success = "success",
  Error = "error",
  Cancelled = "cancelled",
}

export interface NodeExecutionStatus {
  state: ExecutionState;
  started_at?: number;
  finished_at?: number;
  error?: string;
  tokens_used?: number;
}

export interface RunResult {
  run_id: string;
  workflow_id: string;
  state: ExecutionState;
  node_states: Map<NodeId, NodeExecutionStatus>;
  started_at: number;
  finished_at?: number;
  output?: string;
  error?: string;
}
