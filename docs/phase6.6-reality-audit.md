# Phase 6.6 Reality Audit

**Date:** 2026-09-14
**Auditor:** Adversarial QA / Verification Engineer
**Branch:** feat/phase6.5-review (commit cdeaa96)

---

## EXECUTIVE SUMMARY

**Verdict: PARTIALLY VERIFIED**

The backend is real. The data plane (Rust gateway), control plane (Fastify + PostgreSQL), protocol engine, workflow runtime, and compiler are production-quality implementations that work. The frontend data loading, serializer, and validation layers are also real.

**However, the four primary user-facing actions — Save, Validate, Publish, and Run — are all toast-only stubs that never make HTTP calls.** The `RunPanel` component returns `null`. Undo/Redo buttons have no click handlers. This means a user can load, view, and configure a workflow, but cannot persist changes, validate against the backend, publish to the gateway, or execute anything through the UI.

The API functions and React Query hooks to wire these buttons already exist and are fully implemented. The gap is ~20 lines of wiring code, not missing infrastructure.

---

## 1. CRITICAL DEFECTS

### P1-001: Save button is a stub (no HTTP call)

**File:** `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx:316-324`
**Expected:** Serialize workflow → POST to control plane → persist in PostgreSQL → update UI
**Observed:** `toast.info("Save wired to the control plane — run bun run dev to persist.")`
**Evidence:** Lines 316-324: the `onSave` callback only shows a toast and clears `dirty`. No `saveWorkflowVersion` or `useSaveWorkflowMutation` is called. The real hook exists at `lib/use-workflow-publication.ts:81-91`.

### P1-002: Validate button is a stub (no HTTP call)

**File:** `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx:326-334`
**Expected:** Serialize workflow → POST /workflows/:id/validate → receive plan_hash from Rust compiler
**Observed:** `toast.info("Validation compiles against the Rust gateway (backend-authoritative).")` and hardcoded `setPlanHash("local-schema-ok")`
**Evidence:** The `onValidate` callback never calls the real `validateWorkflow` API or `useValidateMutation` hook. It fabricates a plan hash "local-schema-ok" — a direct false positive.

### P1-003: Publish button is a stub (no HTTP call)

**File:** `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx:336-344`
**Expected:** Serialize workflow → POST /workflows/:id/publish → gateway compilation → atomic snapshot swap → persist publication record
**Observed:** `toast.info("Publish requires the control plane and gateways wired (Phase 6.5).")`
**Evidence:** The `onPublish` callback never calls `publishWorkflow` or `usePublishWorkflow`. Both exist and are fully implemented.

### P1-004: Run button is a stub (no HTTP call, empty panel)

**File:** `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx:355-363, 550-552`
**Expected:** POST /workflows/:id/run → gateway execution → real LLM response → render result
**Observed:** `submitRun` shows a toast; `RunPanel` component returns `null` (renders nothing)
**Evidence:** Line 550-552: `function RunPanel(_props: ...) { return null; }`. The real `runWorkflow` API and `useRunWorkflowMutation` hook exist but are never called.

### P1-005: Undo/Redo buttons have no handlers

**File:** `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx:99-104`
**Expected:** Undo/redo workflow changes
**Observed:** Buttons render but have no `onClick` handler. They are purely decorative.
**Evidence:** No undo/redo state management exists in the component.

---

## 2. REALITY MATRIX

