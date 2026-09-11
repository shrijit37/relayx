// Header bar — workflow name, save/run/deploy actions

import { useWorkflowStore } from "../store/workflow-store";
import { ExecutionState } from "../types/execution";

export function Header() {
  const workflowName = useWorkflowStore((s) => s.workflowName);
  const dirty = useWorkflowStore((s) => s.dirty);
  const saveWorkflow = useWorkflowStore((s) => s.saveWorkflow);
  const runWorkflow = useWorkflowStore((s) => s.runWorkflow);
  const cancelExecution = useWorkflowStore((s) => s.cancelExecution);
  const newWorkflow = useWorkflowStore((s) => s.newWorkflow);
  const runValidation = useWorkflowStore((s) => s.runValidation);
  const executionState = useWorkflowStore((s) => s.executionState);

  const isRunning =
    executionState === ExecutionState.Running ||
    executionState === ExecutionState.Streaming;

  const handleSave = () => {
    const json = saveWorkflow();
    // Copy to clipboard for now — backend API will handle persistence later
    navigator.clipboard.writeText(json).catch(() => {});
  };

  const handleRun = () => {
    runWorkflow();
  };

  return (
    <header className="app-header">
      <div className="header-left">
        <span className="header-brand">Relay-X</span>
        <input
          className="workflow-name-input"
          value={workflowName}
          onChange={(e) =>
            useWorkflowStore.setState({
              workflowName: e.target.value,
              dirty: true,
            })
          }
        />
        {dirty && <span className="dirty-indicator">●</span>}
      </div>
      <div className="header-right">
        <button className="btn btn-secondary" onClick={newWorkflow}>
          New
        </button>
        <button
          className="btn btn-secondary"
          onClick={() => {
            // Load from file
            const input = document.createElement("input");
            input.type = "file";
            input.accept = ".json";
            input.onchange = (e) => {
              const file = (e.target as HTMLInputElement).files?.[0];
              if (!file) return;
              const reader = new FileReader();
              reader.onload = (ev) => {
                const json = ev.target?.result as string;
                useWorkflowStore.getState().loadWorkflow(json);
              };
              reader.readAsText(file);
            };
            input.click();
          }}
        >
          Load
        </button>
        <button className="btn btn-secondary" onClick={handleSave}>
          Save
        </button>
        <button className="btn btn-secondary" onClick={runValidation}>
          Validate
        </button>
        {isRunning ? (
          <button
            className="btn btn-danger"
            onClick={cancelExecution}
          >
            Cancel
          </button>
        ) : (
          <button className="btn btn-primary" onClick={handleRun}>
            Run
          </button>
        )}
      </div>
    </header>
  );
}