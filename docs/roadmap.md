# ROADMAP.md
> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** Phase 0–9 task breakdown and completion status.

## Phase 0 — Foundation

- [ ] Repository structure
- [ ] Rust workspace
- [ ] TypeScript control-plane workspace
- [ ] React Flow web app
- [x] CI
- [x] formatting/linting
- [ ] local observability

## Phase 1 — High-performance proxy

- [x] Rust HTTP server
- [x] streaming pass-through
- [x] connection pooling
- [ ] authentication middleware
- [x] metrics/tracing
- [x] benchmark harness
- [x] direct baseline comparison

## Phase 2 — Protocol layer

- [x] canonical request/event model
- [x] Anthropic adapter
- [x] OpenAI Chat Completions adapter
- [x] OpenAI Responses adapter
- [x] protocol conformance fixtures
- [x] streaming correctness (SSE parser + boundary tests)
- [x] capability matrix
- [x] performance benchmarks for translation
- [x] gateway integration (protocol translation via `ProtocolEngine`)
- [x] property tests (SSE parser + canonical roundtrip)

## Phase 3 — Lanes and routing

- [ ] lane registry
- [ ] endpoint registry
- [ ] health checks
- [ ] connection pools per lane
- [ ] direct route
- [ ] proxy route
- [ ] WireGuard integration
- [ ] fallback routing

## Phase 4 — Workflow compiler

- [x] workflow schema (types + validation — `crates/workflow-schema`)
- [x] semantic validator (`Workflow::validate()`: cycles, reachability, dead-end detection)
- [x] compiler (`crates/workflow-runtime/src/compiler.rs` — schema + lane validation)
- [x] execution IR (`ExecutionPlan`, versioned + content-hashed via `workflow-runtime`)
- [x] fast-path classification (`PlanClassification` — simple / translated / workflow)
- [x] runtime executor (`NodeRuntime::execute` — LLM/Transform/Condition/Router/Fallback/Retry)
- [x] gateway workflow execution (`GatewayServer::with_snapshot` + `workflow_id` routes)

## Phase 5 — Runtime publication & real workflow wiring

- [x] snapshot publication abstraction (`SnapshotPublisher` / `SnapshotReader`)
- [x] atomic hot-swap (`InMemoryPublisher` via `ArcSwap`)
- [x] gateway snapshot integration (request → snapshot lookup → compiled plan → classify)
- [x] end-to-end compiled workflow execution (fast path, interpreter, fallback, streaming)
- [x] React Flow → Workflow JSON serializer (`workflow-serializer.ts`)
- [x] frontend API boundary (React Query: `usePublishWorkflow` → gateway admin `/publish`)
- [x] per-lane connection-pool isolation (`LanePools` + `HyperPoolBuilder`)
- [x] protocol translation-loss handling (request-aware loss gate)
- [x] `ExecutionContext` capability plumbing (snapshot metadata / lane clients / milestone reporter)
- [x] integration tests: snapshot publication, hot-swap, workflow e2e, context capabilities
- [x] performance benchmarks for snapshot publication + reader path

## Phase 5b — Control plane (durable state) — COMPLETE (Phase 6)

- [x] PostgreSQL-backed control plane (workflow/provider/lane CRUD)
- [x] workflow lifecycle DB (DRAFT → VALIDATED → COMPILED → PUBLISHED → ACTIVE)
- [x] admin REST endpoints beyond `/publish` (fetch/validate/compile/rollback)
- [x] credential references (never raw secrets in workflow JSON)

## Phase 6 — Durable configuration + compilation + coherent runtime publication (COMPLETE)

- [x] `RuntimeSnapshotBundle` — snapshot + lane pools acquired atomically per request
- [x] PostgreSQL control-plane persistence (projects, providers, lanes, workflows, workflow_versions, publications, workflow_active)
- [x] immutable workflow versions (new edits create new versions; never mutate active)
- [x] publish pipeline (validate → compile → atomic publish → persist publication record)
- [x] rollback = republish a previous validated version
- [x] credential references resolved to lane `Authorization` at publish time
- [x] gateway `/validate` compile-only endpoint (deterministic plan hash before commit)
- [x] control-plane boot rehydrates the last ACTIVE version of every workflow
- [x] frontend wired to real control-plane lifecycle (versions index, publish, status)
- [x] full E2E: control plane → WireSnapshot → gateway atomic publish → request → provider → stream
- [x] hot path free of PostgreSQL/control-plane/compile (67 ns bundle lookup, memory-only)

## Phase 6.5 — Frontend/Backend Reality & Integration Hardening (AUDIT COMPLETE, IMPLEMENTATION COMPLETE)

