/**
 * Repository layer — the only place raw SQL lives.
 *
 * Each repository wraps a `pg.Pool` and returns typed domain objects.
 * This boundary keeps routes and services SQL-free.
 */

import type { Pool } from "pg";
import type { CredentialRef } from "../secrets";

// ── Domain types ───────────────────────────────────────────────────────

export type WorkflowJson = Record<string, unknown>;

export type WorkflowRow = {
  id: string;
  project_id: string;
  name: string;
  status: string;
  created_at: string;
  updated_at: string;
};

export type WorkflowVersionRow = {
  id: string;
  workflow_id: string;
  version: number;
  workflow_json: WorkflowJson;
  plan_hash: string | null;
  status: string;
  created_at: string;
};

export type ProviderRow = {
  id: string;
  project_id: string;
  name: string;
  protocol: string;
  base_url: string;
  model: string;
  created_at: string;
  updated_at: string;
};

export type LaneRow = {
  id: string;
  project_id: string;
  provider_id: string | null;
  endpoint: string;
  base_url: string;
  egress: string;
  policies: string[];
  credential_ref: CredentialRef | null;
  created_at: string;
  updated_at: string;
};

export type PublicationRow = {
  id: string;
  workflow_id: string;
  workflow_version: number;
  plan_hash: string;
  snapshot_version: number;
  status: string;
  error: string | null;
  published_at: string;
};

export type WorkflowActiveRow = {
  workflow_id: string;
  workflow_version: number;
  plan_hash: string;
  snapshot_version: number;
  updated_at: string;
};

// ── Helpers ────────────────────────────────────────────────────────────

/** Update SQL uses only these columns, regardless of what a caller passes.
 *  Unknown fields are silently ignored rather than interpolated into SQL. */
const ALLOWED_PROVIDER_UPDATE_FIELDS = new Set(["name", "protocol", "base_url", "model"]);
const ALLOWED_LANE_UPDATE_FIELDS = new Set(["provider_id", "endpoint", "base_url", "egress", "policies", "credential_ref"]);
const ALLOWED_RUN_UPDATE_FIELDS = new Set(["status", "output", "error", "completed_at"]);

const newId = (): string => crypto.randomUUID();

// ── Workflows ──────────────────────────────────────────────────────────

export const workflows = {
  async create(
    pool: Pool,
    projectId: string,
    name: string,
    id?: string,
  ): Promise<WorkflowRow> {
    const { rows } = await pool.query<WorkflowRow>(
      `INSERT INTO workflows (id, project_id, name) VALUES ($1,$2,$3)
       RETURNING *`,
      [id ?? newId(), projectId, name],
    );
    return rows[0]!;
  },

  async get(pool: Pool, id: string): Promise<WorkflowRow | null> {
    const { rows } = await pool.query<WorkflowRow>("SELECT * FROM workflows WHERE id = $1", [id]);
    return rows[0] ?? null;
  },

  async update(pool: Pool, id: string, name: string): Promise<WorkflowRow | null> {
    const { rows } = await pool.query<WorkflowRow>(
      `UPDATE workflows SET name = $2, updated_at = now() WHERE id = $1 RETURNING *`,
      [id, name],
    );
    return rows[0] ?? null;
  },

  async setStatus(pool: Pool, id: string, status: string): Promise<void> {
    await pool.query(
      "UPDATE workflows SET status = $2, updated_at = now() WHERE id = $1",
      [id, status],
    );
  },

  async listVersions(pool: Pool, workflowId: string): Promise<WorkflowVersionRow[]> {
    const { rows } = await pool.query<WorkflowVersionRow>(
      "SELECT * FROM workflow_versions WHERE workflow_id = $1 ORDER BY version DESC",
      [workflowId],
    );
    return rows;
  },

  async createVersion(
    pool: Pool,
    workflowId: string,
    version: number,
    workflowJson: WorkflowJson,
  ): Promise<WorkflowVersionRow> {
    const { rows } = await pool.query<WorkflowVersionRow>(
      `INSERT INTO workflow_versions (id, workflow_id, version, workflow_json) VALUES ($1,$2,$3,$4)
       RETURNING *`,
      [newId(), workflowId, version, workflowJson],
    );
    return rows[0]!;
  },

  /** Atomically allocate + insert the next version under `workflow_id`.
   *  `max(version)+1` in the same INSERT avoids the concurrent-publish race
   *  where two callers compute the same `next` and one violates the
   *  UNIQUE(workflow_id, version) constraint (review D2). */
  async createNextVersion(
    pool: Pool,
    workflowId: string,
    workflowJson: WorkflowJson,
  ): Promise<WorkflowVersionRow> {
    const { rows } = await pool.query<WorkflowVersionRow>(
      `INSERT INTO workflow_versions (id, workflow_id, version, workflow_json)
       SELECT $1, $2, COALESCE(MAX(version), 0) + 1, $3
       FROM workflow_versions
       WHERE workflow_id = $2
       RETURNING *`,
      [newId(), workflowId, workflowJson],
    );
    return rows[0]!;
  },

  async updateVersionStatus(
    pool: Pool,
    workflowId: string,
    version: number,
    status: string,
    planHash: string | null = null,
  ): Promise<void> {
    await pool.query(
      "UPDATE workflow_versions SET status = $3, plan_hash = COALESCE($4, plan_hash) WHERE workflow_id = $1 AND version = $2",
      [workflowId, version, status, planHash],
    );
  },

  async remove(pool: Pool, id: string): Promise<void> {
    await pool.query("DELETE FROM workflows WHERE id = $1", [id]);
  },

  async getActiveVersion(pool: Pool, workflowId: string): Promise<WorkflowActiveRow | null> {
    const { rows } = await pool.query<WorkflowActiveRow>(
      "SELECT * FROM workflow_active WHERE workflow_id = $1",
      [workflowId],
    );
    return rows[0] ?? null;
  },
};