| Feature | UI Exists | Configurable | API Exists | Backend Works | Persistence Works | E2E Proven | Fake Risk |
|---------|:---------:|:------------:|:----------:|:-------------:|:-----------------:|:----------:|:---------:|
| Workflow creation | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | LOW |
| Node configuration (Inspector) | ✅ | ✅ | — | — | ⚠️ | ⚠️ | MEDIUM |
| Save | ✅ (stub) | — | ✅ | ✅ | ✅ | ❌ | **HIGH** |
| Validate | ✅ (stub) | — | ✅ | ✅ | ✅ | ❌ | **HIGH** |
| Compile | ✅ (stub) | — | ✅ | ✅ | ✅ | ❌ | **HIGH** |
| Publish | ✅ (stub) | — | ✅ | ✅ | ✅ | ❌ | **HIGH** |
| Run | ✅ (stub) | — | ✅ | ✅ | ✅ | ❌ | **HIGH** |
| Streaming | — | — | — | ✅ (server) | — | ❌ | N/A |
| Provider routing | — | — | ✅ | ✅ | ✅ | ⚠️ | LOW |
| Lane routing | — | — | ✅ | ✅ | ✅ | ⚠️ | LOW |
| Conditions | ✅ (config) | ✅ | ✅ | ✅ | ✅ | ⚠️ | LOW |
| Retry | ✅ (config) | ✅ | ✅ | ✅ | ✅ | ⚠️ | LOW |
| Fallback | ✅ (config) | ✅ | ✅ | ✅ | ✅ | ⚠️ | LOW |
| MCP | ✅ (config) | ✅ | ✅ | ✅ | — | ⚠️ | MEDIUM |
| Observability | ✅ (page) | — | ✅ | ✅ | ✅ | ⚠️ | LOW |

Legend: ✅ = real and working | ⚠️ = real infrastructure exists but not E2E proven | ❌ = broken/stub | — = not applicable

---

## 3. WHAT IS REAL (with evidence)

### 3.1 Rust Gateway (Data Plane)
- **HTTP server:** Axum-based, dual listeners (proxy + admin), real `TcpListener::bind`, `tokio::select!` shutdown. (`apps/gateway/src/server/mod.rs`)
- **Publication hot-swap:** `ArcSwap<PublishedBundle>` — atomic snapshot + lane pool swap. (`apps/gateway/src/observability/mod.rs:30-80`)
- **Admin endpoints:** `/publish`, `/validate`, `/run` with Bearer auth. Run calls `execute_workflow`. (`observability/mod.rs:430-610`)
- **LLM node:** Real HTTP POST to provider via Hyper client. Protocol encoding (OpenAI Chat, Anthropic Messages, OpenAI Responses). Streaming SSE decoding with incremental fold. Cancellation + frame timeout. (`crates/workflow-runtime/src/nodes/llm.rs`)
- **Lane pools:** Per-lane Hyper client isolation. `LanePools::build` creates distinct pools per lane identity. (`apps/gateway/src/lanes.rs`)
- **Workflow compiler:** Schema validation → lane reference validation → `ExecutionPlan::compile` (topological sort, plan hash, classification). (`crates/workflow-runtime/src/compiler.rs`)
- **Execution engine:** Topological-order node execution, port-based data routing, conditional edge evaluation, router round-robin, deadline enforcement. (`crates/workflow-runtime/src/execution.rs`)

### 3.2 Control Plane (TypeScript)
- **PostgreSQL:** Real `pg` Pool, idempotent migrations (9 tables), parameterized queries. (`apps/control-plane/src/db/`)
- **Fastify API:** 15+ routes, all backed by real PostgreSQL CRUD. (`apps/control-plane/src/api/routes.ts`)
- **Publish service:** Transactional validate→compile→publish→record pipeline. Monotonic snapshot version. (`apps/control-plane/src/domain/publish.ts`)
- **Gateway client:** Real HTTP `fetch()` to gateway admin endpoints. (`apps/control-plane/src/gateway/client.ts`)
- **Rehydration:** On boot, republishes last ACTIVE version of every workflow to restore gateway state. (`apps/control-plane/src/index.ts:44-68`)

### 3.3 Frontend
- **Data loading:** React Query hooks fetch real data from control plane (workflows, versions, lanes, providers, health). (`apps/web/src/lib/use-workflow-publication.ts`)
- **Serializer:** Lossless canonical model ↔ persisted JSON. Round-trip tested. No fabricated values. (`apps/web/src/lib/workflow/serializer.ts`)
- **Validation:** 3-layer validation (structural, schema, semantic with real lane refs). (`apps/web/src/lib/workflow/validation.ts`)
- **API client:** Real `fetch()` calls with proper error handling. (`apps/web/src/lib/api.ts`)
- **Pages:** Overview, Workflows list, Versions, Providers, Lanes, Health all use real hooks.

