/**
 * Control-plane entry point.
 *
 * Boots: Postgres pool → migrations → Fastify API wired to the gateway admin.
 * The data plane keeps running from whatever bundle was last published even
 * if this process dies — the control plane is never on the request path.
 */

import { buildApp } from "./api/routes";
import { createPool, dbConfigFromEnv, migrate } from "./db/db";
import { GatewayClient } from "./gateway/client";

const PORT = Number(Bun.env["RELAYX_CONTROL_PORT"] ?? 9091);
const GATEWAY_ADMIN = Bun.env["RELAYX_GATEWAY_ADMIN_URL"] ?? "http://127.0.0.1:9090";

const pool = createPool(dbConfigFromEnv());

// Migrations are part of startup: a clean DB is reproducible, an existing
// DB gains new versions non-destructively.
const applied = await migrate(pool);
if (applied.length > 0) console.log(`[migrate] applied: ${applied.join(", ")}`);

// Seed one default project so API calls work out of the box.
await pool.query(
  "INSERT INTO projects (id, name) VALUES ('proj_default','default') ON CONFLICT (id) DO NOTHING",
);

const gateway = new GatewayClient(GATEWAY_ADMIN);
const app = await buildApp({ pool, gateway });

try {
  await app.listen({ port: PORT, host: "0.0.0.0" });
} catch (err) {
  app.log.error(err);
  process.exit(1);
}

// Control plane is durable + independent of the data plane: leaving this
// running keeps serving CRUD; gateway publish cadence is driven by calls.
process.on("SIGINT", async () => {
  await app.close();
  await pool.end();
  process.exit(0);
});