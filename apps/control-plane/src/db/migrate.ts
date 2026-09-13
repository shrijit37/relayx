/**
 * Migration CLI: `bun run migrate` — apply pending SQL files then exit.
 */

import { createPool, dbConfigFromEnv, migrate } from "./db";

const pool = createPool(dbConfigFromEnv());
try {
  const applied = await migrate(pool);
  if (applied.length === 0) {
    console.log("schema up to date");
  } else {
    console.log(`applied ${applied.length} migration(s): ${applied.join(", ")}`);
  }
} finally {
  await pool.end();
}