// ── Providers ──────────────────────────────────────────────────────────

export const providers = {
  async list(pool: Pool, projectId: string): Promise<ProviderRow[]> {
    const { rows } = await pool.query<ProviderRow>(
      "SELECT * FROM providers WHERE project_id = $1 ORDER BY created_at",
      [projectId],
    );
    return rows;
  },

  async get(pool: Pool, id: string): Promise<ProviderRow | null> {
    const { rows } = await pool.query<ProviderRow>("SELECT * FROM providers WHERE id = $1", [id]);
    return rows[0] ?? null;
  },

  async create(
    pool: Pool,
    data: Omit<ProviderRow, "id" | "created_at" | "updated_at">,
  ): Promise<ProviderRow> {
    const { rows } = await pool.query<ProviderRow>(
      `INSERT INTO providers (id, project_id, name, protocol, base_url, model)
       VALUES ($1,$2,$3,$4,$5,$6) RETURNING *`,
      [newId(), data.project_id, data.name, data.protocol, data.base_url, data.model],
    );
    return rows[0]!;
  },

  async update(
    pool: Pool,
    id: string,
    data: Partial<Pick<ProviderRow, "name" | "protocol" | "base_url" | "model">>,
  ): Promise<ProviderRow | null> {
    const fields: string[] = [];
    const values: unknown[] = [id];
    let idx = 2;
    const allowed = ALLOWED_PROVIDER_UPDATE_FIELDS;
    for (const [k, v] of Object.entries(data).filter(([k]) => allowed.has(k))) {
      if (v !== undefined) {
        fields.push(`${k} = $${idx}`);
        values.push(v);
        idx++;
      }
    }
    if (fields.length === 0) return providers.get(pool, id);
    const { rows } = await pool.query<ProviderRow>(
      `UPDATE providers SET ${fields.join(", ")}, updated_at = now() WHERE id = $1 RETURNING *`,
      values,
    );
    return rows[0] ?? null;
  },

  async remove(pool: Pool, id: string): Promise<void> {
    await pool.query("DELETE FROM providers WHERE id = $1", [id]);
  },
};

// ── Lanes ──────────────────────────────────────────────────────────────

export const lanes = {
  async list(pool: Pool, projectId: string): Promise<LaneRow[]> {
    const { rows } = await pool.query<LaneRow>(
      "SELECT * FROM lanes WHERE project_id = $1 ORDER BY created_at",
      [projectId],
    );
    return rows;
  },

  async get(pool: Pool, id: string): Promise<LaneRow | null> {
    const { rows } = await pool.query<LaneRow>("SELECT * FROM lanes WHERE id = $1", [id]);
    return rows[0] ?? null;
  },

  async create(
    pool: Pool,
    data: Omit<LaneRow, "id" | "created_at" | "updated_at"> & { id?: string },
  ): Promise<LaneRow> {
    const id = data.id ?? newId();
    const { rows } = await pool.query<LaneRow>(
      `INSERT INTO lanes (id, project_id, provider_id, endpoint, base_url, egress, policies, credential_ref)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *`,
      [id, data.project_id, data.provider_id, data.endpoint, data.base_url, data.egress, data.policies, data.credential_ref],
    );
    return rows[0]!;
  },

  async update(
    pool: Pool,
    id: string,
    data: Partial<Pick<LaneRow, "endpoint" | "base_url" | "egress" | "policies" | "provider_id" | "credential_ref">>,
  ): Promise<LaneRow | null> {
    const fields: string[] = [];
    const values: unknown[] = [id];
    let idx = 2;
    const allowed = ALLOWED_LANE_UPDATE_FIELDS;
    for (const [k, v] of Object.entries(data).filter(([k]) => allowed.has(k))) {
      if (v !== undefined) {
        fields.push(`${k} = $${idx}`);
        values.push(v);
        idx++;
      }
    }
    if (fields.length === 0) return lanes.get(pool, id);
    const { rows } = await pool.query<LaneRow>(
      `UPDATE lanes SET ${fields.join(", ")}, updated_at = now() WHERE id = $1 RETURNING *`,
      values,
    );
    return rows[0] ?? null;
  },

  async remove(pool: Pool, id: string): Promise<void> {
    await pool.query("DELETE FROM lanes WHERE id = $1", [id]);
  },
};

