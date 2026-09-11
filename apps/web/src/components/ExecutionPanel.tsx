// Execution panel — bottom bar showing run status and errors

import { useWorkflowStore } from "../store/workflow-store";
import { ExecutionState } from "../types/execution";

const STATE_LABELS: Record<ExecutionState, string> = {
  [ExecutionState.Idle]: "Idle",
  [ExecutionState.Queued]: "Queued",
  [ExecutionState.Running]: "Running…",
  [ExecutionState.Streaming]: "Streaming…",
  [ExecutionState.Success]: "Success",
  [ExecutionState.Error]: "Error",
  [ExecutionState.Cancelled]: "Cancelled",
};

export function ExecutionPanel() {
  const executionState = useWorkflowStore((s) => s.executionState);
  const runResult = useWorkflowStore((s) => s.runResult);
  const errors = useWorkflowStore((s) => s.errors);

  return (
    <div className="execution-panel">
      <div className="execution-status">
        <span
          className={`status-badge status-${executionState.toLowerCase()}`}
        >
          {STATE_LABELS[executionState]}
        </span>
        {runResult?.run_id && (
          <span className="run-id">Run: {runResult.run_id}</span>
        )}
        {runResult?.output && (
          <span className="run-output">{runResult.output}</span>
        )}
      </div>
      {errors.length > 0 && (
        <div className="validation-errors">
          {errors.map((err, i) => (
            <span
              key={i}
              className={`validation-error ${err.severity}`}
            >
              {err.node_id ? `[${err.node_id}] ` : ""}
              {err.message}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}