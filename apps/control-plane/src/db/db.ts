/**
 * Thin `pg` pool wrapper. No ORM — SQL is the source of truth and the
 * control plane owns it. Repositories (`src/db/repositories.ts`) sit on top.
 */

import { Pool, type PoolClient, type QueryResult } from "pg";

export type DbConfig = {
  host?: string;
  port?: number;
  database?: string;
  user?: string;
  password?: string;
};

export function dbConfigFromEnv(env = Bun.env): DbConfig {
  return {
    host: env["RELAYX_PG_HOST"] ?? "127.0.0.1",
    port: Number(env["RELAYX_PG_PORT"] ?? 5433),
    database: env["RELAYX_PG_DATABASE"] ?? "relayx",
    user: env["RELAYX_PG_USER"] ?? "relayx",
    password: env["RELAYX_PG_PASSWORD"] ?? "relayx-dev",
  };
}

export function createPool(cfg: DbConfig = dbConfigFromEnv()): Pool {
  return new Pool({ ...cfg, max: 10, idleTimeoutMillis: 30_000 });
}

/** Run one SQL file against the pool. */
export async function runSqlFile(pool: Pool, path: string): Promise<void> {
  const sql = await Bun.file(path).text();
  await pool.query(sql);
}

/**
 * Apply all migration files in `src/db/` in lexical order (idempotent against
 * a `schema_migrations` ledger). Failure stops the run.
 */
export async function migrate(pool: Pool, dir = "src/db"): Promise<string[]> {
  await pool.query(`
    CREATE TABLE IF NOT EXISTS schema_migrations (
      filename TEXT PRIMARY KEY,
      applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )
  `);

  const entries = (await Array.fromAsync(
    new Bun.Glob("*.sql").scan({ cwd: dir, absolute: false }),
  )).sort();

  const applied: string[] = [];
  for (const file of entries) {
    const has = await pool.query("SELECT 1 FROM schema_migrations WHERE filename = $1", [file]);
    if ((has.rowCount ?? 0) > 0) continue;
    await runSqlFile(pool, `${dir}/${file}`);
    await pool.query("INSERT INTO schema_migrations (filename) VALUES ($1)", [file]);
    applied.push(file);
  }
  return applied;
}

export type { PoolClient, QueryResult };