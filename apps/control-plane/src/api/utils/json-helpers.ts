/**
 * Shared helpers for JSON graph canonicalization and Fastify request utilities.
 *
 * These are used by the workflow and publish route modules to match
 * version-stored graphs against submitted content (Review #5).
 */

import type { Pool } from "pg";
import * as repo from "../../db/repositories";

export function projectIdOf(req: import("fastify").FastifyRequest, fallback: string): string {
  return ((req.query as Record<string, string> | undefined)?.project_id ?? fallback);
}

/**
 * Find the stored version whose graph matches `json` on the compilable parts
 * (nodes + edges). Key order is canonicalized (recursively sorted) so a
 * semantically-identical JSON matches regardless of insertion order; the
 * serde `kind` tag inside configs and top-level identity fields are
 * excluded (serialization artifacts, not editor content). Review #5.
 */
export async function versionForJson(
  pool: Pool,
  workflowId: string,
  json: Record<string, unknown>,
): Promise<number | null> {
  const versions = await repo.workflows.listVersions(pool, workflowId);
  if (versions.length === 0) return null;
  const graphOf = (wj: Record<string, unknown>): string =>
    JSON.stringify(sortKeys([stripKind(wj.nodes), stripKind(wj.edges)]));
  const needle = graphOf(json);
  for (const v of versions) {
    if (graphOf(v.workflow_json as Record<string, unknown>) === needle) return v.version;
  }
  return null;
}

/** Recursively sort object keys (stable across JSON representations). */
function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(value as Record<string, unknown>).sort()) {
      out[k] = sortKeys((value as Record<string, unknown>)[k]);
    }
    return out;
  }
  return value;
}

/**
 * Strip the `kind` tag from config objects only.
 *
 * The graph layout is `node = {id, kind, config, inputs, outputs}` where
 * `config = {kind, …}`. The node-level `kind` is the semantic discriminator
 * ("llm" vs "router" — must be preserved); the config-level `kind` is a
 * serde artifact. We strip only the `kind` inside `config` (and recurse
 * into children) — never the node's own `kind`. Review D7.
 */
function stripKind(nodesOrValue: unknown): unknown {
  const isNodeList = Array.isArray(nodesOrValue) && nodesOrValue.every((n) => isNode(n));
  if (isNodeList) return nodesOrValue.map(stripNode);
  return nodesOrValue;
}

function isNode(v: unknown): boolean {
  return (
    v !== null &&
    typeof v === "object" &&
    typeof (v as Record<string, unknown>).kind === "string" &&
    typeof (v as Record<string, unknown>).id !== "undefined"
  );
}

function stripNode(node: unknown): unknown {
  const n = node as {
    id?: unknown;
    kind?: unknown;
    config?: unknown;
    inputs?: unknown;
    outputs?: unknown;
  };
  const config = n.config === undefined ? n.config : stripConfig(n.config);
  return { ...n, config };
}

/** Drop `kind` from this config object, recursively (children keep kinds). */
function stripConfig(value: unknown): unknown {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(stripConfig);
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (k === "kind") continue;
    out[k] = k === "config" ? stripConfig(v) : v;
  }
  return out;
}
