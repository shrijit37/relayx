# ROADMAP.md

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
