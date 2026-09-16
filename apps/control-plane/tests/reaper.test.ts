/**
 * Run-history reaper tests: crash-stuck `running` rows must be reclaimed as
 * `failed` once they exceed the stale threshold; fresh `running` rows and
 * non-running rows must never be touched by the reaper.
 */

import { afterEach, beforeEach, expect, test } from "bun:test";
import { freshDb } from "./helpers";
import { reapStuckRuns, REAP_STALE_AFTER_MS } from "../src/domain/reaper";

let db: Awaited<ReturnType<typeof freshDb>> | null = null;

beforeEach(async () => {
  db = await freshDb("reaper");
});

afterEach(async () => {
  await db?.close();
});

/** Seed a workflow row (runs.workflow_id has an FK to workflows). */
async function seedWorkflow(id: string): Promise<void> {
  const pool = db!.pool;
  await pool.query(
    `INSERT INTO projects (id, name) VALUES ('proj_default', 'default')
     ON CONFLICT (id) DO NOTHING`,
  );
  await pool.query(
    `INSERT INTO workflows (id, project_id, name, status)
     VALUES ($1, 'proj_default', $1, 'draft') ON CONFLICT (id) DO NOTHING`,
    [id],
  );
}

/** Seed a run row with an explicit started_at (back-dated). */
async function seedRun(
  workflowId: string,
  opts: { status: string; startedAt: Date; error?: string | null },
): Promise<string> {
  const pool = db!.pool;
  await seedWorkflow(workflowId);
  const { rows } = await pool.query<{ id: string }>(
    `INSERT INTO runs
       (id, workflow_id, workflow_version, snapshot_version, plan_hash,
        status, input_body, started_at, completed_at)
     VALUES ($1, $2, 1, 1, 'sha256:test', $3, NULL, $4, NULL)
     RETURNING id`,
    [
      `run_${workflowId}_${Math.random().toString(36).slice(2, 8)}`,
      workflowId,
      opts.status,
      opts.startedAt.toISOString(),
    ],
  );
  return rows[0]!.id;
}

test("reaps a stale running row as failed with a completed_at", async () => {
  const pool = db!.pool;
  const staleId = await seedRun("wf-reap-1", {
    status: "running",
    // 10 minutes ago: beyond the 5-minute threshold.
    startedAt: new Date(Date.now() - 10 * 60 * 1000),
  });

  const n = await reapStuckRuns(pool);
  expect(n).toBe(1);

  const { rows } = await pool.query<{
    status: string;
    error: string | null;
    completed_at: string | null;
  }>(`SELECT status, error, completed_at FROM runs WHERE id = $1`, [staleId]);
  expect(rows[0]!.status).toBe("failed");
  expect(rows[0]!.error).toContain("reaper");
  expect(rows[0]!.completed_at).not.toBeNull();
});

test("leaves a fresh running row (under the threshold) untouched", async () => {
  const pool = db!.pool;
  const freshId = await seedRun("wf-fresh", {
    status: "running",
    startedAt: new Date(), // now — well under 5 minutes.
  });

  const n = await reapStuckRuns(pool);
  expect(n).toBe(0);

  const { rows } = await pool.query<{ status: string }>(
    `SELECT status FROM runs WHERE id = $1`,
    [freshId],
  );
  expect(rows[0]!.status).toBe("running");
});

test("never touches completed/failed/cancelled rows", async () => {
  const pool = db!.pool;
  const completedId = await seedRun("wf-completed", {
    status: "completed",
    startedAt: new Date(Date.now() - 30 * 60 * 1000),
  });
  const failedId = await seedRun("wf-failed", {
    status: "failed",
    startedAt: new Date(Date.now() - 30 * 60 * 1000),
  });
  // Back-date a stale running row to prove the reaper only hits it.
  const staleId = await seedRun("wf-stale", {
    status: "running",
    startedAt: new Date(Date.now() - 30 * 60 * 1000),
  });

  const n = await reapStuckRuns(pool);
  expect(n).toBe(1);

  const result = await pool.query<{ id: string; status: string }>(
    `SELECT id, status FROM runs WHERE id = ANY($1)`,
    [[completedId, failedId, staleId]],
  );
  const byId = new Map(result.rows.map((r) => [r.id, r.status]));
  expect(byId.get(completedId)).toBe("completed");
  expect(byId.get(failedId)).toBe("failed");
  expect(byId.get(staleId)).toBe("failed");
});

test("respects a custom stale threshold", async () => {
  const pool = db!.pool;
  // 10 minutes old, but with a 20-minute threshold it is NOT stale.
  const id = await seedRun("wf-custom-threshold", {
    status: "running",
    startedAt: new Date(Date.now() - 10 * 60 * 1000),
  });

  const n = await reapStuckRuns(pool, 20 * 60 * 1000);
  expect(n).toBe(0);

  const { rows } = await pool.query<{ status: string }>(
    `SELECT status FROM runs WHERE id = $1`,
    [id],
  );
  expect(rows[0]!.status).toBe("running");
});

test("REAP_STALE_AFTER_MS is the 10-minute default", () => {
  expect(REAP_STALE_AFTER_MS).toBe(10 * 60 * 1000);
});
