/**
 * Version compare — structural diff of two persisted WorkflowJson versions.
 *
 * Both versions are parsed through the canonical serializer to normalize
 * schema kinds (e.g. provider -> llm) and field ordering, then diffed
 * node-by-node (matched by id) and edge-by-edge.
 *
 * The output is a flat list of "change" records suitable for rendering in
 * a two-column diff panel.
 */

import { fromWorkflowJson, type WorkflowJson } from "@/lib/workflow";
import type { CanonicalEdge, CanonicalNode } from "@/lib/workflow/nodes";

// -- Types ----------------------------------------------------------------

export type DiffKind = "added" | "removed" | "changed";

export interface NodeDiff {
  type: "node";
  kind: DiffKind;
  nodeId: string;
  schemaKind: string;
  fields?: FieldChange[];
}

export interface EdgeDiff {
  type: "edge";
  kind: DiffKind;
  edgeLabel: string;
  fields?: FieldChange[];
}

export interface FieldChange {
  field: string;
  oldValue: unknown;
  newValue: unknown;
}

export type DiffEntry = NodeDiff | EdgeDiff;

export interface CompareResult {
  nodes: DiffEntry[];
  edges: DiffEntry[];
  addedCount: number;
  removedCount: number;
  changedCount: number;
}

// -- Helpers --------------------------------------------------------------

function deepEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (a === null || b === null) return false;
  if (typeof a !== typeof b) return false;

  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (!deepEqual(a[i], (b as unknown[])[i])) return false;
    }
    return true;
  }

  if (typeof a === "object") {
    const keysA = Object.keys(a as Record<string, unknown>);
    const keysB = Object.keys(b as Record<string, unknown>);
    if (keysA.length !== keysB.length) return false;
    for (const k of keysA) {
      if (!deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k])) return false;
    }
    return true;
  }

  return false;
}

/** Strip position/presentation/version for config-only comparison.
 *  `version` is serializer-assigned (always 1) and never semantic. */
function stripLayout(node: CanonicalNode): Record<string, unknown> {
  const out: Record<string, unknown> = { ...node };
  delete out["position"];
  delete out["presentation"];
  delete out["version"];
  return out;
}

/** Edge fields that carry semantics (excluding id). */
function edgeSemantics(edge: CanonicalEdge): Record<string, unknown> {
  const out: Record<string, unknown> = { ...edge };
  delete out["id"];
  return out;
}

function diffByKey<T>(
  a: T[],
  b: T[],
  keyFn: (item: T) => string,
  changeFn: (a: T, b: T) => FieldChange[] | null,
  meta: { type: "node"; kindFn: (item: T) => string } | { type: "edge"; labelFn: (item: T) => string },
): DiffEntry[] {
  const aMap = new Map(a.map((item) => [keyFn(item), item]));
  const bMap = new Map(b.map((item) => [keyFn(item), item]));
  const result: DiffEntry[] = [];
  const allKeys = new Set([...aMap.keys(), ...bMap.keys()]);

  for (const key of allKeys) {
    const inA = aMap.get(key);
    const inB = bMap.get(key);

    if (inA !== undefined && inB === undefined) {
      if (meta.type === "node") {
        result.push({ type: "node", kind: "removed", nodeId: key, schemaKind: meta.kindFn(inA) });
      } else {
        result.push({ type: "edge", kind: "removed", edgeLabel: meta.labelFn(inA) });
      }
    } else if (inA === undefined && inB !== undefined) {
      if (meta.type === "node") {
        result.push({ type: "node", kind: "added", nodeId: key, schemaKind: meta.kindFn(inB) });
      } else {
        result.push({ type: "edge", kind: "added", edgeLabel: meta.labelFn(inB) });
      }
    } else if (inA !== undefined && inB !== undefined) {
      const fields = changeFn(inA, inB);
      if (fields && fields.length > 0) {
        if (meta.type === "node") {
          result.push({ type: "node", kind: "changed", nodeId: key, schemaKind: meta.kindFn(inB), fields });
        } else {
          result.push({ type: "edge", kind: "changed", edgeLabel: meta.labelFn(inB), fields });
        }
      }
    }
  }

  return result;
}

// -- Public API -----------------------------------------------------------

/**
 * Compare two WorkflowJson versions and return a structured diff.
 * Both are parsed through the canonical serializer to normalize.
 */
export function compareVersions(
  oldJson: WorkflowJson,
  newJson: WorkflowJson,
): CompareResult {
  const oldParsed = fromWorkflowJson(oldJson);
  const newParsed = fromWorkflowJson(newJson);

  const oldNodes = oldParsed.workflow.nodes;
  const newNodes = newParsed.workflow.nodes;
  const oldEdges = oldParsed.workflow.edges;
  const newEdges = newParsed.workflow.edges;

  const nodeDiffs = diffByKey(
    oldNodes,
    newNodes,
    (n) => n.id,
    (a, b) => {
      const fieldsA = stripLayout(a);
      const fieldsB = stripLayout(b);
      if (deepEqual(fieldsA, fieldsB)) return null;
      const changes: FieldChange[] = [];
      if (!deepEqual(a.type, b.type)) {
        changes.push({ field: "kind", oldValue: a.type, newValue: b.type });
      }
      if (!deepEqual(a.config, b.config)) {
        const aCfg = a.config as Record<string, unknown>;
        const bCfg = b.config as Record<string, unknown>;
        const allCfgKeys = new Set([...Object.keys(aCfg), ...Object.keys(bCfg)]);
        for (const k of allCfgKeys) {
          if (!deepEqual(aCfg[k], bCfg[k])) {
            changes.push({ field: `config.${k}`, oldValue: aCfg[k] ?? null, newValue: bCfg[k] ?? null });
          }
        }
      }
      if (!deepEqual(a.ports, b.ports)) {
        changes.push({ field: "ports", oldValue: a.ports, newValue: b.ports });
      }
      return changes.length > 0 ? changes : null;
    },
    { type: "node", kindFn: (n) => n.type },
  );

  const edgeDiffs = diffByKey(
    oldEdges,
    newEdges,
    // Key by the canonical edge id (always present — serializer assigns
    // `e.id ?? "e-<index>"`), never by ports alone: parallel edges between
    // the same source/target ports (e.g. condition true/false branches) are
    // distinct edges and must not collapse into one diff entry.
    (e) => e.id,
    (a, b) => {
      const fieldsA = edgeSemantics(a);
      const fieldsB = edgeSemantics(b);
      if (deepEqual(fieldsA, fieldsB)) return null;
      // CanonicalEdge fields: source, sourcePort, target, targetPort, label
      // Only report label changes as config-level diffs; structural
      // source/target swaps are already captured by the key mismatch.
      const changes: FieldChange[] = [];
      if (!deepEqual(a.label, b.label)) {
        changes.push({ field: "label", oldValue: a.label ?? null, newValue: b.label ?? null });
      }
      return changes.length > 0 ? changes : null;
    },
    { type: "edge", labelFn: (e) => `${e.source}:${e.sourcePort}->${e.target}:${e.targetPort}` },
  );

  let addedCount = 0;
  let removedCount = 0;
  let changedCount = 0;
  for (const d of [...nodeDiffs, ...edgeDiffs]) {
    if (d.kind === "added") addedCount++;
    else if (d.kind === "removed") removedCount++;
    else changedCount++;
  }

  return { nodes: nodeDiffs, edges: edgeDiffs, addedCount, removedCount, changedCount };
}
