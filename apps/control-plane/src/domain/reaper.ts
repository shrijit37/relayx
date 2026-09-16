/**
 * Run-history reaper — reclaims crash-stuck `running` rows.
 *
 * The web run handler creates a `running` row before/at stream start and
 * finalizes it with a terminal status. If the control plane dies mid-run
 * (crash, deploy, OOM), the row is never finalized and stays `running`
 * forever: the frontend would poll it every 10s indefinitely.
 *
 * `reapStuckRuns` marks rows that have been `running` longer than a stale
 * threshold as `failed` (honest terminal status + completed_at) so they stop
 * looking live. Runs under the threshold are never touched. This is a soft
 * reaper: it never deletes rows, never re-runs them, and never touches any
 * non-`running` row.
 *
 * NOTE: There is no per-run heartbeat or lease mechanism yet. The stale
 * threshold is a trade-off — too short risks killing legitimate long-running
 * streams; too long leaves zombie rows. 10 minutes covers the vast majority
 * of LLM request durations while limiting zombie visibility. A heartbeat
 * column + periodic PATCH from the streaming pump is the proper fix and
 * should be added when run-history reliability becomes a priority.
 */

import type { Pool } from "pg";

export const REAP_STALE_AFTER_MS = 10 * 60 * 1000; // 10 minutes

/**
 * Mark `running` rows older than `staleAfterMs` as failed. Returns the
 * number of rows reclaimed. Never throws — run history must never take the
 * control plane down; a failing reap is logged and skipped.
 */
export async function reapStuckRuns(
  pool: Pool,
  staleAfterMs = REAP_STALE_AFTER_MS,
): Promise<number> {
  const { rows } = await pool.query<{ count: string }>(
    `UPDATE runs
       SET status = 'failed',
           error = 'terminated by reaper (stale running)',
           completed_at = now()
     WHERE status = 'running'
       AND started_at < now() - ($1::int || ' milliseconds')::interval
     RETURNING id`,
    [staleAfterMs],
  );
  return rows.length;
}

/**
 * Start the periodic reaper. Runs once immediately, then on an interval.
 * Failures are logged, never fatal. Returns a handle to stop the loop (used
 * by tests and clean shutdown paths).
 */
export function startReaper(
  pool: Pool,
  opts: { intervalMs?: number; staleAfterMs?: number } = {},
): { stop: () => void } {
  const intervalMs = opts.intervalMs ?? 60_000;
  const reap = async () => {
    try {
      const n = await reapStuckRuns(pool, opts.staleAfterMs);
      if (n > 0) {
        console.warn(`[reaper] reclaimed ${n} stuck 'running' run(s)`);
      }
    } catch (e) {
      console.warn("[reaper] reap failed:", e);
    }
  };
  void reap();
  const handle = setInterval(reap, intervalMs);
  return { stop: () => clearInterval(handle) };
}