**Why this phase exists:** The Phase 6.5 reality audit ([phase-6.5-reality-audit.md](archive/phase-6.5-reality-audit.md)) found that the Rust backend and control plane are real, but the frontend presents a high-fidelity mock: 13 of 15 pages render fabricated data, the Run button is a `setTimeout` animation, the editor cannot load saved workflows, and the Save/Validate buttons are non-functional. The backend correctly rejects invalid workflows, but the frontend never asks it.

**Confirmed gaps → implementation status (all closed):**
- Run button was entirely fabricated (setTimeout animation, zero API calls) → **REAL**: control-plane `POST /workflows/:id/run` → gateway admin `/run` → workflow runtime → provider, with the real envelope (request id, snapshot version, plan hash, output) shown in the UI; AbortController cancels the real request
- Invalid/empty workflows appeared to execute successfully → **REAL errors**: no ACTIVE version → 409 "must be published"; draft/invalid canvas rejected honestly
- 13 of 15 frontend pages rendered static/mock data from `relay-data.ts` (505 lines) and inline fixtures → **deleted**: `relay-data.ts` removed; every page fetches real backend rows or shows an honest "not available yet" state
- Workflow editor started from a hardcoded 14-node demo graph → **removed**: `graph.ts` is a 2-node empty starter
- No editor load/deserialize path (one-directional persistence) → **real load**: `deserializeWorkflow(latest.workflow_json)` reconstructs the canvas from the backend
- Save and Validate toolbar buttons had no onClick handler → **wired**: Save creates an immutable version (create-then-navigate for new workflows), Validate returns the real plan hash / real rejection
- Run detail page ignored the URL parameter → **honest**: `/runs` have no backend → "run history is not available yet" state, no fabricated waterfall
- AppShell rendered hardcoded workspace name, user, and status → **neutral** "local development" identity + real gateway health from control-plane `/system/health`
- Observability charts used Math.sin/cos fabricated time series → **honest** "No telemetry available" state
- `fetchLanes()` and `validateWorkflow()` defined in api.ts but never called → **wired**: lanes page lists persisted lanes; Validate button calls the real endpoint

**Success criteria — all met (see [PHASE6.5_IMPLEMENTATION_REPORT.md](archive/PHASE6.5_IMPLEMENTATION_REPORT.md)):**
- Every frontend page that shows domain data fetches it from the backend (no `relay-data.ts` in production code paths) ✅

## Phase 6.6 — Canonical Workflow Model + Editor Hardening (COMPLETE)

**What was built:** Typed workflow model with canonical serialization from the React Flow canvas. The editor now emits a schema-contractually correct `workflow_json` through the `workflow-serializer.ts` module (kind/port mapping, lane folding, condition validation, reject-on-invalid editor state). Wire-compat tests (`crates/workflow-schema/tests/wire_compat.rs`) prove the web serializer's JSON parses through the Rust `workflow_schema` crate — no schema-contract drift is possible. See [PHASE6.6_REPORT.md](archive/PHASE6.6_REPORT.md).
- The Run button calls the real gateway execution path and surfaces actual results/errors ✅
- The workflow editor loads a saved workflow version and reconstructs the React Flow canvas ✅
- Empty/invalid workflows are rejected with a clear error (backed by the real `/validate` endpoint) ✅
- Execution displays real results, not simulated progress (the workflow contract is a JSON envelope — per-token SSE is a proxy-route feature, not a workflow-run feature, and is not invented) ✅
- Documentation accurately reflects the real/unavailable split ✅

**What this phase does NOT include (deferred to Phase 7+):**
- MCP/Skills runtime implementation (registry, discovery, tool execution)
- Policy engine enforcement
- Authentication/authorization on gateway or control-plane APIs
- Tenant isolation / multi-project support
- Network lane health checks / WireGuard
- Secret manager integration (vault)
- Observability backend (Prometheus/OTLP integration)
- Run-history/execution-trace backend (the `/runs` pages stay honestly unavailable until one exists)

## Phase 7 — MCP and Skills

- [ ] MCP registry
- [ ] metadata index
- [ ] dynamic discovery
- [ ] deferred tool activation
- [ ] Skill registry
- [ ] progressive loading
- [ ] retrieval evaluation suite

## Phase 8 — Production security

- [ ] secret manager integration
- [ ] tenant isolation
- [ ] policy engine
- [ ] SSRF protections
- [ ] sandboxed tool workers
- [ ] audit log

## Phase 9 — Advanced routing

- [ ] latency-aware routing
- [ ] cost-aware routing
- [ ] capacity-aware routing
- [ ] provider health scoring
- [ ] adaptive capability retrieval
- [ ] workflow-level SLOs

## Explicitly defer

- arbitrary runtime plugin installation
- marketplace
- embedded code execution in gateway process
- provider support count as a vanity metric
