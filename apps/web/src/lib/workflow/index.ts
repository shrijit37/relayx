/**
 * Phase 6.6 canonical workflow model — public surface.
 *
 * ONE semantic model (nodes.ts) with definitions (node-definitions.ts),
 * validation (validation.ts) and the lossless serializer (serializer.ts).
 * The view-level `serializeWorkflow`/`deserializeWorkflow` live here too —
 * there is no legacy `lib/workflow-serializer` path anymore.
 */

export * from "./nodes";
export * from "./node-definitions";
export * from "./serializer";
export * from "./validation";
