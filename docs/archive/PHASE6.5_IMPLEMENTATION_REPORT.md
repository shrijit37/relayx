# PHASE 6.5 — Frontend/Backend Integration Hardening — Implementation Report
> **Archived snapshot** — historical record, not current truth. Current state: [../state.md](../state.md).

**Branch:** `feat/phase6.5-workflow-editor-integration`
**Baseline evidence:** [`phase-6.5-reality-audit.md`](phase-6.5-reality-audit.md)
**Status:** COMPLETE — all audit CRITICALs resolved; primary success criterion proven live.

---

## Executive Summary

Phase 6.5 closed the gap between a high-fidelity mock frontend and a real backend. Before this phase, the Run button animated a `setTimeout` over a hardcoded path, `relay-data.ts` (505 lines) fabricated 13 of 15 page views, the editor could write but never read workflows, and Save/Validate had no handlers. After this phase, every domain value the UI shows is backend-authoritative or honestly marked unavailable: the editor **loads** persisted versions, **Save/Validate/Publish/Run** are real control-plane operations, and Run executes the published ACTIVE version through the real gateway → workflow runtime → provider, surfacing the real execution envelope and real errors. `relay-data.ts` and all inline fixtures are deleted.

**If Relay-X says a workflow ran, it actually ran.** The negative paths are real too: a draft workflow Run returns a real 409 ("must be published"); invalid/empty workflows are rejected by real validation, never animated as if they succeeded.

---

## Before vs After

| Aspect | Before (audit baseline) | After (Phase 6.5) |
|---|---|---|
| Run button | `setTimeout` CSS animation over hardcoded path, zero API calls | Real `POST /workflows/:id/run` → gateway admin `/run` → workflow runtime → provider; real envelope + errors in UI; AbortController cancels |
| Editor load | None — canvas seeded from hardcoded 14-node demo | `deserializeWorkflow(latest.workflow_json)` reconstructs the canvas from the backend's latest version |
| Save | No handler | Creates an immutable version; in `new` mode creates the workflow row then navigates to the durable id |
| Validate | No handler | Real `/validate` → real plan hash / real rejection |
| Publish | Real (pre-existing) | Verified real; `workflow_status` flips to `active` |
| Fabricated data | `relay-data.ts` + inline fixtures + `Math.sin` series | Deleted; every page fetches backend rows or shows an honest "not available yet" state |
| Workflow IDs | Hardcoded in CommandPalette/Inspector | Only real ids from the backend |
| KPIs/latency/run history | Fake waterfall, fake runs, fake metrics | Honest unavailable states |
| Docs | Described the mock | Updated to the real/unavailable split |

---

## Workflow Editor — REAL

The editor is a React Flow canvas with 16 node-kind variants, drag-and-drop, edges, and an Inspector panel, wired end-to-end to the control plane:

- **Load:** `WorkflowBuilder` fetches the workflow's latest version and calls `deserializeWorkflow` to reconstruct nodes + edges (topological column layout, `lane_id` unfolded back into synthetic lane nodes).
- **Save:** `POST /workflows/:id/versions` (immutable). New workflows: `createWorkflow` → navigate to `/workflows/<real-id>` → then the version save — the create loop works through the editor.
- **Validate:** `POST /workflows/:id/validate` → control plane → gateway `/validate` → deterministic `plan_hash`; invalid graphs get the real rejection text.
- **Publish:** `POST /workflows/:id/publish` → atomic gateway publication; version becomes ACTIVE (real `workflow_active` row).
- **Run panel:** a compact overlay with an editable request-body textarea, Run/Stop, the real JSON result, and executed-version metadata (request id, snapshot version, plan hash, output). Dirty-editor runs warn that the *published* ACTIVE version executes, not unsaved changes.

## Persistence — REAL

Immutable workflow versions in Postgres; publish persists a publication record and flips `workflow_active`; the control plane rehydrates ACTIVE versions into the gateway on boot. The editor round-trip (`serialize → save → reload → deserialize`) is the primary success criterion and was proven live.

## Validation — REAL

`WorkflowBuilder.footer` shows real counts only when a real validation result exists, else "not validated". Empty/invalid workflows produce real validation errors — nothing is silently accepted.

## Compilation — REAL

