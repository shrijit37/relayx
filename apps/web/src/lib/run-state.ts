/**
 * Pure execution-state transitions for the Run panel.
 *
 * The only legal source of a state change is a real event: a mutation being
 * submitted (queued → running), a real response (running → completed with
 * output), a real error from the backend envelope (running → failed), or an
 * explicit abort (running → cancelled). `idle` is the initial/stopped state.
 *
 * This reducer is deliberately tiny and framework-free so the state machine
 * is unit-testable without React.
 */

export type RunPhase =
  | "idle" // no run has been started (or the previous run was stopped)
  | "running" // a real run request is in flight (mutation pending)
  | "completed" // the real gateway returned a result
  | "failed" // the real backend/gateway returned an error
  | "cancelled"; // the run was aborted by the user

export type RunState = {
  phase: RunPhase;
  /** Real backend-truth envelope only (not user/fabricated). */
  result?: {
    requestId: string;
    workflowId: string;
    workflowVersion: number;
    snapshotVersion: number;
    planHash: string;
    output: unknown;
  };
  /** Real backend error message. */
  error?: string;
};

export type RunAction =
  | { type: "start" }
  | { type: "completed"; result: NonNullable<RunState["result"]> }
  | { type: "failed"; error: string }
  | { type: "cancel" }
  | { type: "reset" };

export function runReducer(state: RunState, action: RunAction): RunState {
  switch (action.type) {
    case "start":
      return { phase: "running" };
    case "completed":
      return { phase: "completed", result: action.result };
    case "failed":
      return { phase: "failed", error: action.error };
    case "cancel":
      return state.phase === "running" ? { phase: "cancelled" } : state;
    case "reset":
      return { phase: "idle" };
  }
}