/**
 * Defense-in-depth tests: unknown fields and SQL-injection payloads are
 * silently ignored by the repo update functions' field whitelists.
 */

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { freshDb } from "./helpers";
import * as repo from "../src/db/repositories";

let db: Awaited<ReturnType<typeof freshDb>> | null = null;

beforeEach(async () => {
  db = await freshDb("sql");
  await db.pool.query("INSERT INTO projects (id, name) VALUES ('proj1', 'proj1')");
});

afterEach(async () => {
  await db?.close();
});

describe("provider update field whitelist", () => {
  test("unknown fields are silently ignored", async () => {
    const created = await repo.providers.create(db!.pool, {
      project_id: "proj1",
      name: "original",
      protocol: "openai",
      base_url: "https://api.openai.com",
      model: "gpt-4",
    });

    const updated = await repo.providers.update(db!.pool, created.id, {
      name: "updated",
      injected: "1; DROP TABLE providers",
    } as Parameters<typeof repo.providers.update>[2] & Record<string, string>);

    expect(updated!.name).toBe("updated");
    expect(updated!.protocol).toBe("openai");

    // Verify via direct query that injected column didn't land.
    const { rows } = await db!.pool.query(
      "SELECT column_name FROM information_schema.columns WHERE table_name = 'providers' AND column_name = 'injected'",
    );
    expect(rows.length).toBe(0);
  });

  test("SQL-injection payload in a valid field is stored as-is", async () => {
    const created = await repo.providers.create(db!.pool, {
      project_id: "proj1",
      name: "legit",
      protocol: "openai",
      base_url: "https://api.openai.com",
      model: "gpt-4",
    });

    const payload = "test', DROP TABLE providers; --";
    const updated = await repo.providers.update(db!.pool, created.id, { name: payload });
    expect(updated!.name).toBe(payload);

    // Table still exists.
    const { rows } = await db!.pool.query(
      "SELECT oid FROM pg_class WHERE relname = 'providers'",
    );
    expect(rows.length).toBe(1);
  });
});

describe("lane update field whitelist", () => {
  test("unknown fields are silently ignored", async () => {
    const lane = await repo.lanes.create(db!.pool, {
      project_id: "proj1",
      provider_id: null,
      endpoint: "/chat",
      base_url: "http://localhost",
      egress: "direct",
      proxy_url: null,
      policies: [],
      credential_ref: null,
    });

    const updated = await repo.lanes.update(db!.pool, lane.id, {
      endpoint: "/v1/completions",
      injected: "1; DROP TABLE lanes",
    } as Parameters<typeof repo.lanes.update>[2] & Record<string, string>);

    expect(updated!.endpoint).toBe("/v1/completions");

    const { rows } = await db!.pool.query(
      "SELECT column_name FROM information_schema.columns WHERE table_name = 'lanes' AND column_name = 'injected'",
    );
    expect(rows.length).toBe(0);
  });

  test("SQL-injection payload in a valid field is stored as-is", async () => {
    const lane = await repo.lanes.create(db!.pool, {
      project_id: "proj1",
      provider_id: null,
      endpoint: "/chat",
      base_url: "http://localhost",
      egress: "direct",
      proxy_url: null,
      policies: [],
      credential_ref: null,
    });

    const payload = "test', DROP TABLE lanes; --";
    const updated = await repo.lanes.update(db!.pool, lane.id, { endpoint: payload });
    expect(updated!.endpoint).toBe(payload);

    const { rows } = await db!.pool.query(
      "SELECT oid FROM pg_class WHERE relname = 'lanes'",
    );
    expect(rows.length).toBe(1);
  });
});
