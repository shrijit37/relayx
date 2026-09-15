import { describe, expect, test } from "bun:test";
import { runReducer, type RunState } from "@/lib/run-state";

const ok = {
  requestId: "req_1",
  workflowId: "wf",
  workflowVersion: 2,
  snapshotVersion: 7,
  planHash: "plan_abc",
  output: { ok: true },
};

const idle: RunState = { phase: "idle" };

describe("runReducer", () => {
  test("idle → start → running", () => {
    expect(runReducer(idle, { type: "start" })).toEqual({ phase: "running" });
  });

  test("running → completed carries the real result envelope", () => {
    expect(runReducer({ phase: "running" }, { type: "completed", result: ok })).toEqual({
      phase: "completed",
      result: ok,
    });
  });

  test("running → failed carries the real backend error", () => {
    expect(runReducer({ phase: "running" }, { type: "failed", error: "Workflow must be published before it can be run." })).toEqual({
      phase: "failed",
      error: "Workflow must be published before it can be run.",
    });
  });

  test("cancel only fires from running/streaming (a completed run is not clobbered)", () => {
    expect(runReducer(idle, { type: "cancel" })).toEqual(idle);
    expect(runReducer({ phase: "completed", result: ok }, { type: "cancel" })).toEqual({
      phase: "completed",
      result: ok,
    });
    expect(runReducer({ phase: "running" }, { type: "cancel" })).toEqual({ phase: "cancelled" });
    expect(runReducer({ phase: "streaming" }, { type: "cancel" })).toEqual({
      phase: "cancelled",
    });
  });

  test("reset returns to idle", () => {
    expect(runReducer({ phase: "failed", error: "x" }, { type: "reset" })).toEqual(idle);
  });

  test("no fabricated state: idle never has a result", () => {
    const s = runReducer(idle, { type: "reset" });
    expect(s.result).toBeUndefined();
    expect(s.error).toBeUndefined();
  });
});