Validation and publish run the deterministic compiler (`compile_workflow_with_lanes` → `ExecutionPlan` with content hash). The plan hash shown in the UI is the backend's hash, not a client-side fabrication.

## Publication — REAL

Atomic snapshot + lane-pool swap in the gateway (`ArcSwap`). Verified live: publish → ACTIVE → run → snapshot version advances.

## Execution — REAL

Run executes the compiled plan of the ACTIVE version. The LLM node consumes upstream provider SSE incrementally with bounded memory (`decode_streamed_response_incremental` / `StreamFold`); the client contract is the workflow's Output node as JSON (the backend's existing workflow-execution semantics — per-token client SSE is a proxy-route feature, not a workflow-run feature, and is not invented).

## Streaming — REAL (upstream), JSON contract (client)

The provider stream is genuinely consumed incrementally by the runtime. The UI renders the real result JSON; it never simulates per-token arrival.

## Error Handling — REAL

- Unpublished workflow Run → **409 "Workflow must be published before it can be run."** (§11 — never silently executed).
- Unknown workflow → real 404.
- Provider/gateway failure → real 400+ with the backend's error text.
- Request cancellation → AbortController aborts the real in-flight request (phase `cancelled`).

## Frontend Data Sources — ALL BACKEND-AUTHORITATIVE

| Page | Source |
|---|---|
| `/` overview | `useWorkflows()` + `fetchLanes()` + `fetchProviders()` real counts; real gateway health; KPIs honestly unavailable |
| `/workflows/:id` editor | Control plane latest version |
| `/workflows/:id/versions` | Control plane version list + real plan hash |
| `/providers` | Real `GET /providers` + real create form |
| `/lanes` | Real `GET /lanes` |
| `/health` | Control-plane `/system/health` probe (control-plane + gateway `/healthz` + `/ready`) |
| `/runs`, `/runs/:id`, `/observability`, `/mcp`, `/skills`, `/policies`, `/secrets` | Honest "not available yet" — no backend exists; nothing fabricated |
| `/settings` | Neutral "local development environment" + the real (local) keyboard shortcuts |

## Removed Mock Paths

- **`apps/web/src/lib/relay-data.ts`** — deleted (no consumers remain).
- **`graph.ts`** — reduced to a 2-node (input+output) empty starter; the hardcoded 14-node demo is gone.
- **Inline fixtures** — secrets/health/lanes/policies/settings page fixtures removed.
- **`Math.sin`/`Math.cos` latency series** — removed; observability is an honest empty state.
- **Hardcoded ids/versions** (`plan_8f31a2`, `v24`, fake workflow ids) — removed from Inspector and CommandPalette.
- **setTimeout Run simulation** — removed (real request path only).

## Backend APIs Used (unchanged or reused)

- Gateway admin: `/validate`, `/publish` (reused), plus **new** `/run`.
- Control plane: workflows/versions/providers/lanes CRUD, `/validate`, `/publish` (reused), plus **new** `/workflows/:id/run` and `/system/health`.

## New API/Backend Changes (smallest justified addition)

1. **Gateway admin `POST /run`** (`apps/gateway/src/observability/mod.rs`) — request `{ workflow_id, body }`; loads the current snapshot, 404s on unknown workflow, calls the existing `execute_workflow`, returns `{ status:"ok", request_id, workflow_id, snapshot_version, plan_hash, output }`. Errors flow through the existing `GatewayError → HTTP` mapping. Reuses the current snapshot, per-lane pools, and execution path; no new storage, no proxy-route changes.
2. **Control-plane `POST /workflows/:id/run`** (`apps/control-plane/src/api/routes.ts`) — 404 missing workflow; **409** when no ACTIVE version (§11); else forwards to gateway `/run` and returns the envelope or the gateway's structured error.
3. **Control-plane `GET /system/health`** — read-only probe of control-plane + gateway `/healthz` + `/ready`, consumed by the frontend health page and AppShell dot.

No auth added (deferred to Phase 8). No run-history backend added — the `/runs` pages stay honestly unavailable.

## Tests