### 3.4 Tests
- **Total:** 290 Rust tests + 23 frontend tests = **313 tests, all passing** (verified 2026-09-14)
- **Control plane tests:** Real PostgreSQL per test suite (`freshDb`), real Fastify app, real HTTP `fetch()` calls. Tests CRUD, publish pipeline, SQL injection. (`apps/control-plane/tests/`)
- **Serializer tests:** 15+ tests verifying round-trip, no-fabrication, unsupported-node handling, legacy migration. (`apps/web/src/lib/workflow-serializer.test.ts`)
- **Compiler tests:** Real workflow compilation, lane validation, plan hash determinism, fast-path classification. (`crates/workflow-runtime/src/compiler.rs` tests)
- **Integration tests:** Real workflow execution through compiled plans. (`crates/workflow-runtime/tests/integration.rs`)

---

## 4. FALSE-POSITIVE REPORT

### FP-001: Validate button fabricates plan hash
**Location:** `WorkflowBuilder.tsx:333`
**Behavior:** `setPlanHash("local-schema-ok")` — a hardcoded string presented as a real plan hash
**Why it's a false positive:** The footer shows `✓ local-schema-ok — validated` which looks like a successful backend compilation. In reality, the backend Rust compiler was never contacted.

### FP-002: Save shows "saved" status without persistence
**Location:** `WorkflowBuilder.tsx:323`
**Behavior:** `setDirty(false)` after the toast, making the footer show `✓ saved`
**Why it's a false positive:** The UI indicates the workflow is saved, but nothing was persisted. Refreshing the page would lose all changes.

### FP-003: Inspector shows "Served by Rust gateway data plane"
**Location:** `Inspector.tsx:238`
**Behavior:** Static text "Rust gateway data plane" in the Execution section
**Why it's misleading:** This is presentation text, not a live connection indicator. It implies the node is connected to the gateway even though no execution has occurred.

---

## 5. NODE CONFIGURATION AUDIT

| Node | Inspector UI | Fields | Config Changes State | Serialized | Persisted | Compiled | Runtime Used |
|------|:----------:|:------:|:-------------------:|:----------:|:---------:|:--------:|:------------:|
| Input | ✅ | none | — | ✅ | ✅ (if wired) | ✅ | ✅ passthrough |
| Output | ✅ | none | — | ✅ | ✅ (if wired) | ✅ | ✅ passthrough |
| Provider (LLM) | ✅ | 7 fields | ✅ | ✅ | ✅ (if wired) | ✅ | ✅ real HTTP |
| Route (Router) | ✅ | 1 field | ✅ | ✅ | ✅ (if wired) | ✅ | ✅ round-robin |
| Transform | ✅ | 1 field | ✅ | ✅ | ✅ (if wired) | ✅ | ✅ |
| Condition | ✅ | 4 fields | ✅ | ✅ | ✅ (if wired) | ✅ | ✅ branching |
| MCP | ✅ | 2 fields | ✅ | ✅ | ✅ (if wired) | ✅ | ⚠️ stub |
| Skill | ✅ | 2 fields | ✅ | ✅ | ✅ (if wired) | ✅ | ⚠️ stub |
| Fallback | ✅ | 1 field | ✅ | ✅ | ✅ (if wired) | ✅ | ✅ |
| Retry | ✅ | 5 fields | ✅ | ✅ | ✅ (if wired) | ✅ | ✅ |
| Lane | display-only | — | — | blocks publish | — | — | — |
| Endpoint | display-only | — | — | blocks publish | — | — | — |
| Tool | display-only | — | — | blocks publish | — | — | — |
| Agent | display-only | — | — | blocks publish | — | — | — |
| Policy | display-only | — | — | blocks publish | — | — | — |
| Observability | display-only | — | — | blocks publish | — | — | — |

**Key issue:** The "Persisted" column says "if wired" because the Save button is a stub. Node configuration IS serialized correctly through the canonical model, but never actually saved.

---

## 6. BUTTON/ACTION AUDIT