// ── Runs ────────────────────────────────────────────────────────────────

export type RunRow = {
  id: string;
  workflow_id: string;
  workflow_version: number;
  snapshot_version: number;
  plan_hash: string | null;
  status: string;
  input_body: unknown;
  output: unknown;
  error: string | null;
  started_at: string;
  completed_at: string | null;
};

export const runs = {
  async create(
    pool: Pool,
    data: {
      workflow_id: string;
      workflow_version: number;
      snapshot_version: number;
      plan_hash?: string | null;
      input_body?: unknown;
    },
  ): Promise<RunRow> {
    const { rows } = await pool.query<RunRow>(
      `INSERT INTO runs (id, workflow_id, workflow_version, snapshot_version, plan_hash, status, input_body)
       VALUES ($1,$2,$3,$4,$5,'running',$6) RETURNING *`,
      [
        newId(),
        data.workflow_id,
        data.workflow_version,
        data.snapshot_version,
        data.plan_hash ?? null,
        data.input_body ?? null,
      ],
    );
    return rows[0]!;
  },

  async update(
    pool: Pool,
    id: string,
    patch: { status?: string; output?: unknown; error?: string | null; completed_at?: string },
  ): Promise<RunRow | null> {
    // Guard: only allow known columns to prevent SQL injection through future
    // callers that might pass untrusted field names.
    const fields: string[] = [];
    const values: unknown[] = [id];
    let idx = 2;
    for (const [key, val] of Object.entries(patch)) {
      if (val !== undefined && ALLOWED_RUN_UPDATE_FIELDS.has(key)) {
        fields.push(`${key} = $${idx}`);
        values.push(val);
        idx++;
      }
    }
    if (fields.length === 0) return runs.get(pool, id);
    const { rows } = await pool.query<RunRow>(
      `UPDATE runs SET ${fields.join(", ")} WHERE id = $1 RETURNING *`,
      values,
    );
    return rows[0] ?? null;
  },

  async get(pool: Pool, id: string): Promise<RunRow | null> {
    const { rows } = await pool.query<RunRow>("SELECT * FROM runs WHERE id = $1", [id]);
    return rows[0] ?? null;
  },

  async list(pool: Pool, workflowId?: string, projectId?: string): Promise<RunRow[]> {
    const conditions: string[] = [];
    const params: unknown[] = [];
    let idx = 1;
    if (workflowId) {
      conditions.push(`r.workflow_id = $${idx++}`);
      params.push(workflowId);
    }
    if (projectId) {
      conditions.push(`w.project_id = $${idx++}`);
      params.push(projectId);
    }
    const where = conditions.length > 0 ? `WHERE ${conditions.join(" AND ")}` : "";
    const { rows } = await pool.query<RunRow>(
      `SELECT r.* FROM runs r
       LEFT JOIN workflows w ON w.id = r.workflow_id
       ${where}
       ORDER BY (r.status = 'running') DESC, r.started_at DESC LIMIT 200`,
      params,
    );
    return rows;
  },
};

// ── Publications ───────────────────────────────────────────────────────

export const publications = {
  async listByWorkflow(pool: Pool, workflowId: string, limit = 50): Promise<PublicationRow[]> {
    const { rows } = await pool.query<PublicationRow>(
      "SELECT * FROM publications WHERE workflow_id = $1 ORDER BY published_at DESC LIMIT $2",
      [workflowId, limit],
    );
    return rows;
  },

  async create(
    pool: Pool,
    workflowId: string,
    workflowVersion: number,
    planHash: string,
    snapshotVersion: number,
    status: "succeeded" | "failed",
    error: string | null = null,
  ): Promise<PublicationRow> {
    const { rows } = await pool.query<PublicationRow>(
      `INSERT INTO publications (id, workflow_id, workflow_version, plan_hash, snapshot_version, status, error)
       VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING *`,
      [newId(), workflowId, workflowVersion, planHash, snapshotVersion, status, error],
    );
    return rows[0]!;
  },
};