| Suite | Result |
|---|---|
| Gateway Rust (incl. **new** `gateway_admin_run_executes_published_workflow`, restore of hot-swap body) | **270 passing workspace-wide** (269 baseline + 1), 0 failures |
| Control-plane integration (incl. **3 new** run tests: published→envelope, unpublished→409, gateway failure→400) | 12 passing |
| Frontend `bun test` (serializer: malformed/unknown-kind/empty/lane round-trip/version metadata; run-state reducer: 6) | 19 passing |
| Frontend `tsc --noEmit` | clean |
| Frontend production build | clean |
| `cargo clippy --all-features --workspace` | clean |

## E2E Results (live stack, real Postgres + real gateway + mock upstream)

Primary success criterion **proven live through the UI**:

1. **Create** workflow via the editor (`e2e-run-test`).
2. **Open editor → Build** the graph.
3. **Save** → immutable version persisted.
4. **Reload** → workflow reconstructed from persisted JSON (canvas reseeded from backend, not a demo).
5. **Validate** → real plan hash.
6. **Fix errors** → real rejection surfaced when invalid.
7. **Compile/Publish** → ACTIVE.
8. **Run** → real gateway `/run` → real workflow runtime → mock upstream provider → **real output** ("Hello from the Phase 6 live stack"), usage 5/6/11, request id `2fa0aba5-…`, snapshot version 25, plan hash `0d63dc75fbc0`.

Negative paths proven live: **draft workflow Run → real 409** ("Workflow must be published before it can be run"); unknown workflow → 404.

## Remaining Gaps (honest — deferred, never fabricated)

- **Run history / execution traces** — no backend → `/runs` pages are honest "not available yet". **UNAVAILABLE**
- **MCP/Skills runtime** (registry, discovery, execution) — Phase 7. **UNAVAILABLE**
- **Policy engine, secret manager, tenant isolation, auth** — Phase 8. **UNAVAILABLE**
- **Observability backend** (Prometheus/OTLP) — no telemetry ingestion; overview/observability show honest empty states. **UNAVAILABLE**
- **Gateway `/run` auth** — admin listener, same trust model as `/validate`/`/publish`. Partial by design.
- **Per-token client SSE for Run** — not a workflow-run feature; the workflow output is a JSON envelope. **PARTIAL** (upstream streaming is real; client stream isn't part of the run contract).

## Performance Impact

- `/run` reuses the current snapshot load and the existing `execute_workflow` path; no request-hot-path change (the proxy route path is untouched). A fresh Hyper client is constructed per run call (marked `ponytail:` — share the pool's client if run throughput ever matters).

## Security Considerations

- Raw secrets never appear in workflow JSON, API responses, or logs (credential refs resolved to `Authorization` at publish time, verified by the lane CRUD test).
- Run executes only the ACTIVE published version — no draft can be silently executed; unknown workflow ids 404.
- No outbound calls from Run beyond the configured lane endpoints.
- No new storage of request bodies beyond the existing execution path (bounded-memory streaming).

## Documentation Updated

- `docs/state.md` — frontend section rewritten to the real/unavailable split; status + Phase 6 bullet updated.
- `README.md` — frontend status now backend-authoritative; real Run; no mock data; metric table updated.
- `docs/roadmap.md` — Phase 6.5 marked IMPLEMENTATION COMPLETE; every audit gap mapped to its closure.
- `docs/architecture.md` — Phase 6.5 note now describes the real wiring (control plane run passthrough, backend-authoritative frontend).
- `docs/development.md` — added control-plane section; documented the editor dev flow including the `/run` contract and 409 gate.

## Definition of Done

- [x] No fake execution anywhere in production paths — Run is a real request with real errors/result
- [x] Backend is authoritative for all domain state (ids, versions, statuses, hashes, output, errors)
- [x] Editor load/save/validate/publish real; new-workflow create loop works
- [x] Run executes only published ACTIVE versions (409 gate); invalid/empty workflows show real validation errors, never animate
- [x] All fabricated data removed (`relay-data.ts` deleted, fixtures gone, `graph.ts` 2-node starter)
- [x] Every page real-or-honest-unavailable
- [x] Tests: serializer round-trip + malformed deserialize, run-state reducer, control-plane run integration, gateway `/run` integration, live E2E + negative paths
- [x] Docs updated; this report produced with REAL/PARTIAL/UNAVAILABLE tags