| Button | Visible | Clickable | Real API | Persistence | Error Handling | E2E |
|--------|:-------:|:---------:|:--------:|:-----------:|:--------------:|:---:|
| Save | ✅ | ✅ | ❌ stub | ❌ | ❌ (no error path) | ❌ |
| Validate | ✅ | ✅ | ❌ stub | — | ❌ (no error path) | ❌ |
| Publish | ✅ | ✅ | ❌ stub | ❌ | ❌ (no error path) | ❌ |
| Run | ✅ | ✅ | ❌ stub | — | ❌ | ❌ |
| Stop | ✅ | ✅ | — | — | — | ❌ |
| Undo | ✅ | ✅ | ❌ no handler | — | — | ❌ |
| Redo | ✅ | ✅ | ❌ no handler | — | — | ❌ |
| Toggle Library | ✅ | ✅ | — | — | — | ✅ local |
| Toggle Inspector | ✅ | ✅ | — | — | — | ✅ local |
| Add Node (drag) | ✅ | ✅ | — | — | — | ✅ local |
| Versions link | ✅ | ✅ | ✅ (navigates) | — | — | ✅ |

---

## 7. TEST INFRASTRUCTURE REPORT

- **Test PostgreSQL:** Real per-suite databases via `freshDb()` in `apps/control-plane/tests/helpers.ts`. Creates isolated DB, runs migrations, cleans up.
- **Mock Gateway:** In-process Fastify server returning deterministic responses. Not a mock of the database — only simulates gateway compilation/publish behavior.
- **Protocol fixtures:** `crates/mock-upstream/` provides a real Hyper HTTP server that echoes/delays SSE responses for testing streaming.
- **Frontend tests:** Bun test runner with `bun:test`. No browser-level E2E tests exist. All frontend tests are unit tests of the serializer and run-state logic.
- **No browser E2E:** There are no Playwright/Cypress/WebDriverIO tests. The frontend has never been tested through real user interaction.

---

## 8. DEFECTS SUMMARY

### P0 — Catastrophic: None
No unauthenticated endpoints, no credential exposure, no cross-tenant access.

### P1 — Critical (5 defects)
1. **P1-001:** Save button is a toast stub — no HTTP call, no persistence
2. **P1-002:** Validate button is a toast stub + fabricates plan hash
3. **P1-003:** Publish button is a toast stub — no publication
4. **P1-004:** Run button is a toast stub + RunPanel renders null
5. **P1-005:** Undo/Redo buttons are decorative (no handlers)

### P2 — Major (2 defects)
1. **P2-001:** `validateConfig` in `validation.ts:76` always returns `undefined` for `configNodeId` — validation issues are never attributed to their source node in the config validation layer (the workflow-level `validateWorkflow` does attribute via the outer loop, masking this in most paths)
2. **P2-002:** No browser-level E2E tests exist — the entire frontend has zero interaction testing

### P3 — Minor (2 defects)
1. **P3-001:** Inspector shows static "Served by Rust gateway data plane" text that implies a live connection
2. **P3-002:** `RunPanel` type import at line 548 (`import type { RunState } from "@/lib/run-state"`) shadows the local `RunState` type defined at line 12 of `nodes.tsx` — harmless but confusing

---

## 9. ARCHITECTURAL OBSERVATIONS

### What's done well
- Clean separation: API layer → React Query hooks → components
- Canonical serializer prevents data loss between editor and backend
- Backend publish pipeline is transactional with proper rollback
- Gateway publication is atomic (snapshot + pools swap together)
- Protocol adapters handle real provider wire formats
- Tests use real PostgreSQL for control plane tests
- Compiler validates lane references at compile time, not runtime

### What's missing
- **Wiring:** The ~20 lines connecting toolbar callbacks to existing hooks
- **RunPanel UI:** An entire component (runs and shows results)
- **Browser E2E tests:** No interaction testing at all
- **Execution history backend:** The runs pages are honest placeholders
- **MCP/Skill execution:** Backend stubs exist but don't actually execute external tools

---

## 10. RECOMMENDATION

The project has a **solid backend** and a **well-architected frontend** with a critical wiring gap. The fix is not a rewrite — it's connecting 4 existing hooks to 4 existing buttons, building the RunPanel component, and adding browser E2E tests to prove the connection works.

Estimated remediation: **1-2 days of focused wiring work** + E2E test suite.

The backend architecture (gateway execution, protocol translation, lane pools, snapshot publication) is genuine and production-ready. The frontend architecture (canonical serializer, validation, React Query hooks) is genuine and well-tested. The gap is exclusively in the toolbar→hook connection layer.
